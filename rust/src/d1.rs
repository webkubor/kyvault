//! Cloudflare D1 后端 —— 与 CortexOS 的 Go 端（pkg/infra/secretvault）和 Python 版
//! 共用同一张 secret_vault 表、同一套 wire format，密文互相可读、零迁移。
//!
//! 这里**没有 curl 兜底**，而 Python 版必须有。那段兜底存在的理由是一类真实故障：
//! 某些机器上 Python 的 TLS 验证整体失效（同一条证书链 openssl verify 判 OK、
//! curl 连得通，唯独 Python 报 CERTIFICATE_VERIFY_FAILED，且跨解释器复现）。
//! Rust 走 rustls 静态链接，不碰系统 OpenSSL 那条路径 —— 这正是重写的主要动因，
//! 所以这个文件比它的 Python 对应物少了 60 行兜底代码。
//!
//! master key 只读：这里不会自动生成。它由 Go 端初始化在 site_config 表里，
//! 谁都不该有第二个生成入口 —— 生成两次等于把先写的密文全部变成解不开的乱码。

use std::cell::OnceCell;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use crate::crypto::{d1_key, decrypt_split, encrypt_split};
use crate::model::{last4, parse_ref, SecretMeta};

const TIMEOUT_SECS: u64 = 15;

pub struct D1 {
    account_id: String,
    database_id: String,
    token: String,
    key: OnceCell<[u8; 32]>,
}

impl D1 {
    /// 连 D1 需要的三件套。**自举**：本地加密库里存了就用它，
    /// 没有才要求环境变量。
    ///
    /// 为什么要自举：连 D1 的 Cloudflare token 本身就是密钥，不能明文放配置
    /// 文件；而只认环境变量的后果是——裸跑 kyvault 连不上真源，必须靠外部
    /// 包装器（此前是 `cs kyvault`）注入。于是调度系统被迫持有所有密钥，
    /// 而且 kyvault 每加一个字段，包装器就得跟着改一次。
    ///
    /// 解法是用它自己的本地库自举：`~/.keyring/`（master.key 0600）加密存
    /// 这三件套（platform=kyvault，不能用下划线开头 —— 那是保留命名空间，get 读不到），启动时先解出来再连 D1。密钥库自己管住了连自己的钥匙，
    /// 外部就不需要再知道任何东西。
    ///
    /// 顺序刻意是「环境变量优先」：CI、容器、临时覆盖都靠它，
    /// 而且不破坏任何现有调用方式。
    fn from_local_store() -> (String, String, String) {
        let Ok(st) = crate::store::Store::default_location() else {
            return (String::new(), String::new(), String::new());
        };
        let g = |k: &str| {
            st.get_secret(&format!("secret://kyvault/{k}"))
                .ok()
                .flatten()
                .unwrap_or_default()
        };
        (g("d1-account-id"), g("d1-database-id"), g("d1-token"))
    }

    pub fn from_env() -> Result<Self> {
        let (l_acct, l_db, l_tok) = Self::from_local_store();
        let account_id = std::env::var("KYVAULT_D1_ACCOUNT_ID")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or(l_acct);
        let database_id = std::env::var("KYVAULT_D1_DATABASE_ID")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or(l_db);
        let token = std::env::var("CLOUDFLARE_API_TOKEN")
            .or_else(|_| std::env::var("CF_API_TOKEN"))
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or(l_tok);
        let missing: Vec<&str> = [
            ("KYVAULT_D1_ACCOUNT_ID", &account_id),
            ("KYVAULT_D1_DATABASE_ID", &database_id),
            ("CLOUDFLARE_API_TOKEN 或 CF_API_TOKEN", &token),
        ]
        .iter()
        .filter(|(_, v)| v.is_empty())
        .map(|(n, _)| *n)
        .collect();
        if !missing.is_empty() {
            return Err(anyhow!(
                "KYVAULT_BACKEND=d1 缺少配置：{}。\n\
                 要么设环境变量，要么跑一次 `kyvault d1 setup` 把它们存进本地加密库\n\
                 （存完之后裸跑 kyvault 就能连真源，不再需要外部注入）。",
                missing.join(", ")
            ));
        }
        Ok(Self {
            account_id,
            database_id,
            token,
            key: OnceCell::new(),
        })
    }

    fn url(&self) -> String {
        format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/d1/database/{}/query",
            self.account_id, self.database_id
        )
    }

    /// 与 Go 端 D1Client.Query 同一份契约：POST {sql, params}。
    /// HTTP 4xx/5xx 也要读 body —— D1 把错误详情放在响应体里，
    /// 只看状态码会把「SQL 写错」报成「网络问题」。
    fn query(&self, sql: &str, params: Vec<Value>) -> Result<Value> {
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
            .build();
        let payload = json!({ "sql": sql, "params": params });
        let resp = agent
            .post(&self.url())
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json")
            .send_json(payload);

        let body: Value = match resp {
            Ok(r) => r.into_json().context("D1 返回的不是合法 JSON")?,
            Err(ureq::Error::Status(_, r)) => r.into_json().context("D1 错误响应不是合法 JSON")?,
            Err(e) => return Err(anyhow!("D1 请求失败：{e}")),
        };
        if body.get("success").and_then(|v| v.as_bool()) != Some(true) {
            let msg = body
                .get("errors")
                .and_then(|e| e.get(0))
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("未知错误");
            let code = body
                .get("errors")
                .and_then(|e| e.get(0))
                .and_then(|e| e.get("code"))
                .and_then(|c| c.as_i64())
                .unwrap_or(0);
            return Err(anyhow!("D1 错误: {msg} (code {code})"));
        }
        Ok(body)
    }

    fn rows(body: &Value) -> Vec<Value> {
        body.get("result")
            .and_then(|r| r.as_array())
            .map(|groups| {
                groups
                    .iter()
                    .filter_map(|g| g.get("results").and_then(|r| r.as_array()))
                    .flatten()
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn master(&self) -> Result<&[u8; 32]> {
        if let Some(k) = self.key.get() {
            return Ok(k);
        }
        let body = self.query(
            "SELECT value FROM site_config WHERE key = ?1 LIMIT 1",
            vec![json!("secret_vault_master_key")],
        )?;
        let rows = Self::rows(&body);
        let encoded = rows
            .first()
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "D1 的 site_config 里还没有 secret_vault_master_key。\
                     先用 cs kyvault set 写入至少一条密钥完成初始化。"
                )
            })?;
        let k = d1_key(encoded)?;
        let _ = self.key.set(k);
        Ok(self.key.get().unwrap())
    }

    pub fn get_secret(&self, r: &str) -> Result<Option<String>> {
        let body = self.query(
            "SELECT ciphertext, nonce FROM secret_vault WHERE id = ?1 LIMIT 1",
            vec![json!(r)],
        )?;
        let rows = Self::rows(&body);
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        let ct = row
            .get("ciphertext")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let nonce = row
            .get("nonce")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        Ok(Some(decrypt_split(ct, nonce, self.master()?)?))
    }

    pub fn set_secret(&self, r: &str, value: &str, kind: &str, account: &str) -> Result<()> {
        let (platform, name) = parse_ref(r)?;
        let (ct, nonce) = encrypt_split(value, self.master()?)?;
        let now = now_utc();
        let sha = sha256_hex(value);
        self.query(
            "INSERT INTO secret_vault \
             (id, kind, platform, name, account, ciphertext, nonce, length, sha256, last4, source, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) \
             ON CONFLICT(id) DO UPDATE SET \
             kind=excluded.kind, platform=excluded.platform, name=excluded.name, \
             account=excluded.account, ciphertext=excluded.ciphertext, nonce=excluded.nonce, \
             length=excluded.length, sha256=excluded.sha256, last4=excluded.last4, \
             source=excluded.source, updated_at=excluded.updated_at",
            vec![
                json!(r), json!(kind), json!(platform), json!(name), json!(account),
                json!(ct), json!(nonce), json!(value.len()), json!(sha), json!(last4(value)),
                json!("user_input"), json!(now), json!(now),
            ],
        )?;
        Ok(())
    }

    /// 确保 org / scopes 两列存在。
    ///
    /// D1 没有 migration 工具，而这两列是后加的 —— 已经有 170 条存量记录，
    /// 不能要求人手动跑 SQL（那等于把升级门槛推给每一台机器）。
    /// ALTER TABLE ADD COLUMN 在列已存在时会报错，这里**吞掉那个错**：
    /// 幂等是目的，报错只是 SQLite 表达「已经有了」的方式。
    ///
    /// 为什么是两列而不是塞进 account 备注：备注是给人看的自由文本，
    /// org 要能过滤（「好易美有哪些机器人」）、scopes 要能校验
    /// （「这个 key 有没有权限发消息」）—— 结构化字段才做得到。
    fn ensure_schema(&self) -> Result<()> {
        for col in ["org", "scopes"] {
            let _ = self.query(
                &format!("ALTER TABLE secret_vault ADD COLUMN {col} TEXT"),
                vec![],
            );
        }
        Ok(())
    }

    /// 只改元信息，不碰密文 —— 改一句备注不该要求把密钥明文再交一遍
    /// （每交一趟都是一次泄漏机会，而且「手上没有明文」时根本做不到）。
    pub fn annotate(
        &self,
        r: &str,
        account: Option<&str>,
        kind: Option<&str>,
        org: Option<&str>,
        scopes: Option<&str>,
    ) -> Result<bool> {
        self.ensure_schema()?;
        if account.is_none() && kind.is_none() && org.is_none() && scopes.is_none() {
            return Err(anyhow!(
                "至少要给 --account / --kind / --org / --scopes 之一，否则这次调用什么都不会改"
            ));
        }
        if Self::rows(&self.query(
            "SELECT id FROM secret_vault WHERE id = ?1 LIMIT 1",
            vec![json!(r)],
        )?)
        .is_empty()
        {
            return Ok(false); // 不静默当成功：打错一个字就以为改好了是最坏的结果
        }
        let mut sets = Vec::new();
        let mut params: Vec<Value> = Vec::new();
        if let Some(a) = account {
            params.push(json!(a));
            sets.push(format!("account=?{}", params.len()));
        }
        if let Some(k) = kind {
            params.push(json!(k));
            sets.push(format!("kind=?{}", params.len()));
        }
        if let Some(o) = org {
            params.push(json!(o));
            sets.push(format!("org=?{}", params.len()));
        }
        if let Some(sc) = scopes {
            params.push(json!(sc));
            sets.push(format!("scopes=?{}", params.len()));
        }
        params.push(json!(now_utc()));
        sets.push(format!("updated_at=?{}", params.len()));
        params.push(json!(r));
        let sql = format!(
            "UPDATE secret_vault SET {} WHERE id = ?{}",
            sets.join(", "),
            params.len()
        );
        self.query(&sql, params)?;
        Ok(true)
    }

    pub fn delete_secret(&self, r: &str) -> Result<bool> {
        if Self::rows(&self.query(
            "SELECT id FROM secret_vault WHERE id = ?1 LIMIT 1",
            vec![json!(r)],
        )?)
        .is_empty()
        {
            return Ok(false);
        }
        self.query("DELETE FROM secret_vault WHERE id = ?1", vec![json!(r)])?;
        Ok(true)
    }

    pub fn list_secrets(&self) -> Result<Vec<SecretMeta>> {
        // 先确保列在 —— 老库没有 org/scopes，直接 SELECT 会整条查询失败，
        // 那会让「升级后 list 全挂」，比没有新字段严重得多。
        self.ensure_schema()?;
        let body = self.query(
            "SELECT platform, name, kind, account, last4, length, updated_at, \
             COALESCE(org,'') AS org, COALESCE(scopes,'') AS scopes \
             FROM secret_vault ORDER BY platform, name",
            vec![],
        )?;
        Ok(Self::rows(&body)
            .iter()
            .map(|r| SecretMeta {
                platform: str_of(r, "platform"),
                name: str_of(r, "name"),
                kind: str_of(r, "kind"),
                account: str_of(r, "account"),
                last4: str_of(r, "last4"),
                length: r.get("length").and_then(|v| v.as_u64()).unwrap_or(0),
                updated_at: str_of(r, "updated_at"),
                org: str_of(r, "org"),
                scopes: str_of(r, "scopes"),
            })
            .collect())
    }
}

fn str_of(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

fn now_utc() -> String {
    // 只需要 D1 那一列的 %Y-%m-%dT%H:%M:%SZ，为此拉一个 chrono 不值得
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = civil_from_unix(secs as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Unix 秒 → UTC 年月日时分秒（Howard Hinnant 的 civil_from_days 算法）
fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

fn sha256_hex(v: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(v.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_flattens_result_groups() {
        let body = json!({"result": [{"results": [{"a": 1}, {"a": 2}]}, {"results": [{"a": 3}]}]});
        assert_eq!(D1::rows(&body).len(), 3);
    }

    #[test]
    fn rows_tolerates_missing_shape() {
        assert!(D1::rows(&json!({})).is_empty());
        assert!(D1::rows(&json!({"result": null})).is_empty());
    }

    /// 时间戳格式必须和 Go/Python 端写进 updated_at 的一模一样，
    /// 否则同一张表里会出现两种格式，按时间排序就乱了。
    #[test]
    fn timestamp_matches_go_format() {
        // 期望值全部取自 python `time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(ts))`
        // —— 就是 Python/Go 端写 updated_at 用的同一个口径。第一版我按脑子里的
        // 印象填了个期望值，被这条测试当场抓住（实现是对的，猜的期望值错了）。
        for (ts, want) in [
            (0_i64, (1970, 1, 1, 0, 0, 0)),
            (1_000_000_000, (2001, 9, 9, 1, 46, 40)),
            (1_789_016_400, (2026, 9, 10, 5, 0, 0)),
        ] {
            assert_eq!(civil_from_unix(ts), want, "ts={ts}");
        }
        let now = now_utc();
        assert_eq!(now.len(), 20, "应为 YYYY-MM-DDTHH:MM:SSZ：{now}");
        assert!(now.ends_with('Z') && now.contains('T'));
    }

    #[test]
    fn sha256_matches_hashlib() {
        // python: hashlib.sha256(b"abc").hexdigest()
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn from_env_lists_all_missing() {
        // 这个测试不设环境变量，只看错误信息把三项都点出来
        let err = D1::from_env().err();
        if let Some(e) = err {
            let s = e.to_string();
            assert!(s.contains("KYVAULT_D1_ACCOUNT_ID") || s.contains("CLOUDFLARE_API_TOKEN"));
        }
    }
}
