//! 离线只读监察：元数据默认不解密，重复值检查须显式开启。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use anyhow::{anyhow, Context, Result};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde_json::{json, Map, Value};
use sha2::Sha256;

use crate::audit;
use crate::model::{now_utc, parse_ref, SecretMeta};
use crate::registry::Registry;
use crate::store::Store;

struct Entry {
    meta: SecretMeta,
    storage: &'static str,
}

/// 生成本地报告。不会联网、写审计、创建锁文件或修改库文件。
/// `duplicates = false` 时不读取 master key，也不调用任何解密 API。
pub fn report(store: &Store, duplicates: bool) -> Result<Value> {
    let data = store.load()?;
    validate_store(&data)?;
    let metadata = read_metadata(store)?;
    validate_registry(store)?;
    let registry = Registry::at(store.root())?;
    let entries = inventory(store, &data, &metadata)?;
    let health = crate::health::read(store)?;
    let health_entries = health
        .as_object()
        .ok_or_else(|| anyhow!("health.json 结构错误"))?;
    let generated_at = now_utc();
    let now = timestamp(&generated_at).ok_or_else(|| anyhow!("系统时间无法解析"))?;
    let mut timestamp_issues = Vec::new();
    let mut by_platform = BTreeMap::<String, usize>::new();
    let mut by_kind = BTreeMap::<String, usize>::new();
    let mut by_account = BTreeMap::<String, usize>::new();
    let mut locations = BTreeMap::<String, Vec<&str>>::new();

    for entry in &entries {
        *by_platform.entry(entry.meta.platform.clone()).or_default() += 1;
        *by_kind.entry(entry.meta.kind.clone()).or_default() += 1;
        *by_account.entry(entry.meta.account.clone()).or_default() += 1;
        locations
            .entry(entry.meta.r#ref())
            .or_default()
            .push(entry.storage);
    }
    let existing: BTreeSet<String> = locations.keys().cloned().collect();
    let mut associations = BTreeMap::<String, BTreeMap<String, usize>>::new();
    let mut orphan_refs = Vec::new();
    let mut profiles = Vec::new();
    for profile in registry.list() {
        let mut linked = BTreeSet::new();
        for reference in &profile.refs {
            *associations
                .entry(reference.clone())
                .or_default()
                .entry(profile.name.clone())
                .or_default() += 1;
            let reason = if parse_ref(reference).is_err() {
                Some("invalid_ref")
            } else if !reference_exists(&data, reference) {
                Some("missing_secret")
            } else {
                linked.insert(reference.clone());
                if let Ok((platform, name)) = parse_ref(reference) {
                    if platform == "server" && !name.contains('/') {
                        for field in existing
                            .iter()
                            .filter(|field| field.starts_with(&format!("{reference}/")))
                        {
                            associations
                                .entry(field.clone())
                                .or_default()
                                .entry(profile.name.clone())
                                .or_insert(1);
                        }
                    }
                }
                None
            };
            if let Some(reason) = reason {
                orphan_refs.push(json!({
                    "profile": profile.name, "ref": reference, "reason": reason
                }));
            }
        }
        check_time(
            &mut timestamp_issues,
            &format!("profile:{}", profile.name),
            "created_at",
            Some(&Value::String(profile.created_at.clone())),
            now,
            true,
            true,
        );
        profiles.push(json!({
            "name": profile.name,
            "platform": profile.platform,
            "username": profile.username,
            "org": profile.org,
            "refs": profile.refs,
            "existing_refs": linked.len(),
        }));
    }
    let shared_refs: Vec<Value> = associations
        .iter()
        .filter(|(_, profiles)| profiles.values().sum::<usize>() > 1)
        .map(|(reference, profiles)| {
            json!({"ref": reference, "profiles": profiles, "associations": profiles.values().sum::<usize>()})
        })
        .collect();
    let unlinked_refs: Vec<&String> = existing
        .iter()
        .filter(|reference| !associations.contains_key(*reference))
        .collect();
    let ambiguous_refs: Vec<Value> = locations
        .iter()
        .filter(|(_, sources)| sources.len() > 1)
        .map(|(reference, sources)| json!({"ref": reference, "storage": sources}))
        .collect();

    let (audit_report, last_used) = audit_observations(store, now, &mut timestamp_issues)?;
    let mut items = Vec::new();
    for entry in &entries {
        let reference = entry.meta.r#ref();
        let key = format!("{}/{}", entry.meta.platform, entry.meta.name);
        let meta = metadata.get(&key).and_then(Value::as_object);
        let created = meta.and_then(|m| m.get("created_at"));
        let updated = meta.and_then(|m| m.get("updated_at"));
        let expires = meta.and_then(|m| m.get("expires_at"));
        let created_ts = check_time(
            &mut timestamp_issues,
            &reference,
            "created_at",
            created,
            now,
            true,
            true,
        );
        let updated_ts = check_time(
            &mut timestamp_issues,
            &reference,
            "updated_at",
            updated,
            now,
            true,
            true,
        );
        let expires_ts = check_time(
            &mut timestamp_issues,
            &reference,
            "expires_at",
            expires,
            now,
            false,
            false,
        );
        if matches!((created_ts, updated_ts), (Some(c), Some(u)) if u < c) {
            timestamp_issues.push(json!({
                "target": reference, "field": "updated_at", "issue": "before_created_at"
            }));
        }
        if matches!((created_ts, expires_ts), (Some(c), Some(e)) if e < c) {
            timestamp_issues.push(json!({
                "target": reference, "field": "expires_at", "issue": "before_created_at"
            }));
        }
        let expiry = match expires_ts {
            Some(value) if value <= now => "expired_by_metadata",
            Some(_) => "future_by_metadata",
            None if expires.is_some() => "invalid_metadata",
            None => "unknown",
        };
        let linked_profiles: Vec<&String> = associations
            .get(&reference)
            .map(|profiles| profiles.keys().collect())
            .unwrap_or_default();
        items.push(json!({
            "ref": reference,
            "storage": entry.storage,
            "platform": entry.meta.platform,
            "kind": entry.meta.kind,
            "account": entry.meta.account,
            "org": entry.meta.org,
            "profiles": linked_profiles,
            "created_at": safe_time(created),
            "updated_at": safe_time(updated),
            "expires_at": safe_time(expires),
            "expiry": expiry,
            "validity": health_entries.get(&reference).map(|result| json!({
                "status":result["status"], "checked_at":result["checked_at"],
                "http_status":result["http_status"], "elapsed_ms":result["elapsed_ms"],
                "source":"last_recorded_check", "current_validity":"not_rechecked"
            })).unwrap_or_else(|| json!({"status":"unchecked","reason":"offline_report"})),
            "last_used": last_used.get(&reference),
        }));
    }
    let duplicates_report = if duplicates {
        duplicate_groups(store, &entries, &data)?
    } else {
        json!({"status": "not_requested", "checked": 0, "groups": []})
    };
    let unknown_metadata: Vec<String> = metadata
        .keys()
        .filter(|key| !existing.contains(&format!("secret://{key}")))
        .map(|key| format!("secret://{key}"))
        .collect();
    let unsupported_namespaces: Vec<&String> = data
        .as_object()
        .expect("已验证对象")
        .iter()
        .filter(|(key, value)| {
            !matches!(key.as_str(), "_servers" | "_clis")
                && value.get("keys").is_none()
                && value.get("accounts").is_none()
                && !(key.contains('/') && value.get("ciphertext").is_some())
        })
        .map(|(key, _)| key)
        .collect();
    let active_missing = registry
        .active()
        .filter(|name| registry.get(name).is_none());
    let timestamp_issue_count = timestamp_issues.len();
    Ok(json!({
        "schema_version": 1,
        "generated_at": generated_at,
        "mode": "offline_read_only",
        "summary": {
            "secrets": entries.len(), "unique_refs": existing.len(),
            "profiles": profiles.len(), "by_platform": by_platform,
            "by_kind": by_kind, "by_account": by_account,
            "unchecked_validity": existing.iter().filter(|r| !health_entries.contains_key(*r)).count(),
            "recorded_checks": existing.iter().filter(|r| health_entries.contains_key(*r)).count(),
        },
        "entries": items,
        "profiles": {
            "active": registry.active(), "active_missing": active_missing,
            "items": profiles, "shared_refs": shared_refs,
            "orphan_refs": orphan_refs, "unlinked_refs": unlinked_refs,
        },
        "ambiguous_refs": ambiguous_refs,
        "orphan_metadata_refs": unknown_metadata,
        "unsupported_namespaces": unsupported_namespaces,
        "timestamps": {"issue_count": timestamp_issue_count, "issues": timestamp_issues},
        "audit": audit_report,
        "duplicates": duplicates_report,
        "limitations": [
            "离线报告不实时验证服务端接受、权限或撤销；validity 仅显示最近一次 health.json 记录，没有记录时为 unchecked。",
            "过期时间仅来自可编辑的本地元数据，不代表服务端验证结果。",
            "last_used 仅表示本地日志中观察到的成功 file/get；日志可能关闭、缺失、删除或被修改，actor 不可认证。",
            "重复检查比较原始完整值，不输出明文、尾号或指纹；结果只覆盖当前可解密条目。"
        ]
    }))
}

fn reference_exists(data: &Value, reference: &str) -> bool {
    let Ok((platform, name)) = parse_ref(reference) else {
        return false;
    };
    if platform == "server" {
        let mut parts = name.splitn(2, '/');
        let host = parts.next().unwrap_or_default();
        if let Some(field) = parts.next() {
            return data
                .get("_servers")
                .and_then(|value| value.get(host))
                .and_then(|value| value.get(field))
                .is_some_and(Value::is_string);
        }
        return data
            .get("_servers")
            .and_then(|value| value.get(host))
            .is_some_and(Value::is_object);
    }
    if platform == "cli" {
        if let Some((cli, profile)) = name.split_once('/') {
            return data
                .get("_clis")
                .and_then(|value| value.get(cli))
                .and_then(|value| value.get(profile))
                .is_some_and(Value::is_string);
        }
    }
    if !platform.starts_with('_')
        && ["keys", "accounts"].iter().any(|bucket| {
            data.get(&platform)
                .and_then(|value| value.get(*bucket))
                .and_then(|value| value.get(&name))
                .is_some_and(Value::is_string)
        })
    {
        return true;
    }
    data.get(format!("{platform}/{name}"))
        .and_then(|value| value.get("ciphertext"))
        .is_some_and(Value::is_string)
}

fn read_metadata(store: &Store) -> Result<Map<String, Value>> {
    let bytes = match fs::read(store.meta_file()) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(error).context("读取 meta.json 失败"),
    };
    let value: Value = serde_json::from_slice(&bytes).context("meta.json 语法损坏")?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("meta.json 顶层必须是对象"))?;
    if object.values().any(|value| !value.is_object()) {
        return Err(anyhow!("meta.json 条目必须是对象"));
    }
    Ok(object.clone())
}

fn validate_registry(store: &Store) -> Result<()> {
    let bytes = match fs::read(store.root().join(crate::registry::FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("读取 accounts.json 失败"),
    };
    let value: Value = serde_json::from_slice(&bytes).context("accounts.json 语法损坏")?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("accounts.json 顶层必须是对象"))?;
    if let Some(profiles) = object.get("profiles") {
        let profiles = profiles
            .as_object()
            .ok_or_else(|| anyhow!("accounts.json profiles 必须是对象"))?;
        if profiles.values().any(|profile| !profile.is_object()) {
            return Err(anyhow!("accounts.json profile 条目必须是对象"));
        }
    }
    Ok(())
}

fn validate_store(data: &Value) -> Result<()> {
    let root = data
        .as_object()
        .ok_or_else(|| anyhow!("secrets.json 顶层必须是对象"))?;
    for (name, value) in root {
        if matches!(name.as_str(), "_servers" | "_clis") {
            let groups = value
                .as_object()
                .ok_or_else(|| anyhow!("密钥库命名空间结构损坏"))?;
            for fields in groups.values() {
                validate_ciphertexts(fields)?;
            }
        } else if !name.starts_with('_') {
            let object = value
                .as_object()
                .ok_or_else(|| anyhow!("密钥库平台结构损坏"))?;
            for bucket in ["accounts", "keys"] {
                if let Some(fields) = object.get(bucket) {
                    validate_ciphertexts(fields)?;
                }
            }
            if let Some(ciphertext) = object.get("ciphertext") {
                if !ciphertext.is_string() {
                    return Err(anyhow!("密钥库旧格式密文类型损坏"));
                }
            }
        }
    }
    Ok(())
}

fn validate_ciphertexts(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("密钥库桶结构损坏"))?;
    if object.values().any(|ciphertext| !ciphertext.is_string()) {
        return Err(anyhow!("密钥库密文条目必须是字符串"));
    }
    Ok(())
}

fn inventory(store: &Store, data: &Value, metadata: &Map<String, Value>) -> Result<Vec<Entry>> {
    let known: BTreeMap<String, SecretMeta> = store
        .list_secrets()?
        .into_iter()
        .map(|meta| (meta.r#ref(), meta))
        .collect();
    let mut entries = Vec::new();
    let mut add = |platform: &str, name: &str, storage: &'static str, kind: &str, account: &str| {
        let reference = format!("secret://{platform}/{name}");
        let mut meta = known.get(&reference).cloned().unwrap_or_default();
        meta.platform = platform.into();
        meta.name = name.into();
        meta.kind = kind.into();
        meta.account = account.into();
        if let Some(fields) = metadata
            .get(&format!("{platform}/{name}"))
            .and_then(Value::as_object)
        {
            if let Some(kind) = fields
                .get("kind")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                meta.kind = kind.into();
            }
            if let Some(account) = fields
                .get("account")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                meta.account = account.into();
            }
        }
        entries.push(Entry { meta, storage });
    };
    for platform in store.platforms()?.keys() {
        for name in store.list_bucket(platform, "keys")? {
            add(platform, &name, "keys", "Key", "");
        }
        for name in store.list_bucket(platform, "accounts")? {
            add(platform, &name, "accounts", "Account", &name);
        }
    }
    for (namespace, platform, kind) in [
        ("_servers", "server", "Server Field"),
        ("_clis", "cli", "CLI Token"),
    ] {
        if let Some(groups) = data.get(namespace).and_then(Value::as_object) {
            for (group, fields) in groups {
                for field in fields.as_object().expect("已验证桶结构").keys() {
                    let account = if platform == "cli" {
                        field.as_str()
                    } else {
                        ""
                    };
                    add(
                        platform,
                        &format!("{group}/{field}"),
                        namespace,
                        kind,
                        account,
                    );
                }
            }
        }
    }
    for (name, value) in data.as_object().expect("已验证对象") {
        if !name.starts_with('_') && value.get("keys").is_none() && value.get("accounts").is_none()
        {
            if let Some((platform, name)) = name.split_once('/') {
                if value.get("ciphertext").is_some() {
                    let kind = value.get("kind").and_then(Value::as_str).unwrap_or("Key");
                    let account = value.get("account").and_then(Value::as_str).unwrap_or("");
                    add(platform, name, "legacy", kind, account);
                }
            }
        }
    }
    entries.sort_by(|a, b| {
        (&a.meta.platform, &a.meta.name, a.storage).cmp(&(
            &b.meta.platform,
            &b.meta.name,
            b.storage,
        ))
    });
    Ok(entries)
}

fn duplicate_groups(store: &Store, entries: &[Entry], data: &Value) -> Result<Value> {
    if entries.is_empty() {
        return Ok(json!({"status": "complete", "checked": 0, "groups": [], "failed_refs": []}));
    }
    store
        .master_key()
        .map_err(|_| anyhow!("重复检查需要可用的 master key"))?;
    // 本次调用专用的随机 HMAC key；指纹既不跨次稳定，也不进入 JSON 或文件。
    let mut ephemeral_key = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut ephemeral_key)
        .map_err(|_| anyhow!("无法生成重复检查所需的临时随机密钥"))?;
    let mut groups = BTreeMap::<[u8; 32], Vec<String>>::new();
    let mut failed = Vec::new();
    let mut checked = 0;
    for entry in entries {
        let reference = entry.meta.r#ref();
        if parse_ref(&reference).is_err() {
            failed
                .push(json!({"ref": reference, "storage": entry.storage, "reason": "invalid_ref"}));
            continue;
        }
        let plain = match entry.storage {
            "accounts" => store.get_account(&entry.meta.platform, &entry.meta.name),
            "keys" if matches!(entry.meta.platform.as_str(), "server" | "cli") => {
                store.get_key(&entry.meta.platform, &entry.meta.name)
            }
            "keys" => store.get_secret(&reference),
            "legacy" => {
                // get_secret 优先特殊命名空间和 keys/accounts；旧格式按实际位置读取。
                let key = crate::crypto::derive_file_key(
                    &store
                        .master_key()
                        .map_err(|_| anyhow!("重复检查需要可用的 master key"))?,
                )?;
                let path = format!("{}/{}", entry.meta.platform, entry.meta.name);
                let ciphertext = data[&path]["ciphertext"]
                    .as_str()
                    .expect("已验证旧格式密文");
                crate::crypto::decrypt_joined(ciphertext, &key).map(Some)
            }
            _ => store.get_secret(&reference),
        };
        match plain {
            Ok(Some(plain)) => {
                let mut mac =
                    Hmac::<Sha256>::new_from_slice(&ephemeral_key).expect("HMAC 接受任意长度密钥");
                mac.update(plain.as_bytes());
                let fingerprint: [u8; 32] = mac.finalize().into_bytes().into();
                drop(plain);
                groups.entry(fingerprint).or_default().push(reference);
                checked += 1;
            }
            // Store 的完整性错误可能带 sha256；报告不能转发原始错误。
            _ => failed
                .push(json!({"ref": reference, "storage": entry.storage, "reason": "unreadable"})),
        }
    }
    ephemeral_key.fill(0);
    let mut duplicates: Vec<Vec<String>> =
        groups.into_values().filter(|refs| refs.len() > 1).collect();
    duplicates.sort();
    let duplicates: Vec<Value> = duplicates
        .into_iter()
        .map(|refs| json!({"refs": refs}))
        .collect();
    Ok(json!({
        "status": if failed.is_empty() { "complete" } else { "partial" },
        "checked": checked,
        "groups": duplicates,
        "failed_refs": failed,
    }))
}

fn audit_observations(
    store: &Store,
    now: i64,
    issues: &mut Vec<Value>,
) -> Result<(Value, BTreeMap<String, Value>)> {
    let events = audit::read(store, usize::MAX, None, None)?;
    let raw = match fs::read_to_string(audit::path(store)) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).context("读取本地审计日志失败"),
    };
    let malformed = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| serde_json::from_str::<Value>(line).map_or(true, |value| !value.is_object()))
        .count();
    let mut latest = BTreeMap::<String, (i64, String)>::new();
    let mut successful_gets = 0;
    for event in &events {
        if event["action"] != "get" || event["result"] != "ok" || event["note"] != "file" {
            continue;
        }
        successful_gets += 1;
        let Some(reference) = event["target"].as_str().filter(|r| parse_ref(r).is_ok()) else {
            continue;
        };
        let Some(ts) = check_time(
            issues,
            reference,
            "audit.ts",
            event.get("ts"),
            now,
            true,
            true,
        ) else {
            continue;
        };
        if ts > now + 300 {
            continue;
        }
        if latest
            .get(reference)
            .map_or(true, |(previous, _)| ts > *previous)
        {
            latest.insert(
                reference.into(),
                (ts, event["ts"].as_str().expect("已解析时间").into()),
            );
        }
    }
    let last_used = latest.into_iter().map(|(reference, (_, at))| (reference, json!({
        "at": at, "source": "local_audit_log", "complete": false, "actor_authenticated": false
    }))).collect();
    Ok((
        json!({
            "source": "local_audit_log", "present": audit::path(store).exists(),
            "events_read": events.len(), "malformed_lines": malformed,
            "successful_file_get_events": successful_gets,
            "complete": false, "actor_authenticated": false,
            "last_used_semantics": "仅本地日志观察；缺少事件不代表从未使用，记录的 actor 不构成身份认证。"
        }),
        last_used,
    ))
}

fn safe_time(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| timestamp(value).is_some())
}

fn check_time(
    issues: &mut Vec<Value>,
    target: &str,
    field: &str,
    value: Option<&Value>,
    now: i64,
    required: bool,
    reject_future: bool,
) -> Option<i64> {
    let missing = value.is_none() || value.and_then(Value::as_str) == Some("");
    let parsed = value.and_then(Value::as_str).and_then(timestamp);
    let issue = if missing {
        required.then_some("missing")
    } else if parsed.is_none() {
        Some("invalid")
    } else if reject_future && parsed.is_some_and(|value| value > now + 300) {
        Some("future")
    } else {
        None
    };
    if let Some(issue) = issue {
        issues.push(json!({"target": target, "field": field, "issue": issue}));
    }
    parsed
}

// 接受 RFC3339 的 UTC/偏移与小数秒；按实际日期验证，避免字符串排序误判时区。
fn timestamp(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let number = |start: usize, length: usize| -> Option<i64> {
        let digits = bytes.get(start..start + length)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        digits.iter().try_fold(0i64, |value, digit| {
            value.checked_mul(10)?.checked_add((digit - b'0') as i64)
        })
    };
    let (year, month, day) = (number(0, 4)?, number(5, 2)?, number(8, 2)?);
    let (hour, minute, second) = (number(11, 2)?, number(14, 2)?, number(17, 2)?);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if !(1..=days_in_month).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let mut position = 19;
    if bytes.get(position) == Some(&b'.') {
        position += 1;
        let start = position;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        if position == start {
            return None;
        }
    }
    let offset = match bytes.get(position)? {
        b'Z' | b'z' if position + 1 == bytes.len() => 0,
        sign @ (b'+' | b'-') if position + 6 == bytes.len() && bytes[position + 3] == b':' => {
            let hours = number(position + 1, 2)?;
            let minutes = number(position + 4, 2)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            (hours * 3600 + minutes * 60) * if *sign == b'+' { 1 } else { -1 }
        }
        _ => return None,
    };
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * shifted_month + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{derive_file_key, encrypt_joined};

    fn encrypted_store() -> (tempfile::TempDir, Store) {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(directory.path());
        store.init_master_key().unwrap();
        (directory, store)
    }

    fn snapshot(directory: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
        fs::read_dir(directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    fs::read(entry.path()).unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn default_report_needs_no_master_key_and_never_writes() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(directory.path());
        fs::write(
            store.secrets_path(),
            r#"{"service":{"keys":{"token":"not-even-ciphertext"},"accounts":{}}}"#,
        )
        .unwrap();
        fs::write(store.meta_file(), r#"{"service/token":{"last4":"TAIL-DO-NOT-PRINT","sha256":"HASH-DO-NOT-PRINT","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T00:00:00Z"}}"#).unwrap();
        let before = snapshot(directory.path());
        let result = report(&store, false).unwrap();
        assert_eq!(result["summary"]["secrets"], 1);
        assert_eq!(result["duplicates"]["status"], "not_requested");
        assert_eq!(result["entries"][0]["validity"]["status"], "unchecked");
        let serialized = result.to_string();
        for secret in [
            "not-even-ciphertext",
            "TAIL-DO-NOT-PRINT",
            "HASH-DO-NOT-PRINT",
        ] {
            assert!(!serialized.contains(secret));
        }
        assert_eq!(snapshot(directory.path()), before);
    }

    #[test]
    fn duplicate_scan_covers_accounts_cli_server_and_legacy_without_leaking_values() {
        let (directory, store) = encrypted_store();
        let repeated = "INSPECTION-TEST-PRIVATE-VALUE-9f8e7d6c";
        store
            .set_key("service", "same-name", "unique-key-value")
            .unwrap();
        store.set_account("service", "same-name", repeated).unwrap();
        store.set_cli_token("tool", "work", repeated).unwrap();
        store
            .set_secret("secret://server/host/password", repeated)
            .unwrap();
        let key = derive_file_key(&store.master_key().unwrap()).unwrap();
        let mut data = store.load().unwrap();
        data["legacy/password"] = json!({"ciphertext": encrypt_joined(repeated, &key).unwrap()});
        fs::write(store.secrets_path(), serde_json::to_vec(&data).unwrap()).unwrap();
        let before = snapshot(directory.path());
        let result = report(&store, true).unwrap();
        assert_eq!(result["duplicates"]["checked"], 5);
        assert_eq!(result["duplicates"]["status"], "complete");
        let groups = result["duplicates"]["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["refs"].as_array().unwrap().len(), 4);
        let serialized = result.to_string();
        for forbidden in [
            repeated,
            "unique-key-value",
            "fingerprint",
            "sha256",
            "last4",
        ] {
            assert!(!serialized.contains(forbidden), "报告包含不应输出的材料");
        }
        assert!(serialized.contains("secret://service/same-name"));
        assert_eq!(snapshot(directory.path()), before);
    }

    #[test]
    fn damaged_vault_registry_and_metadata_are_errors() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(directory.path());
        for broken in [
            "{broken",
            "null",
            "[]",
            r#"{"service":{"keys":[]}}"#,
            r#"{"_servers":{"host":null}}"#,
        ] {
            fs::write(store.secrets_path(), broken).unwrap();
            assert!(report(&store, false).is_err());
        }
        fs::write(store.secrets_path(), "{}").unwrap();
        for broken in [
            "{broken",
            "[]",
            "null",
            r#"{"profiles":[]}"#,
            r#"{"profiles":{"work":[]}}"#,
        ] {
            fs::write(directory.path().join(crate::registry::FILE), broken).unwrap();
            assert!(report(&store, false).is_err());
        }
        fs::write(directory.path().join(crate::registry::FILE), "{}").unwrap();
        for broken in ["{broken", "[]", r#"{"service/token":null}"#] {
            fs::write(store.meta_file(), broken).unwrap();
            assert!(report(&store, false).is_err());
        }
    }

    #[test]
    fn profile_links_time_issues_and_untrusted_local_last_used_are_reported() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(directory.path());
        fs::write(
            store.secrets_path(),
            r#"{"service":{"keys":{"token":"ciphertext","unlinked":"ciphertext"}}}"#,
        )
        .unwrap();
        fs::write(store.meta_file(), r#"{"service/token":{"created_at":"2026-01-02T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","expires_at":"2026-02-30T00:00:00Z"}}"#).unwrap();
        fs::write(directory.path().join(crate::registry::FILE), r#"{"profiles":{"one":{"name":"one","refs":["secret://service/token","secret://service/missing"]},"two":{"name":"two","refs":["secret://service/token","invalid-ref"]}}}"#).unwrap();
        fs::write(audit::path(&store), concat!(
            "{\"ts\":\"2026-01-03T00:00:00Z\",\"action\":\"get\",\"target\":\"secret://service/token\",\"result\":\"ok\",\"note\":\"file\",\"actor\":\"unverified-person\"}\n",
            "{\"ts\":\"2026-01-04T00:00:00Z\",\"action\":\"get\",\"target\":\"secret://service/token\",\"result\":\"ok\",\"note\":\"d1\"}\n",
            "{broken\n"
        )).unwrap();
        let result = report(&store, false).unwrap();
        assert_eq!(
            result["profiles"]["shared_refs"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            result["profiles"]["orphan_refs"].as_array().unwrap().len(),
            2
        );
        assert_eq!(
            result["profiles"]["unlinked_refs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let entry = result["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["ref"] == "secret://service/token")
            .unwrap();
        assert_eq!(entry["last_used"]["at"], "2026-01-03T00:00:00Z");
        assert_eq!(entry["last_used"]["actor_authenticated"], false);
        assert_eq!(result["audit"]["complete"], false);
        assert_eq!(result["audit"]["malformed_lines"], 1);
        assert!(result["timestamps"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["issue"] == "before_created_at"));
    }

    #[test]
    fn aggregate_server_profile_and_same_ref_legacy_remain_visible() {
        let (directory, store) = encrypted_store();
        store
            .set_key("service", "token", "repeated-fixture-value")
            .unwrap();
        store
            .set_secret("secret://server/host/password", "other-fixture-value")
            .unwrap();
        let key = derive_file_key(&store.master_key().unwrap()).unwrap();
        let mut data = store.load().unwrap();
        data["service/token"] =
            json!({"ciphertext": encrypt_joined("repeated-fixture-value", &key).unwrap()});
        fs::write(store.secrets_path(), serde_json::to_vec(&data).unwrap()).unwrap();
        fs::write(
            directory.path().join(crate::registry::FILE),
            r#"{"profiles":{"server":{"name":"server","refs":["secret://server/host"]}}}"#,
        )
        .unwrap();
        let result = report(&store, true).unwrap();
        assert_eq!(
            result["profiles"]["orphan_refs"].as_array().unwrap().len(),
            0
        );
        assert_eq!(result["ambiguous_refs"].as_array().unwrap().len(), 1);
        assert_eq!(
            result["duplicates"]["groups"][0]["refs"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let server = result["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["storage"] == "_servers")
            .unwrap();
        assert_eq!(server["profiles"], json!(["server"]));
    }

    #[test]
    fn timestamps_validate_calendar_and_normalize_offsets() {
        assert_eq!(timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            timestamp("2026-01-01T00:00:00Z"),
            timestamp("2026-01-01T08:00:00.123+08:00")
        );
        assert!(timestamp("2024-02-29T23:59:59Z").is_some());
        for invalid in [
            "2026-02-29T00:00:00Z",
            "2026-01-01T24:00:00Z",
            "2026-01-01T00:00:00+24:00",
            "2026-01-01T00:00:00.Z",
            "密钥不应作为日期回显",
        ] {
            assert!(timestamp(invalid).is_none());
        }
    }
}
