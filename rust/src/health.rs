//! 本地最新检查结果：只记录引用和状态，不保存密钥或上游响应。

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::now_utc;
use crate::providers::CheckResult;
use crate::store::{harden, Store};

pub const FILE: &str = "health.json";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    #[serde(rename = "ref")]
    reference: String,
    provider: String,
    status: String,
    http_status: Option<u16>,
    checked_at: String,
    elapsed_ms: u128,
}

fn load(store: &Store) -> Result<BTreeMap<String, Entry>> {
    let content = match fs::read_to_string(store.root().join(FILE)) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error).context("无法读取 health.json"),
    };
    let entries: BTreeMap<String, Entry> = serde_json::from_str(&content)
        .map_err(|_| anyhow!("health.json 损坏或结构无效，拒绝读取和覆盖"))?;
    if entries.iter().any(|(reference, entry)| {
        reference != &entry.reference
            || !matches!(
                entry.status.as_str(),
                "valid"
                    | "invalid"
                    | "permission_limited"
                    | "rate_limited"
                    | "unknown"
                    | "unsupported"
                    | "api_error"
            )
    }) {
        return Err(anyhow!("health.json 记录结构无效，拒绝读取和覆盖"));
    }
    Ok(entries)
}

/// 同一引用只保留最新检查；锁覆盖读改写，临时文件在写入前收紧权限。
/// 调用方只传引用，不传密钥；message/models/balance 不进入磁盘记录。
pub fn record(
    store: &Store,
    reference: &str,
    provider: &str,
    result: &CheckResult,
    elapsed_ms: u128,
) -> Result<()> {
    let _lock = store.lock()?;
    let mut entries = load(store)?;
    entries.insert(
        reference.to_string(),
        Entry {
            reference: reference.to_string(),
            provider: provider.to_string(),
            status: result.status.to_string(),
            http_status: result.http_status,
            checked_at: now_utc(),
            elapsed_ms,
        },
    );
    let content = serde_json::to_vec_pretty(&entries)?;
    let tmp = store.root().join("health.json.tmp");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&tmp)
        .context("无法创建 health.json 临时文件")?;
    let saved = (|| -> Result<()> {
        harden(&tmp)?;
        file.write_all(&content)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, store.root().join(FILE))?;
        Ok(())
    })();
    if saved.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    saved.context("无法保存 health.json")
}

/// 返回按引用索引的结构化对象。仅文件不存在时为空；损坏时明确报错。
pub fn read(store: &Store) -> Result<Value> {
    Ok(serde_json::to_value(load(store)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn result(status: &'static str, http_status: Option<u16>) -> CheckResult {
        CheckResult {
            valid: matches!(status, "valid" | "rate_limited"),
            status,
            http_status,
            message: "测试私密值不得从错误文案进入健康记录".to_string(),
            models: vec!["测试私密模型".to_string()],
            balance: Some("测试私密余额".to_string()),
        }
    }

    #[test]
    fn record_keeps_only_health_fields_and_preserves_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        fs::write(dir.path().join("secrets.json"), "测试密文").unwrap();
        fs::write(dir.path().join("meta.json"), "测试元数据").unwrap();
        record(
            &store,
            "secret://openai/default",
            "openai",
            &result("rate_limited", Some(429)),
            42,
        )
        .unwrap();
        let data = read(&store).unwrap();
        let entry = &data["secret://openai/default"];
        assert_eq!(entry["ref"], "secret://openai/default");
        assert_eq!(entry["provider"], "openai");
        assert_eq!(entry["status"], "rate_limited");
        assert_eq!(entry["http_status"], 429);
        assert_eq!(entry["elapsed_ms"], 42);
        assert!(entry["checked_at"].as_str().unwrap().ends_with('Z'));
        assert_eq!(entry.as_object().unwrap().len(), 6);
        let raw = fs::read_to_string(dir.path().join(FILE)).unwrap();
        for forbidden in [
            "message",
            "models",
            "balance",
            "last4",
            "hash",
            "value",
            "测试私密",
        ] {
            assert!(!raw.contains(forbidden), "健康记录包含禁止字段或内容");
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("secrets.json")).unwrap(),
            "测试密文"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("meta.json")).unwrap(),
            "测试元数据"
        );
        assert!(!dir.path().join("master.key").exists());
        assert!(!dir.path().join("health.json.tmp").exists());
    }

    #[test]
    fn updates_latest_result_and_preserves_other_references() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        assert_eq!(read(&store).unwrap(), json!({}));
        record(
            &store,
            "secret://openai/first",
            "openai",
            &result("valid", Some(200)),
            1,
        )
        .unwrap();
        record(
            &store,
            "secret://openai/second",
            "openai",
            &result("unknown", None),
            2,
        )
        .unwrap();
        record(
            &store,
            "secret://openai/first",
            "openai",
            &result("invalid", Some(401)),
            3,
        )
        .unwrap();
        let data = read(&store).unwrap();
        assert_eq!(data.as_object().unwrap().len(), 2);
        assert_eq!(data["secret://openai/first"]["status"], "invalid");
        assert_eq!(data["secret://openai/first"]["elapsed_ms"], 3);
        assert_eq!(data["secret://openai/second"]["status"], "unknown");
        assert!(data["secret://openai/second"]["http_status"].is_null());
    }

    #[test]
    fn concurrent_records_preserve_every_reference() {
        let dir = tempfile::tempdir().unwrap();
        let handles: Vec<_> = (0..4)
            .map(|index| {
                let root = dir.path().to_path_buf();
                std::thread::spawn(move || {
                    record(
                        &Store::new(root),
                        &format!("secret://openai/test-{index}"),
                        "openai",
                        &result("valid", Some(200)),
                        index,
                    )
                    .unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let data = read(&Store::new(dir.path())).unwrap();
        assert_eq!(data.as_object().unwrap().len(), 4);
        for index in 0..4 {
            assert_eq!(
                data[format!("secret://openai/test-{index}")]["status"],
                "valid"
            );
        }
    }

    #[test]
    fn damaged_json_and_invalid_structure_block_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let path = dir.path().join(FILE);
        for content in [
            "{损坏",
            "[]",
            "null",
            r#"{"secret://openai/default": {}}"#,
            r#"{"secret://openai/default":{"ref":"secret://openai/default","provider":"openai","status":"valid","http_status":200,"checked_at":"now","elapsed_ms":1,"value":"测试私密值"}}"#,
        ] {
            fs::write(&path, content).unwrap();
            let error = read(&store).unwrap_err().to_string();
            assert!(error.contains("health.json"));
            assert!(!error.contains("测试私密值"));
            assert!(record(
                &store,
                "secret://openai/default",
                "openai",
                &result("valid", Some(200)),
                1,
            )
            .is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
            assert!(!dir.path().join("health.json.tmp").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn health_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        record(
            &store,
            "secret://openai/default",
            "openai",
            &result("valid", Some(200)),
            1,
        )
        .unwrap();
        let path = dir.path().join(FILE);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        record(
            &store,
            "secret://openai/default",
            "openai",
            &result("unknown", None),
            2,
        )
        .unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
