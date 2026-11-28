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
use tauri_plugin_updater::UpdaterExt;

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
pub(super) fn active(state: &DesktopState) -> Result<MutexGuard<'_, Session>> {
    let mut session = state.session.lock().map_err(|_| "会话不可用，请重启 Ky")?;
    if session.locked {
        return Err("内容已隐藏，请先返回工作台".into());
    }
    session.last_activity = Instant::now();
    Ok(session)
}
pub(super) fn vault() -> Result<Store> {
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
    source: Option<String>,
    /// 图片条目：宽/高/PNG 字节数。像素数据由 get_entry_image 单独取。
    image: Option<kyvault::desktop::ImageMeta>,
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
            source: e.source,
            image: e.image,
        }
    }
}

/// 剪贴板 RGBA 像素 → PNG 字节。超大图（>12000px 边）或编码失败返回 None。
/// 抓取线程（main.rs）与手动抓取（capture_clipboard）共用。
pub fn encode_rgba_png_pub(width: usize, height: usize, bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Cursor;
    if width == 0 || height == 0 || width > 12_000 || height > 12_000 {
        return None;
    }
    let expected = width * height * 4;
    if bytes.len() != expected {
        // 有些平台给的是 BGRA 或带 padding；长度不符时不猜格式，直接放弃。
        return None;
    }
    let rgba = image::RgbaImage::from_raw(width as u32, height as u32, bytes.to_vec())?;
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut out, image::ImageFormat::Png)
        .ok()?;
    let buf = out.into_inner();
    (buf.len() <= 8 * 1024 * 1024).then_some(buf)
}

/// 取某条图片的 PNG 像素（base64），供前端 <img> 渲染。
#[tauri::command]
pub fn get_entry_image(id: u64, state: State<DesktopState>) -> Result<serde_json::Value> {
    let _session = active(&state)?;
    let png = state
        .history
        .image_data(id)
        .map_err(|_| "图片读取失败或已过期")?;
    use base64::Engine;
    Ok(serde_json::json!({ "png": base64::engine::general_purpose::STANDARD.encode(png) }))
}
/// 一条发布记录（供"关于/更新"对话框展示版本与更新日志）。
#[derive(Serialize)]
pub struct ReleaseNote {
    pub version: String,
    pub name: String,
    pub body: String,
    pub published_at: String,
    pub url: String,
}

#[derive(Serialize)]
pub struct UpdatePreview {
    version: String,
    notes: Option<String>,
}

/// Fetch the configured manifest on every request. None means a successful check.
#[tauri::command]
pub async fn check_update<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<UpdatePreview>> {
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    updater
        .check()
        .await
        .map(|update| {
            update.map(|u| UpdatePreview {
                version: u.version,
                notes: u.body,
            })
        })
        .map_err(|e| e.to_string())
}

/// 用户主动点"更新"：检查并安装最新版，装完重启。
/// 结果与进度通过 update-progress 事件发给前端（含下载百分比）。
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle, expected_version: String) -> Result<()> {
    static INSTALL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let guard = INSTALL.try_lock().map_err(|_| "install_busy")?;
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| "update_unavailable")?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "version_changed".to_string())?;
    let version = update.version.clone();
    if version != expected_version {
        return Err("version_changed".into());
    }
    let mut downloaded = 0usize;
    let mut last_report = Instant::now();
    let _ = app.emit(
        "update-progress",
        serde_json::json!({"phase":"downloading","version":version}),
    );
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk;
                if last_report.elapsed() < Duration::from_millis(150) && total != Some(downloaded as u64) {
                    return;
                }
                last_report = Instant::now();
                let _ = app.emit(
                    "update-progress",
                    serde_json::json!({
                        "phase":"downloading","version":version,"downloaded":downloaded,"total":total
                    }),
                );
            },
            || { let _ = app.emit("update-progress", serde_json::json!({"phase":"installing","version":version})); },
        )
        .await
        .map_err(|e| e.to_string())?;
    let _ = app.emit(
        "update-progress",
        serde_json::json!({"phase":"installed","version":version}),
    );
    // 给前端一点时间显示"安装完成"，再重启（重启逻辑交给前端 setTimeout 调 restart_app，
    // 或直接在这里延迟重启）。这里直接延迟重启，前端会收到 installed 事件显示提示。
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = guard;
        tokio::time::sleep(Duration::from_secs(2)).await;
        handle.restart();
    });
    Ok(())
}

/// A failed request is distinct from a successful empty history.
#[tauri::command]
pub async fn get_app_info() -> Result<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(release_history)
        .await
        .map_err(|_| "history_unavailable")?
}

fn release_history() -> Result<serde_json::Value> {
    let releases: Vec<serde_json::Value> =
        ureq::get("https://api.github.com/repos/webkubor/kyvault/releases?per_page=100")
            .set("Accept", "application/vnd.github+json")
            .set(
                "User-Agent",
                concat!("ky-desktop/", env!("CARGO_PKG_VERSION")),
            )
            .timeout(Duration::from_secs(10))
            .call()
            .map_err(|_| "history_unavailable")?
            .into_json()
            .map_err(|_| "history_unavailable")?;
    let notes: Vec<_> = releases
        .iter()
        .filter_map(|r| {
            if r["draft"] == true || r["prerelease"] == true {
                return None;
            }
            let version = r["tag_name"].as_str()?.strip_prefix("desktop-v")?;
            Some(ReleaseNote {
                version: version.to_owned(),
                name: r["name"].as_str().unwrap_or_default().into(),
                body: r["body"].as_str().unwrap_or_default().into(),
                published_at: r["published_at"].as_str().unwrap_or_default().into(),
                url: r["html_url"].as_str().unwrap_or_default().into(),
            })
        })
        .collect();
    Ok(serde_json::json!({"current":env!("CARGO_PKG_VERSION"),"notes":notes}))
}

#[derive(Serialize)]
pub struct Secret {
    account: String,
    kind: String,
    aliases: Vec<String>,
    reference: String,
    platform: String,
    name: String,
}
#[derive(Serialize)]
pub struct Snapshot {
    native_reveal: bool,
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
            native_reveal: cfg!(target_os = "macos"),
            entries: vec![],
            secrets: vec![],
            monitoring: false,
            locked: true,
            lang: load_prefs(state).lang,
        });
    }
    let store = vault()?;
    let aliases = kyvault::alias::Aliases::default_location()
        .map_err(|_| "无法定位别名表")?
        .load();
    let mut by_ref = std::collections::HashMap::<String, Vec<String>>::new();
    for (alias, reference) in aliases {
        by_ref.entry(reference).or_default().push(alias);
    }
    let secrets = store
        .list_secrets()
        .map_err(|_| "读取密钥列表失败，请检查 ky doctor")?
        .into_iter()
        .map(|meta| Secret {
            aliases: by_ref.remove(&meta.r#ref()).unwrap_or_default(),
            reference: meta.r#ref(),
            kind: meta.kind,
            account: meta.account,
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
        native_reveal: cfg!(target_os = "macos"),
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
pub fn preview_text(
    text: String,
    reveal: Option<bool>,
    state: State<DesktopState>,
) -> Result<serde_json::Value> {
    let _session = active(&state)?;
    if text.len() > 65_536 {
        return Err("文本不能超过 64 KiB".into());
    }
    let needs_mask = kyvault::desktop::needs_mask(&text);
    // reveal=false（默认）只给打码摘要；用户显式点"显示原文"才返回原文。
    let body = if needs_mask && !reveal.unwrap_or(false) {
        redact(&text)
    } else {
        text
    };
    Ok(serde_json::json!({"redacted":needs_mask,"text":body,"revealed":reveal.unwrap_or(false)}))
}
#[tauri::command]
pub fn capture_clipboard(state: State<DesktopState>) -> Result<Snapshot> {
    let session = active(&state)?;
    let mut cb = arboard::Clipboard::new().map_err(|_| "剪贴板不可用")?;
    let text = cb.get_text().unwrap_or_default();
    if !text.is_empty() {
        if session.is_sensitive(&text) {
            return Err("这是从密钥库复制的敏感值，不能加入剪贴板历史".into());
        }
        state
            .history
            .capture(&text, Some("Ky"))
            .map_err(|_| "无法保存：文本须为 1–65536 字节，且本地历史目录可写")?;
        return read_snapshot(&state, &session);
    }
    // 没有文本 → 试图片（手动抓取按钮对图片同样生效）。
    if let Ok(img) = cb.get_image() {
        if let Some(png) = encode_rgba_png_pub(img.width, img.height, &img.bytes) {
            let (w, h) = (img.width as u32, img.height as u32);
            state
                .history
                .capture_image(&png, w, h, Some("Ky"))
                .map_err(|_| "无法保存图片：本地历史目录不可写或图片过大")?;
        }
    }
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
        None => state.history.capture(&text, Some("Ky")),
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
    let mut session = active(&state)?;
    let entry = state
        .history
        .list()
        .map_err(|_| "读取历史失败")?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("记录已过期或被删除")?;
    // 图片条目：解密像素 → 写回剪贴板（图片不敏感，无需清除计时）。
    if entry.image.is_some() {
        let png = state
            .history
            .image_data(id)
            .map_err(|_| "图片读取失败或已过期")?;
        let img = image::load_from_memory(&png).map_err(|_| "图片解码失败")?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let data = arboard::ImageData {
            width: w as usize,
            height: h as usize,
            bytes: std::borrow::Cow::Owned(rgba.into_raw()),
        };
        return arboard::Clipboard::new()
            .and_then(|mut c| c.set_image(data))
            .map_err(|_| "无法写入系统剪贴板".into());
    }
    // 敏感条目复制的是原文，走与密钥库相同的 30s 自动清除，安全不退化。
    if entry.redacted {
        session.remember_secret(&entry.text)?;
        let hash = fingerprint(&entry.text);
        copy(entry.text)?;
        session.clear_at = Some((Instant::now() + Duration::from_secs(30), hash));
        Ok(())
    } else {
        copy(entry.text)
    }
}
#[tauri::command]
pub fn copy_reference(reference: String, state: State<DesktopState>) -> Result<()> {
    let _session = active(&state)?;
    validate_reference(&reference)?;
    copy(reference)
}
#[tauri::command]
pub fn show_secret(
    window: tauri::WebviewWindow,
    reference: String,
    state: State<DesktopState>,
) -> Result<()> {
    let _session = active(&state)?;
    validate_reference(&reference)?;
    #[cfg(target_os = "macos")]
    {
        let value = vault()?
            .get_secret(&reference)
            .map_err(|_| "密钥读取失败")?
            .ok_or("密钥不存在")?;
        let parent = window.ns_window().map_err(|_| "窗口不可用")?;
        extern "C" {
            fn ky_show_secret(
                window: *mut std::ffi::c_void,
                bytes: *const u8,
                len: usize,
                english: bool,
            ) -> bool;
        }
        // Synchronous Tauri commands execute on the main thread. AppKit copies
        // the bytes into its sheet; no plaintext crosses the WebView bridge.
        let shown = unsafe {
            ky_show_secret(
                parent,
                value.as_ptr(),
                value.len(),
                load_prefs(&state).lang == "en",
            )
        };
        if shown {
            Ok(())
        } else {
            Err("当前窗口无法显示密钥".into())
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("当前平台不支持原生查看".into())
    }
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

/// 手动录入。普通密码只加密写入本地，API 凭据沿用平台校验。
#[tauri::command(async)]
pub fn save_secret_with_meta(
    reference: String,
    value: String,
    kind: Option<String>,
    account: Option<String>,
    alias: Option<String>,
    state: State<'_, DesktopState>,
) -> Result<serde_json::Value> {
    save_unlocked_secret(&state, &reference, &value, kind, account, alias)
}

fn save_unlocked_secret(
    state: &DesktopState,
    reference: &str,
    value: &str,
    kind: Option<String>,
    account: Option<String>,
    alias: Option<String>,
) -> Result<serde_json::Value> {
    let _session = active(state)?;
    let store = vault()?;
    save_manual_secret(
        &store,
        reference,
        value,
        kind,
        account,
        alias,
        validate_manual_api_key,
    )
}

fn save_manual_secret(
    store: &Store,
    reference: &str,
    value: &str,
    kind: Option<String>,
    account: Option<String>,
    alias: Option<String>,
    validate_api_key: impl FnOnce(&str, &str) -> Result<(String, String)>,
) -> Result<serde_json::Value> {
    validate_reference(reference)?;
    if value.is_empty() || value.len() > 65_536 {
        return Err("凭据须为 1–65536 字节".into());
    }
    let (platform, name) = kyvault::model::parse_ref(reference).map_err(|_| "引用格式错误")?;
    let meta = store
        .load_meta()
        .map_err(|_| "无法读取凭据元数据，请先恢复密钥库")?;
    let kind = kind
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .or_else(|| {
            meta.get(&format!("{platform}/{name}"))?
                .get("kind")?
                .as_str()
        })
        .map(str::to_owned)
        .or_else(|| default_kind(&platform));
    store.prepare_desktop_key().map_err(|_| {
        "无法使用本地加密密钥。请检查目录权限，已有库请先恢复原 master key 或旧 Auth Guard。"
    })?;
    let (check_status, check_message) = if kind
        .as_deref()
        .is_some_and(|k| k.trim().eq_ignore_ascii_case("Password"))
    {
        ("local".to_owned(), "密码已在本地加密保存".to_owned())
    } else {
        validate_api_key(&platform, value)?
    };
    store
        .set_secret_with_meta(reference, value, kind.as_deref(), account.as_deref())
        .map_err(|_| "凭据保存失败，请检查本地密钥库")?;
    let alias = alias
        .map(|a| a.trim().to_lowercase())
        .filter(|a| !a.is_empty());
    if let Some(a) = &alias {
        let _lock = store.lock().map_err(|_| "无法锁定别名文件")?;
        kyvault::alias::Aliases::new(store.root())
            .set(a, reference)
            .map_err(|_| "凭据已保存，别名写入失败")?;
    }
    Ok(serde_json::json!({
        "ok": true,
        "reference": reference,
        "status": check_status,
        "message": check_message,
        "alias": alias.unwrap_or_default(),
    }))
}

#[cfg(test)]
#[test]
fn locked_manual_save_fails_before_opening_vault() {
    let dir = tempfile::tempdir().unwrap();
    let state = DesktopState {
        history: DesktopHistory::new(dir.path()),
        session: Mutex::new(Session {
            locked: true,
            ..Session::default()
        }),
        data_root: dir.path().into(),
    };
    let result = save_unlocked_secret(
        &state,
        "secret://openai/sample",
        "q9X",
        Some("Password".into()),
        None,
        None,
    );
    assert_eq!(result.unwrap_err(), "内容已隐藏，请先返回工作台");
}

#[cfg(test)]
#[test]
fn manual_password_never_invokes_provider_or_returns_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let reference = "secret://openai/fixture";
    let password = "q9X";
    for kind in [Some("Password".into()), None] {
        let receipt = save_manual_secret(
            &store,
            reference,
            password,
            kind,
            Some("fixture-account".into()),
            None,
            |_, _| panic!("password reached API validation"),
        )
        .unwrap();
        assert_eq!(receipt["status"], "local");
        assert!(receipt.get("last4").is_none());
        assert!(!receipt.to_string().contains(password));
    }
    assert!(store.get_secret(reference).unwrap().as_deref() == Some(password));
    let meta = store.load_meta().unwrap();
    assert_eq!(meta["openai/fixture"]["account"], "fixture-account");
    assert_eq!(meta["openai/fixture"]["kind"], "Password");
    assert!(meta["openai/fixture"].get("sha256").is_none());
    assert!(!fs::read_to_string(store.secrets_path())
        .unwrap()
        .contains(&format!("\"{password}\"")));
}

fn validate_manual_api_key(platform: &str, value: &str) -> Result<(String, String)> {
    // 有校验端点的平台先验一把：401 的 key 不该进库。查不了（无端点/网络不通）
    // 就按未验证入库，并如实返回 —— 不假装验过。
    let result = if platform == "xiaomi" {
        (
            "unverified".to_string(),
            "小米 Token Plan Key 已保存；按小米规则不做自动探测，仅用于兼容工具中的交互式使用"
                .to_string(),
        )
    } else if kyvault::providers::get_provider(platform).is_some() {
        let r = kyvault::providers::validate_key(platform, value);
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

    Ok(result)
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
    #[cfg(target_os = "macos")]
    crate::flash::native::lock(app);
    #[cfg(target_os = "macos")]
    app.run_on_main_thread(|| {
        extern "C" {
            fn ky_hide_secret();
        }
        unsafe { ky_hide_secret() };
    })
    .map_err(|_| "窗口不可用")?;
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
    fn update_preview_serializes_version_and_optional_manifest_notes() {
        for notes in [None, Some("Fixture release notes".to_owned())] {
            let preview = UpdatePreview {
                version: "0.4.0".to_owned(),
                notes: notes.clone(),
            };
            assert_eq!(
                serde_json::to_value(preview).unwrap(),
                serde_json::json!({"version": "0.4.0", "notes": notes})
            );
        }
    }

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
