//! 操作审计 —— 「谁、什么时候、动过哪条」。
//!
//! 此前只有 `meta.json` 里的 `updated_at`：时间有，人不明。而 `get` 是**唯一
//! 输出明文的命令**，读行为却完全不留痕 —— 一旦密钥泄露，连「有没有人读过」
//! 都答不上来。
//!
//! ## 记什么，不记什么
//!
//! 记：时间、actor、动作、目标 ref、结果、备注（后端名 / 来源）。
//! **绝不记明文，也不记密文，连 last4 都不记** —— 审计日志本身如果成了泄漏面，
//! 就把加这层的目的抵消了。
//!
//! ## 为什么默认只写本地、不入仓
//!
//! `audit.log` 里有读事件。把它 push 到团队仓等于「谁读了自己的密钥」对全组
//! 可见 —— 那是监视，不是审计。团队维度的「谁改了」由 Git 提交承载（见
//! `vaultrepo::push_initial` 与 `gitlab::push` 的提交信息），两者互补：
//! 本地日志回答「谁读过」，Git 历史回答「谁改过」。
//!
//! `KY_AUDIT=0` 可关（批处理/压测时），`KY_AUDIT_SYNC=1` 可让它入仓。
//!
//! ## 失败不阻断
//!
//! 审计是**旁路**：磁盘满了、权限错了，都不该让 `ky get` 取不出密钥。写入
//! 失败只在 stderr 提示一次，然后照常返回。

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;
use serde_json::{json, Value};

use crate::model::now_utc;
use crate::store::{harden, Store};

/// 日志文件名。放库根目录，跟 `secrets.json` 同级，权限 0600。
pub const FILE: &str = "audit.log";

pub fn path(store: &Store) -> PathBuf {
    store.root().join(FILE)
}

/// env 关掉总开关。默认开 —— 审计要么默认开，要么等于没有。
pub fn enabled() -> bool {
    !matches!(
        std::env::var("KY_AUDIT").unwrap_or_default().trim(),
        "0" | "off" | "false" | "no"
    )
}

/// actor 的解析顺序：显式参数 → `KY_ACTOR` → 库里 git 的 user.name → `$USER`。
///
/// 为什么 git 在 `$USER` 前面：团队库的 Git 历史用同一个身份提交，两处对得上
/// 才叫「同一个人」。反过来 `$USER` 在多账号机器上只是 shell 登录名。
pub fn actor(explicit: Option<&str>) -> String {
    if let Some(a) = explicit.map(str::trim).filter(|a| !a.is_empty()) {
        return a.to_string();
    }
    if let Ok(a) = std::env::var("KY_ACTOR") {
        let a = a.trim();
        if !a.is_empty() {
            return a.to_string();
        }
    }
    let mut cmd = Command::new("git");
    cmd.args(["config", "--get", "user.name"]);
    // 优先在库目录里问：团队库的 local config 才是这个库的身份
    if let Some(root) = std::env::var_os("KYVAULT_STORE_DIR") {
        cmd.current_dir(root);
    }
    if let Ok(out) = cmd.output() {
        if out.status.success() {
            let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !name.is_empty() {
                return name;
            }
        }
    }
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

/// 一条事件。字段名刻意短：日志是给人 `ky audit` 看的，不是给 ELK 灌的。
pub struct Event<'a> {
    pub action: &'a str,
    pub target: &'a str,
    pub result: &'a str,
    pub note: &'a str,
}

/// 追加一条。**永不返回 Err** —— 审计失败不该打断正在做的事。
pub fn record(store: &Store, actor: &str, ev: Event<'_>) {
    if !enabled() {
        return;
    }
    let line = json!({
        "ts": now_utc(),
        "actor": actor,
        "action": ev.action,
        "target": ev.target,
        "result": ev.result,
        "note": ev.note,
    });
    if let Err(e) = append(store, &line.to_string()) {
        eprintln!("⚠️  审计日志写入失败（不影响本次操作）：{e}");
    }
}

fn append(store: &Store, line: &str) -> Result<()> {
    std::fs::create_dir_all(store.root())?;
    let p = path(store);
    let fresh = !p.exists();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)?;
    if fresh {
        harden(&p)?;
    }
    writeln!(f, "{line}")?;
    Ok(())
}

/// 读最近 `limit` 条（新的在前），可按 action / actor 过滤。
pub fn read(
    store: &Store,
    limit: usize,
    action: Option<&str>,
    actor: Option<&str>,
) -> Result<Vec<Value>> {
    let p = path(store);
    if !p.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&p)?;
    let mut out: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| action.map_or(true, |a| v["action"] == a))
        .filter(|v| actor.map_or(true, |a| v["actor"] == a))
        .collect();
    if out.len() > limit {
        out = out.split_off(out.len() - limit);
    }
    out.reverse();
    Ok(out)
}

/// 统计用：按 action 数条目（panel 用，不读全文两遍）。
pub fn counts(store: &Store) -> Result<std::collections::BTreeMap<String, usize>> {
    let mut m = std::collections::BTreeMap::new();
    let p = path(store);
    if !p.exists() {
        return Ok(m);
    }
    for line in std::fs::read_to_string(&p)?.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if let Some(a) = v["action"].as_str() {
                *m.entry(a.to_string()).or_insert(0) += 1;
            }
        }
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_prefers_explicit_then_env() {
        // 显式参数最高优先，且不受 env 影响
        assert_eq!(actor(Some("alice")), "alice");
        // 空字符串等于没给
        assert!(!actor(Some("   ")).is_empty());
    }

    #[test]
    fn append_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let st = Store::new(dir.path());
        record(
            &st,
            "alice",
            Event {
                action: "get",
                target: "secret://github/pat",
                result: "ok",
                note: "file",
            },
        );
        record(
            &st,
            "bob",
            Event {
                action: "set",
                target: "secret://gitlab/token",
                result: "ok",
                note: "file",
            },
        );
        let all = read(&st, 10, None, None).unwrap();
        assert_eq!(all.len(), 2);
        // 新的在前
        assert_eq!(all[0]["action"], "set");
        let only_get = read(&st, 10, Some("get"), None).unwrap();
        assert_eq!(only_get.len(), 1);
        assert_eq!(only_get[0]["actor"], "alice");
        let only_bob = read(&st, 10, None, Some("bob")).unwrap();
        assert_eq!(only_bob.len(), 1);
        // 日志里不能出现明文
        let raw = std::fs::read_to_string(path(&st)).unwrap();
        assert!(!raw.contains("glpat-"));
    }
}
