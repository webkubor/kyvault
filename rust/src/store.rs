//! 本地 file 后端 —— 加密存 ~/.keyring/secrets.json，零网络。
//!
//! **读-改-写必须保留未知字段**，所以全程操作 serde_json::Value 而不是
//! 反序列化到强类型结构。secrets.json 是异构的：平台键（{accounts,keys}）、
//! `_servers`、`_clis`，还有更早的扁平格式 `"platform/name": {ciphertext,...}`。
//! 用强类型 struct 往返一次，就会把没建模的那些整段丢掉 —— 对密钥库来说
//! 那不是丢字段，是丢密钥。
//!
//! Store 带 root 而不是用全局常量，纯粹为了可测：测试能指到临时目录，
//! 不会去动真的 ~/.keyring。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};

use crate::crypto::{decrypt_joined, derive_file_key, encrypt_joined, new_master_key_b64};
use crate::model::{parse_ref, SecretMeta};

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 默认位置 ~/.keyring
    pub fn default_location() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("找不到 home 目录"))?;
        Ok(Self::new(home.join(".keyring")))
    }

    fn secrets_file(&self) -> PathBuf {
        self.root.join("secrets.json")
    }

    fn master_key_file(&self) -> PathBuf {
        self.root.join("master.key")
    }

    // 下面三个只读访问器给 doctor 用 —— 自检要报「文件在不在、权限对不对」，
    // 就得知道路径。刻意只暴露读，不给外部改 root 的机会。
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn master_key_path(&self) -> PathBuf {
        self.master_key_file()
    }

    pub fn secrets_path(&self) -> PathBuf {
        self.secrets_file()
    }

    /// master key：环境变量优先，其次本地文件。与 Python 版同序，
    /// 否则 CI/容器里注入的 key 会被本地文件悄悄盖掉。
    pub fn master_key(&self) -> Result<String> {
        if let Ok(k) = std::env::var("KEYRING_MASTER_KEY") {
            if !k.trim().is_empty() {
                return Ok(k.trim().to_string());
            }
        }
        let f = self.master_key_file();
        if f.exists() {
            return Ok(fs::read_to_string(&f)?.trim().to_string());
        }
        Err(anyhow!("未初始化。运行 kyvault init 生成 master key。"))
    }

    /// 已存在就原样返回 —— 绝不覆盖：覆盖 master key 等于让全部存量密文永久解不开。
    pub fn init_master_key(&self) -> Result<String> {
        let f = self.master_key_file();
        if f.exists() {
            return Ok(fs::read_to_string(&f)?.trim().to_string());
        }
        fs::create_dir_all(&self.root)?;
        let key = new_master_key_b64();
        fs::write(&f, &key)?;
        fs::set_permissions(&f, fs::Permissions::from_mode(0o600))?;
        Ok(key)
    }

    fn aes_key(&self) -> Result<[u8; 32]> {
        derive_file_key(&self.master_key()?)
    }

    fn load(&self) -> Value {
        // 读不出来就当空库：这里绝不能 panic，否则一个手改坏的 json
        // 会让 kyvault 整个用不了（包括本来能救场的 init）。
        fs::read_to_string(self.secrets_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!({}))
    }

    fn save(&self, data: &Value) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        let path = self.secrets_file();
        // 先写临时文件再 rename：写一半被打断也不会留下半个 json 把库废掉
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(data)?)
            .with_context(|| format!("写 {} 失败", tmp.display()))?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn obj_mut<'a>(root: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
        root.as_object_mut()
            .expect("顶层必须是对象")
            .entry(key.to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("该键必须是对象")
    }

    // ── secret:// 寻址 ─────────────────────────────────────

    pub fn get_secret(&self, r: &str) -> Result<Option<String>> {
        let (platform, name) = parse_ref(r)?;
        let key = self.aes_key()?;
        let data = self.load();

        if platform == "server" {
            let mut it = name.splitn(2, '/');
            let host = it.next().unwrap_or_default();
            return match it.next() {
                Some(field) => self.decrypt_at(&data, &["_servers", host, field], &key),
                None => {
                    // 不带字段时返回整台机器的 json，与 Python 版一致
                    let Some(fields) = data
                        .pointer(&format!("/_servers/{host}"))
                        .and_then(|v| v.as_object())
                    else {
                        return Ok(None);
                    };
                    let mut out = Map::new();
                    for (k, v) in fields {
                        out.insert(
                            k.clone(),
                            Value::String(decrypt_joined(v.as_str().unwrap_or_default(), &key)?),
                        );
                    }
                    Ok(Some(serde_json::to_string(&out)?))
                }
            };
        }
        if platform == "cli" {
            if let Some((cli, profile)) = name.split_once('/') {
                return self.decrypt_at(&data, &["_clis", cli, profile], &key);
            }
        }
        if !platform.starts_with('_') {
            for bucket in ["keys", "accounts"] {
                if let Some(v) = self.decrypt_at(&data, &[&platform, bucket, &name], &key)? {
                    return Ok(Some(v));
                }
            }
        }
        // 旧扁平格式：{"platform/name": {"ciphertext": ...}}
        if let Some(ct) = data
            .get(format!("{platform}/{name}"))
            .and_then(|v| v.get("ciphertext"))
            .and_then(|v| v.as_str())
        {
            return Ok(Some(decrypt_joined(ct, &key)?));
        }
        Ok(None)
    }

    fn decrypt_at(&self, data: &Value, path: &[&str], key: &[u8; 32]) -> Result<Option<String>> {
        let mut cur = data;
        for p in path {
            match cur.get(*p) {
                Some(v) => cur = v,
                None => return Ok(None),
            }
        }
        match cur.as_str() {
            Some(ct) => Ok(Some(decrypt_joined(ct, key)?)),
            None => Ok(None),
        }
    }

    pub fn set_secret(&self, r: &str, value: &str) -> Result<()> {
        let (platform, name) = parse_ref(r)?;
        let key = self.aes_key()?;
        let ct = encrypt_joined(value, &key)?;
        let mut data = self.load();

        if platform == "cli" {
            if let Some((cli, profile)) = name.split_once('/') {
                let clis = Self::obj_mut(&mut data, "_clis");
                clis.entry(cli.to_string())
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .unwrap()
                    .insert(profile.to_string(), Value::String(ct));
                return self.save(&data);
            }
        }
        if platform == "server" {
            if let Some((host, field)) = name.split_once('/') {
                let servers = Self::obj_mut(&mut data, "_servers");
                servers
                    .entry(host.to_string())
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .unwrap()
                    .insert(field.to_string(), Value::String(ct));
                return self.save(&data);
            }
        }
        let p = Self::obj_mut(&mut data, &platform);
        p.entry("accounts".to_string()).or_insert_with(|| json!({}));
        p.entry("keys".to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(name, Value::String(ct));
        self.save(&data)
    }

    pub fn delete_secret(&self, r: &str) -> Result<bool> {
        let (platform, name) = parse_ref(r)?;
        let mut data = self.load();
        let removed = {
            let root = data.as_object_mut().ok_or_else(|| anyhow!("库结构损坏"))?;
            let mut hit = false;
            if platform == "cli" {
                if let Some((cli, profile)) = name.split_once('/') {
                    if let Some(m) = root
                        .get_mut("_clis")
                        .and_then(|v| v.get_mut(cli))
                        .and_then(|v| v.as_object_mut())
                    {
                        hit = m.remove(profile).is_some();
                    }
                }
            } else if platform == "server" {
                let host = name.split('/').next().unwrap_or_default();
                if let Some(m) = root.get_mut("_servers").and_then(|v| v.as_object_mut()) {
                    hit = m.remove(host).is_some();
                }
            } else {
                if let Some(m) = root
                    .get_mut(&platform)
                    .and_then(|v| v.get_mut("keys"))
                    .and_then(|v| v.as_object_mut())
                {
                    hit = m.remove(&name).is_some();
                }
                if !hit {
                    hit = root.remove(&format!("{platform}/{name}")).is_some();
                }
            }
            hit
        };
        if removed {
            self.save(&data)?;
        }
        Ok(removed)
    }

    pub fn list_secrets(&self) -> Vec<SecretMeta> {
        let data = self.load();
        let mut out = Vec::new();
        let Some(root) = data.as_object() else {
            return out;
        };

        if let Some(servers) = root.get("_servers").and_then(|v| v.as_object()) {
            for (host, fields) in servers {
                for field in fields
                    .as_object()
                    .map(|m| m.keys().collect::<Vec<_>>())
                    .unwrap_or_default()
                {
                    out.push(SecretMeta {
                        platform: "server".into(),
                        name: format!("{host}/{field}"),
                        kind: "Server Field".into(),
                        ..Default::default()
                    });
                }
            }
        }
        if let Some(clis) = root.get("_clis").and_then(|v| v.as_object()) {
            for (cli, profiles) in clis {
                for profile in profiles
                    .as_object()
                    .map(|m| m.keys().collect::<Vec<_>>())
                    .unwrap_or_default()
                {
                    out.push(SecretMeta {
                        platform: "cli".into(),
                        name: format!("{cli}/{profile}"),
                        kind: "CLI Token".into(),
                        account: profile.clone(),
                        ..Default::default()
                    });
                }
            }
        }
        for (platform, v) in root {
            if platform.starts_with('_') {
                continue;
            }
            if let Some(keys) = v.get("keys").and_then(|k| k.as_object()) {
                for name in keys.keys() {
                    out.push(SecretMeta {
                        platform: platform.clone(),
                        name: name.clone(),
                        kind: "Key".into(),
                        ..Default::default()
                    });
                }
            }
            if let Some(accounts) = v.get("accounts").and_then(|k| k.as_object()) {
                for user in accounts.keys() {
                    out.push(SecretMeta {
                        platform: platform.clone(),
                        name: user.clone(),
                        kind: "Account".into(),
                        account: user.clone(),
                        ..Default::default()
                    });
                }
            }
            // 旧扁平格式
            if v.get("keys").is_none() && v.get("accounts").is_none() {
                if let Some(ct) = v.as_object() {
                    if ct.contains_key("ciphertext") {
                        if let Some((p, n)) = platform.split_once('/') {
                            out.push(SecretMeta {
                                platform: p.into(),
                                name: n.into(),
                                kind: ct.get("kind").and_then(|k| k.as_str()).unwrap_or("").into(),
                                account: ct
                                    .get("account")
                                    .and_then(|k| k.as_str())
                                    .unwrap_or("")
                                    .into(),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| (&a.platform, &a.name).cmp(&(&b.platform, &b.name)));
        out
    }

    // ── account / key / platform 命名空间 ─────────────────
    //
    // 这三组是**只有本地 file 后端**的能力（Python 版同样如此）：D1 那张
    // secret_vault 表是扁平的 id→密文，没有 accounts 这个桶。所以 CLI 层
    // 直接用 Store，不走后端分发 —— 假装支持再静默落到别处，比明确只支持本地糟。

    pub fn set_account(&self, platform: &str, user: &str, password: &str) -> Result<()> {
        let ct = encrypt_joined(password, &self.aes_key()?)?;
        let mut data = self.load();
        let p = Self::obj_mut(&mut data, platform);
        p.entry("keys".to_string()).or_insert_with(|| json!({}));
        p.entry("accounts".to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(user.to_string(), Value::String(ct));
        self.save(&data)
    }

    pub fn get_account(&self, platform: &str, user: &str) -> Result<Option<String>> {
        let key = self.aes_key()?;
        self.decrypt_at(&self.load(), &[platform, "accounts", user], &key)
    }

    // ── _servers / _clis 命名空间 ─────────────────────────
    //
    // 这两组同样只有 file 后端有（与 account/key 同理）。它们是**顶层特殊 key**
    // （`_servers` / `_clis`），不是某个 platform 下的桶 —— 结构必须和 Python 版
    // 逐字一致，否则老库读不出来：
    //   _servers[hostname] = { ip, root-password, cost?, provider? }   每个值单独加密
    //   _clis[cli_name][profile] = <密文>
    // 注意 `root-password` 带连字符，不是下划线（Python 版就这么写的，别顺手改）。

    pub fn set_server(
        &self,
        hostname: &str,
        ip: &str,
        root_password: &str,
        cost: &str,
        provider: &str,
    ) -> Result<()> {
        let key = self.aes_key()?;
        let mut entry = Map::new();
        entry.insert("ip".into(), Value::String(encrypt_joined(ip, &key)?));
        entry.insert(
            "root-password".into(),
            Value::String(encrypt_joined(root_password, &key)?),
        );
        // 空字段不写：写了空密文，get 时会解出空串，看着像"存过但丢了"
        if !cost.is_empty() {
            entry.insert("cost".into(), Value::String(encrypt_joined(cost, &key)?));
        }
        if !provider.is_empty() {
            entry.insert(
                "provider".into(),
                Value::String(encrypt_joined(provider, &key)?),
            );
        }
        let mut data = self.load();
        Self::obj_mut(&mut data, "_servers").insert(hostname.to_string(), Value::Object(entry));
        self.save(&data)
    }

    /// field 为 None 时返回全部字段（已解密）；指定 field 只返回那一项。
    pub fn get_server(
        &self,
        hostname: &str,
        field: Option<&str>,
    ) -> Result<Option<Vec<(String, String)>>> {
        let key = self.aes_key()?;
        let data = self.load();
        let Some(entry) = data.get("_servers").and_then(|v| v.get(hostname)) else {
            return Ok(None);
        };
        let Some(obj) = entry.as_object() else {
            return Ok(None);
        };
        let mut out = Vec::new();
        for (k, v) in obj {
            if let Some(f) = field {
                if k != f {
                    continue;
                }
            }
            if let Some(ct) = v.as_str() {
                out.push((k.clone(), decrypt_joined(ct, &key)?));
            }
        }
        if out.is_empty() {
            return Ok(None);
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Some(out))
    }

    pub fn list_servers(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .load()
            .get("_servers")
            .and_then(|s| s.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    pub fn delete_server(&self, hostname: &str) -> Result<bool> {
        let mut data = self.load();
        let removed = Self::obj_mut(&mut data, "_servers")
            .remove(hostname)
            .is_some();
        if removed {
            self.save(&data)?;
        }
        Ok(removed)
    }

    pub fn set_cli_token(&self, cli_name: &str, profile: &str, token: &str) -> Result<()> {
        let ct = encrypt_joined(token, &self.aes_key()?)?;
        let mut data = self.load();
        Self::obj_mut(&mut data, "_clis")
            .entry(cli_name.to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(profile.to_string(), Value::String(ct));
        self.save(&data)
    }

    pub fn get_cli_token(&self, cli_name: &str, profile: &str) -> Result<Option<String>> {
        let key = self.aes_key()?;
        self.decrypt_at(&self.load(), &["_clis", cli_name, profile], &key)
    }

    /// cli_name 为 None 时列出所有 CLI 名；给了就列它的 profile。
    pub fn list_clis(&self, cli_name: Option<&str>) -> Vec<String> {
        let data = self.load();
        let Some(clis) = data.get("_clis").and_then(|v| v.as_object()) else {
            return vec![];
        };
        let mut v: Vec<String> = match cli_name {
            None => clis.keys().cloned().collect(),
            Some(n) => clis
                .get(n)
                .and_then(|v| v.as_object())
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default(),
        };
        v.sort();
        v
    }

    pub fn delete_cli_token(&self, cli_name: &str, profile: &str) -> Result<bool> {
        let mut data = self.load();
        let removed = Self::obj_mut(&mut data, "_clis")
            .get_mut(cli_name)
            .and_then(|v| v.as_object_mut())
            .and_then(|o| o.remove(profile))
            .is_some();
        if removed {
            self.save(&data)?;
        }
        Ok(removed)
    }

    pub fn set_key(&self, platform: &str, name: &str, value: &str) -> Result<()> {
        let ct = encrypt_joined(value, &self.aes_key()?)?;
        let mut data = self.load();
        let p = Self::obj_mut(&mut data, platform);
        p.entry("accounts".to_string()).or_insert_with(|| json!({}));
        p.entry("keys".to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(name.to_string(), Value::String(ct));
        self.save(&data)
    }

    pub fn get_key(&self, platform: &str, name: &str) -> Result<Option<String>> {
        let key = self.aes_key()?;
        self.decrypt_at(&self.load(), &[platform, "keys", name], &key)
    }

    /// bucket 传 "accounts" 或 "keys"
    pub fn list_bucket(&self, platform: &str, bucket: &str) -> Vec<String> {
        self.load()
            .get(platform)
            .and_then(|v| v.get(bucket))
            .and_then(|v| v.as_object())
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn delete_from_bucket(&self, platform: &str, bucket: &str, name: &str) -> Result<bool> {
        let mut data = self.load();
        let hit = data
            .get_mut(platform)
            .and_then(|v| v.get_mut(bucket))
            .and_then(|v| v.as_object_mut())
            .map(|m| m.remove(name).is_some())
            .unwrap_or(false);
        if hit {
            self.save(&data)?;
        }
        Ok(hit)
    }

    /// 平台 → (accounts, keys)。跳过 _servers / _clis 这些系统保留键。
    pub fn platforms(&self) -> std::collections::BTreeMap<String, (Vec<String>, Vec<String>)> {
        let data = self.load();
        let mut out = std::collections::BTreeMap::new();
        if let Some(root) = data.as_object() {
            for (platform, v) in root {
                if platform.starts_with('_') {
                    continue;
                }
                let take = |b: &str| {
                    v.get(b)
                        .and_then(|x| x.as_object())
                        .map(|m| m.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default()
                };
                out.insert(platform.clone(), (take("accounts"), take("keys")));
            }
        }
        out
    }

    pub fn exists(&self) -> bool {
        Path::new(&self.secrets_file()).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path());
        s.init_master_key().unwrap();
        (d, s)
    }

    #[test]
    fn secret_roundtrip_and_delete() {
        let (_d, s) = store();
        s.set_secret("secret://github/token", "ghp_x").unwrap();
        assert_eq!(
            s.get_secret("secret://github/token").unwrap().unwrap(),
            "ghp_x"
        );
        assert!(s.delete_secret("secret://github/token").unwrap());
        assert!(s.get_secret("secret://github/token").unwrap().is_none());
        assert!(
            !s.delete_secret("secret://github/token").unwrap(),
            "删不存在的要返回 false"
        );
    }

    /// cli/server 走各自的命名空间，别落进普通平台的 keys 里 ——
    /// CLAUDE.md 里 secret://cli/<cli>/<profile> 这种三段寻址依赖它
    #[test]
    fn cli_and_server_namespaces() {
        let (_d, s) = store();
        s.set_secret("secret://cli/gh/main", "gho_1").unwrap();
        s.set_secret("secret://server/vex/root-password", "pw")
            .unwrap();
        assert_eq!(
            s.get_secret("secret://cli/gh/main").unwrap().unwrap(),
            "gho_1"
        );
        assert_eq!(
            s.get_secret("secret://server/vex/root-password")
                .unwrap()
                .unwrap(),
            "pw"
        );
        // 不带字段时返回整台机器的 json
        let all = s.get_secret("secret://server/vex").unwrap().unwrap();
        assert!(all.contains("root-password"), "应返回整机 json：{all}");
    }

    /// 读-改-写必须保住没建模的字段。对密钥库来说丢字段就是丢密钥。
    #[test]
    fn unknown_fields_survive_write() {
        let (d, s) = store();
        let f = d.path().join("secrets.json");
        fs::write(
            &f,
            r#"{"legacy/flat":{"ciphertext":"x","kind":"K"},"future_thing":{"a":1}}"#,
        )
        .unwrap();
        s.set_secret("secret://github/token", "v").unwrap();
        let after: Value = serde_json::from_str(&fs::read_to_string(&f).unwrap()).unwrap();
        assert!(after.get("future_thing").is_some(), "未知字段被写掉了");
        assert!(after.get("legacy/flat").is_some(), "旧扁平格式被写掉了");
    }

    #[test]
    fn old_flat_format_readable() {
        let (d, s) = store();
        let key = s.aes_key().unwrap();
        let ct = encrypt_joined("old-value", &key).unwrap();
        fs::write(
            d.path().join("secrets.json"),
            serde_json::to_string(&json!({ "legacy/thing": { "ciphertext": ct } })).unwrap(),
        )
        .unwrap();
        assert_eq!(
            s.get_secret("secret://legacy/thing").unwrap().unwrap(),
            "old-value"
        );
    }

    #[test]
    fn account_and_key_buckets() {
        let (_d, s) = store();
        s.set_account("github", "webkubor", "pw1").unwrap();
        s.set_key("github", "pat", "ghp_2").unwrap();
        assert_eq!(s.get_account("github", "webkubor").unwrap().unwrap(), "pw1");
        assert_eq!(s.get_key("github", "pat").unwrap().unwrap(), "ghp_2");
        assert_eq!(s.list_bucket("github", "accounts"), vec!["webkubor"]);
        assert_eq!(s.list_bucket("github", "keys"), vec!["pat"]);
        let p = s.platforms();
        assert_eq!(p["github"].0, vec!["webkubor"]);
        assert!(s.delete_from_bucket("github", "keys", "pat").unwrap());
        assert!(s.list_bucket("github", "keys").is_empty());
    }

    #[test]
    fn master_key_never_overwritten() {
        let (_d, s) = store();
        let first = s.master_key().unwrap();
        let again = s.init_master_key().unwrap();
        assert_eq!(
            first, again,
            "init 第二次绝不能换 key —— 换了存量密文全部解不开"
        );
    }

    #[test]
    fn broken_json_does_not_panic() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path());
        s.init_master_key().unwrap();
        fs::write(d.path().join("secrets.json"), "{不是 json").unwrap();
        assert!(s.list_secrets().is_empty(), "坏库当空库，不能 panic");
        // 还能继续写（新内容覆盖坏文件）
        s.set_secret("secret://a/b", "v").unwrap();
        assert_eq!(s.get_secret("secret://a/b").unwrap().unwrap(), "v");
    }
}
