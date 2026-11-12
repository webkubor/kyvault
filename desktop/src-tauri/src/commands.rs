use kyvault::{
    desktop::{redact, DesktopHistory, HistoryEntry},
    store::Store,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};

type Result<T> = std::result::Result<T, String>;

pub struct DesktopState {
    pub history: DesktopHistory,
    pub session: Mutex<Session>,
    pub data_root: PathBuf,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Prefs {
    pub lang: String,
    pub monitoring: bool,
}
impl Default for Prefs {
    fn default() -> Self {
        Self {
            lang: "zh-CN".into(),
            // Clipboard history is the product. It is on by default and the
            // user turns it off explicitly in Settings.
            monitoring: true,
        }
    }
}

pub fn prefs_path(state: &DesktopState) -> PathBuf {
    state.data_root.join("prefs.json")
}
pub fn load_prefs(state: &DesktopState) -> Prefs {
    load_prefs_for_root(&state.data_root)
}
/// Read preferences without needing managed state (used during startup).
pub fn load_prefs_for_root(root: &std::path::Path) -> Prefs {
    match fs::read_to_string(root.join("prefs.json")) {
        Ok(raw) => serde_json::from_str::<Prefs>(&raw).unwrap_or_default(),
        Err(_) => Prefs::default(),
    }
}
pub fn save_prefs(state: &DesktopState, prefs: &Prefs) -> Result<()> {
    let dir = &state.data_root;
    let tmp = dir.join(".prefs.json.tmp");
    {
        let data =
            serde_json::to_vec_pretty(prefs).map_err(|_| "序列化本地偏好失败".to_string())?;
        fs::write(&tmp, data).map_err(|_| "写入本地偏好失败".to_string())?;
        fs::rename(&tmp, prefs_path(state)).map_err(|_| "替换本地偏好失败".to_string())?;
    }
    // Best-effort restrictive permissions on the preferences file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(prefs_path(state), fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
pub struct Session {
    pub locked: bool,
    pub monitoring: bool,
    pub last_activity: Instant,
    pub clear_at: Option<(Instant, [u8; 32])>,
    recent_secret_fingerprints: HashSet<[u8; 32]>,
}
impl Session {
    /// Build a session that resumes the persisted monitoring choice.
    pub fn resumed(monitoring: bool) -> Self {
        Self {
            monitoring,
            ..Self::default()
        }
    }
}
impl Default for Session {
    fn default() -> Self {
        Self {
            locked: false,
            // Clipboard history is the product: capture is on by default.
            monitoring: true,
            last_activity: Instant::now(),
            clear_at: None,
            recent_secret_fingerprints: HashSet::new(),
        }
    }
}
/// Normalization catches leading/trailing whitespace without retaining plaintext.
pub fn fingerprint(text: &str) -> [u8; 32] {
    Sha256::digest(text.trim().as_bytes()).into()
}
impl Session {
    /// Checks exact values and individual lines. This is not arbitrary substring
    /// detection: an embedded unlabelled secret still requires user review.
    pub fn is_sensitive(&self, text: &str) -> bool {
        self.recent_secret_fingerprints.contains(&fingerprint(text))
            || text.lines().any(|line| {
                self.recent_secret_fingerprints.contains(&fingerprint(line))
                    || self
                        .recent_secret_fingerprints
                        .contains(&fingerprint(line.trim().trim_matches(['\"', '\''])))
            })
    }
    pub fn remember_secret(&mut self, text: &str) -> Result<()> {
        let hash = fingerprint(text);
        if !self.recent_secret_fingerprints.contains(&hash)
            && self.recent_secret_fingerprints.len() >= 256
        {
            return Err("本次会话已保护 256 个敏感值，请重启后继续复制密钥".into());
        }
        self.recent_secret_fingerprints.insert(hash);
        Ok(())
    }
}
fn active(state: &DesktopState) -> Result<MutexGuard<'_, Session>> {
    let mut session = state.session.lock().map_err(|_| "会话不可用，请重启 Ky")?;
    if session.locked {
        return Err("内容已隐藏，请先返回工作台".into());
    }
    session.last_activity = Instant::now();
    Ok(session)
}
fn vault() -> Result<Store> {
    Store::default_location().map_err(|_| "无法打开本地密钥库，请检查 ky doctor".into())
}
fn validate_reference(reference: &str) -> Result<()> {
    if reference.len() > 240
        || reference
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("引用最多 240 字符，不能包含空白或控制字符".into());
    }
    kyvault::model::parse_ref(reference)
        .map(|_| ())
        .map_err(|_| "引用格式应为 secret://平台/名称".into())
}
#[derive(Serialize)]
pub struct Entry {
    id: u64,
    created_at: u64,
    text: String,
    redacted: bool,
    snippet: Option<String>,
    version: Option<u64>,
}
impl From<HistoryEntry> for Entry {
    fn from(e: HistoryEntry) -> Self {
        Self {
            id: e.id,
            created_at: e.created_at,
            text: e.text,
            redacted: e.redacted,
            snippet: e.name,
            version: e.version,
        }
    }
}
#[derive(Serialize)]
pub struct Secret {
    reference: String,
    platform: String,
    name: String,
}
#[derive(Serialize)]
pub struct Snapshot {
    entries: Vec<Entry>,
    secrets: Vec<Secret>,
    monitoring: bool,
    locked: bool,
    /// Active interface language, persisted in the local preferences file.
    lang: String,
}
fn read_snapshot(state: &DesktopState, session: &Session) -> Result<Snapshot> {
    if session.locked {
        return Ok(Snapshot {
            entries: vec![],
            secrets: vec![],
            monitoring: false,
            locked: true,
            lang: load_prefs(state).lang,
        });
    }
    let store = vault()?;
    let secrets = store
        .list_secrets()
        .map_err(|_| "读取密钥列表失败，请检查 ky doctor")?
        .into_iter()
        .map(|meta| Secret {
            reference: meta.r#ref(),
            platform: meta.platform,
            name: meta.name,
        })
        .collect();
    let entries = state
        .history
        .list()
        .map_err(|_| "读取加密历史失败，请检查本地目录或密钥文件")?
        .into_iter()
        .map(Entry::from)
        .collect();
    Ok(Snapshot {
        entries,
        secrets,
        monitoring: session.monitoring,
        lang: load_prefs(state).lang,
        locked: false,
    })
}
#[tauri::command]
pub fn snapshot(state: State<DesktopState>) -> Result<Snapshot> {
    let session = state.session.lock().map_err(|_| "会话不可用")?;
    read_snapshot(&state, &session)
}
#[tauri::command]
pub fn preview_text(text: String, state: State<DesktopState>) -> Result<serde_json::Value> {
    let _session = active(&state)?;
    if text.len() > 65_536 {
        return Err("文本不能超过 64 KiB".into());
    }
    let safe = redact(&text);
    Ok(serde_json::json!({"redacted":safe != text,"text":safe}))
}
#[tauri::command]
pub fn capture_clipboard(state: State<DesktopState>) -> Result<Snapshot> {
    let session = active(&state)?;
    let text = arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|_| "剪贴板没有可读取的文本")?;
    if session.is_sensitive(&text) {
        return Err("这是从密钥库复制的敏感值，不能加入剪贴板历史".into());
    }
    state
        .history
        .capture(&text)
        .map_err(|_| "无法保存：文本须为 1–65536 字节，且本地历史目录可写")?;
    read_snapshot(&state, &session)
}
#[tauri::command]
pub fn set_monitoring(enabled: bool, state: State<DesktopState>) -> Result<bool> {
    let mut session = active(&state)?;
    session.monitoring = enabled;
    let prefs = Prefs {
        lang: load_prefs(&state).lang,
        monitoring: enabled,
    };
    save_prefs(&state, &prefs)?;
    Ok(enabled)
}
#[tauri::command]
pub fn save_text(
    text: String,
    snippet: Option<String>,
    state: State<DesktopState>,
) -> Result<Snapshot> {
    let session = active(&state)?;
    if session.is_sensitive(&text) {
        return Err("从密钥库复制的敏感值不能保存为历史或片段".into());
    }
    let result = match snippet.filter(|s| !s.trim().is_empty()) {
        Some(name) => state.history.save_version(&name, &text),
        None => state.history.capture(&text),
    };
    result.map_err(|_| "保存失败，请检查文本长度、名称和本地目录权限")?;
    read_snapshot(&state, &session)
}
#[tauri::command]
pub fn delete_entry(id: u64, state: State<DesktopState>) -> Result<Snapshot> {
    let session = active(&state)?;
    state.history.delete(id).map_err(|_| "删除失败")?;
    read_snapshot(&state, &session)
}
#[tauri::command]
pub fn restore_entry(id: u64, state: State<DesktopState>) -> Result<Snapshot> {
    let session = active(&state)?;
    state
        .history
        .restore(id)
        .map_err(|_| "版本不存在或不是命名片段")?;
    read_snapshot(&state, &session)
}
fn copy(text: String) -> Result<()> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|_| "无法写入系统剪贴板".into())
}
#[tauri::command]
pub fn copy_entry(id: u64, state: State<DesktopState>) -> Result<()> {
    let _session = active(&state)?;
    let entry = state
        .history
        .list()
        .map_err(|_| "读取历史失败")?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("记录已过期或被删除")?;
    copy(entry.text)
}
#[tauri::command]
pub fn copy_reference(reference: String, state: State<DesktopState>) -> Result<()> {
    let _session = active(&state)?;
    validate_reference(&reference)?;
    copy(reference)
}
#[tauri::command]
pub fn copy_secret(reference: String, state: State<DesktopState>) -> Result<()> {
    let mut session = active(&state)?;
    validate_reference(&reference)?;
    let value = vault()?
        .get_secret(&reference)
        .map_err(|_| "密钥读取失败")?
        .ok_or("密钥不存在")?;
    session.remember_secret(&value)?;
    let hash = fingerprint(&value);
    copy(value)?;
    session.clear_at = Some((Instant::now() + Duration::from_secs(30), hash));
    Ok(())
}
#[tauri::command]
pub fn save_secret(
    reference: String,
    value: String,
    state: State<DesktopState>,
) -> Result<Snapshot> {
    let session = active(&state)?;
    validate_reference(&reference)?;
    if value.is_empty() || value.len() > 65_536 {
        return Err("密钥须为 1–65536 字节".into());
    }
    let store = vault()?;
    // The shared store owns encryption and locking; the UI requires explicit confirmation for replacement.
    store
        .set_secret(&reference, &value)
        .map_err(|_| "密钥保存失败，请检查本地密钥库")?;
    read_snapshot(&state, &session)
}

/// 手动录入升级版：平台下拉 + 账号 + 别名。API Key 平台先在线校验再入库。
#[tauri::command]
pub fn save_secret_with_meta(
    reference: String,
    value: String,
    kind: Option<String>,
    account: Option<String>,
    alias: Option<String>,
) -> Result<serde_json::Value> {
    let _session_guard = (); // 校验在 fulfill 之外：手动录入不绑定请求
    validate_reference(&reference)?;
    if value.is_empty() || value.len() > 65_536 {
        return Err("密钥须为 1–65536 字节".into());
    }
    let (platform, _) =
        kyvault::model::parse_ref(&reference).map_err(|_| "引用格式错误".to_string())?;
    let store = vault()?;
    let aliases = kyvault::alias::Aliases::default_location().map_err(|e| e.to_string())?;

    // 有校验端点的平台先验一把：401 的 key 不该进库。查不了（无端点/网络不通）
    // 就按未验证入库，并如实返回 —— 不假装验过。
    let (check_status, check_message) = if kyvault::providers::get_provider(&platform).is_some() {
        let r = kyvault::providers::validate_key(&platform, &value);
        if r.valid {
            (r.status.to_string(), r.message)
        } else if r.status == "unsupported" || r.status == "unknown" {
            (
                "unverified".to_string(),
                "已保存，但这次没能在线验证（网络或平台不支持）".to_string(),
            )
        } else {
            return Err(format!(
                "校验未通过（{}），密钥没有入库：{}",
                r.status, r.message
            ));
        }
    } else {
        (
            "unverified".to_string(),
            "平台无在线校验接口，已按原样入库".to_string(),
        )
    };

    let kind = kind
        .filter(|k| !k.trim().is_empty())
        .map(|k| k.trim().to_string())
        .or_else(|| default_kind(&platform));
    let account = account.filter(|a| !a.trim().is_empty());
    store
        .set_secret_with_meta(&reference, &value, kind.as_deref(), account.as_deref())
        .map_err(|_| "密钥保存失败，请检查本地密钥库")?;

    let alias = alias
        .map(|a| a.trim().to_lowercase())
        .filter(|a| !a.is_empty());
    if let Some(a) = &alias {
        aliases.set(a, &reference).map_err(|_| "别名写入失败")?;
    }
    let last4 = kyvault::model::last4(&value);
    Ok(serde_json::json!({
        "ok": true,
        "reference": reference,
        "status": check_status,
        "message": check_message,
        "alias": alias.unwrap_or_default(),
        "last4": last4,
    }))
}

fn default_kind(platform: &str) -> Option<String> {
    let kind = match platform {
        "github" | "gitlab" | "openrouter" | "cloudflare" | "feishu" => "Token",
        _ => "API Key",
    };
    Some(kind.to_string())
}

/// Agent 发起的待录入请求清单（只有元信息，永远没有密文）。
#[tauri::command]
pub fn pending_requests() -> Result<Vec<serde_json::Value>> {
    let items = kyvault::requests::summaries().map_err(|e| e.to_string())?;
    Ok(items
        .into_iter()
        .map(|m| {
            serde_json::json!({
                "id": m["id"], "platform": m["platform"], "name": m["name"],
                "account": m["account"], "kind": m["kind"], "alias": m["alias"],
                "note": m["note"], "requested_by": m["requested_by"],
                "created_at": m["created_at"],
            })
        })
        .collect())
}

/// 按 Agent 请求入库：校验 → 加密写库 → 建别名 → 写回执。
///
/// 校验失败时返回 Err，请求留在 pending，人可以改值重试。
#[tauri::command]
pub fn fulfill_request(id: String, value: String) -> Result<serde_json::Value> {
    let store = vault()?;
    let aliases = kyvault::alias::Aliases::default_location().map_err(|e| e.to_string())?;
    let outcome =
        kyvault::requests::fulfill(&store, &aliases, &id, &value).map_err(|e| e.to_string())?;
    serde_json::to_value(&outcome).map_err(|e| e.to_string())
}

/// 人明确放弃一条请求。
#[tauri::command]
pub fn discard_request(id: String) -> Result<()> {
    kyvault::requests::fail(&id, "已被用户放弃").map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_secret(reference: String, state: State<DesktopState>) -> Result<Snapshot> {
    let session = active(&state)?;
    validate_reference(&reference)?;
    vault()?
        .delete_secret(&reference)
        .map_err(|_| "密钥删除失败")?;
    read_snapshot(&state, &session)
}
pub fn lock(app: &tauri::AppHandle) -> Result<()> {
    let state = app.state::<DesktopState>();
    let mut session = state.session.lock().map_err(|_| "会话不可用")?;
    session.locked = true;
    // Locking hides content; it must not silently switch off clipboard
    // history. The capture loop already skips while locked, and the user's
    // choice stays in prefs.json so unlocking restores it.
    app.emit("app-locked", ())
        .map_err(|_| "窗口通知失败".into())
}
#[tauri::command]
pub fn lock_app(app: tauri::AppHandle) -> Result<()> {
    lock(&app)
}
#[tauri::command]
pub fn unlock_app(state: State<DesktopState>) -> Result<Snapshot> {
    let mut session = state.session.lock().map_err(|_| "会话不可用")?;
    session.locked = false;
    session.last_activity = Instant::now();
    // Re-read the persisted choice so unlocking never leaves capture in a
    // state the user cannot see in Settings.
    session.monitoring = load_prefs(&state).monitoring;
    read_snapshot(&state, &session)
}
#[tauri::command]
pub fn clear_history(state: State<DesktopState>) -> Result<Snapshot> {
    let session = active(&state)?;
    state.history.clear().map_err(|_| "清空历史失败")?;
    read_snapshot(&state, &session)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copied_secrets_stay_suppressed_after_replacement_and_lock() {
        let mut session = Session::default();
        session.remember_secret("first synthetic value").unwrap();
        session.remember_secret("second synthetic value").unwrap();
        session.clear_at = None; // Clipboard was cleared or changed.
        session.locked = true;
        assert!(session.is_sensitive("first synthetic value"));
        assert!(session.is_sensitive("second synthetic value"));
        assert!(session.is_sensitive("header\n  first synthetic value \nfooter"));
        assert!(session.is_sensitive("\"first synthetic value\""));
        assert!(!session.is_sensitive("ordinary non-sensitive text"));
    }
    #[test]
    fn fingerprint_capacity_rejects_new_secrets_without_forgetting() {
        let mut session = Session::default();
        for n in 0..256 {
            session.remember_secret(&format!("fixture {n}")).unwrap();
        }
        assert!(session.remember_secret("new fixture").is_err());
        assert!(session.remember_secret("fixture 0").is_ok());
        assert!(session.is_sensitive("fixture 0"));
        assert!(session.is_sensitive("fixture 255"));
    }
    #[test]
    fn pending_clear_holds_only_a_normalized_digest() {
        let mut session = Session::default();
        session.remember_secret("synthetic value").unwrap();
        let digest = fingerprint("synthetic value");
        session.clear_at = Some((Instant::now(), digest));
        assert_eq!(digest.len(), 32);
        assert_eq!(fingerprint(" synthetic value\n"), digest);
        assert_ne!(fingerprint("different"), digest);
        // A failed copy/clear leaves the independent suppression set intact.
        session.clear_at = None;
        assert!(session.is_sensitive("synthetic value"));
    }

    #[test]
    fn clipboard_history_is_on_by_default() {
        // The product is clipboard history; a fresh install must capture
        // without the user hunting for a switch.
        assert!(Prefs::default().monitoring);
        assert!(Session::default().monitoring);
    }
}

#[tauri::command]
pub fn touch_activity(state: State<DesktopState>) -> Result<()> {
    drop(active(&state)?);
    Ok(())
}

#[tauri::command]
pub fn set_ui_language(lang: String, state: State<DesktopState>) -> Result<Snapshot> {
    if lang != "zh-CN" && lang != "en" {
        return Err("不支持的语言".into());
    }
    let session = active(&state)?;
    let prefs = Prefs {
        lang: lang.clone(),
        monitoring: session.monitoring,
    };
    save_prefs(&state, &prefs)?;
    read_snapshot(&state, &session)
}
