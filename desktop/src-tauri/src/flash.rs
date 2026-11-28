//! Native capture entries use the same encrypted collection as private notes.
use crate::commands::{active, DesktopState};
use std::collections::BTreeSet;
use tauri::State;

fn rules(state: &DesktopState) -> Result<BTreeSet<String>, String> {
    match std::fs::read(state.data_root.join("flash-apps.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "无法读取闪记来源".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(_) => Err("无法读取闪记来源".into()),
    }
}

#[tauri::command]
pub fn flash_apps(state: State<DesktopState>) -> Result<BTreeSet<String>, String> {
    let _session = active(&state)?;
    rules(&state)
}

#[tauri::command]
pub fn flash_app_rule(
    state: State<DesktopState>,
    app: String,
    enabled: bool,
) -> Result<BTreeSet<String>, String> {
    let _session = active(&state)?;
    if app.len() > 250
        || !app.contains('.')
        || !app
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'))
    {
        return Err("请输入应用的 bundle ID".into());
    }
    let mut set = rules(&state)?;
    if enabled {
        set.insert(app);
    } else {
        set.remove(&app);
    }
    let mut file =
        tempfile::NamedTempFile::new_in(&state.data_root).map_err(|_| "无法保存闪记来源")?;
    serde_json::to_writer(&mut file, &set).map_err(|_| "无法保存闪记来源")?;
    file.persist(state.data_root.join("flash-apps.json"))
        .map_err(|_| "无法保存闪记来源")?;
    Ok(set)
}

#[tauri::command]
pub fn flash_sound(enabled: Option<bool>) -> bool {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn ky_flash_sound(update: bool, enabled: bool) -> bool;
        }
        unsafe { ky_flash_sound(enabled.is_some(), enabled.unwrap_or(false)) }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = enabled;
        false
    }
}

#[cfg(target_os = "macos")]
fn received(state: &DesktopState, overlay: bool, audible: bool) {
    extern "C" {
        fn ky_flash_received(overlay: bool, audible: bool, english: bool);
    }
    unsafe {
        ky_flash_received(
            overlay,
            audible,
            crate::commands::load_prefs(state).lang == "en",
        );
    }
}

#[cfg(target_os = "macos")]
pub fn automatic(state: &DesktopState, change: isize, text: &str) {
    use std::ffi::CStr;
    extern "C" {
        fn ky_flash_source(change: isize) -> *const std::ffi::c_char;
    }
    let ptr = unsafe { ky_flash_source(change) };
    if ptr.is_null() {
        return;
    }
    let Ok(source) = unsafe { CStr::from_ptr(ptr) }.to_str() else {
        return;
    };
    if rules(state).is_ok_and(|list| list.contains(source))
        && crate::workspace::save_flash(state, text, Some(source), true).is_ok()
    {
        received(state, true, false);
    }
}

#[tauri::command(async)]
pub fn flash_browser_setup(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let dir = app
        .path()
        .resource_dir()
        .map_err(|_| "无法找到浏览器扩展")?
        .join("flash-browser");
    let status = std::process::Command::new("/bin/sh")
        .arg(dir.join("install-host.sh"))
        .output()
        .map_err(|_| "连接浏览器失败")?;
    if !status.status.success() {
        return Err("请先将 Ky 安装到应用程序目录".into());
    }
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(dir.join("extension"))
        .status()
        .map_err(|_| "无法打开扩展目录")?;
    Ok(())
}

#[tauri::command]
pub fn flash_choose_app() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn ky_flash_choose_app() -> *const std::ffi::c_char;
        }
        let ptr = unsafe { ky_flash_choose_app() };
        if !ptr.is_null() {
            return unsafe { std::ffi::CStr::from_ptr(ptr) }
                .to_str()
                .ok()
                .map(str::to_owned);
        }
    }
    None
}

#[tauri::command]
pub fn flash_history(state: State<DesktopState>, id: u64) -> Result<(), String> {
    let entry = {
        let _session = active(&state)?;
        state
            .history
            .list()
            .map_err(|_| "无法读取剪贴板历史")?
            .into_iter()
            .find(|e| e.id == id && e.image.is_none() && !e.redacted)
            .ok_or("无法收藏此条目")?
    };
    crate::workspace::save_flash(&state, &entry.text, entry.source.as_deref(), false)?;
    #[cfg(target_os = "macos")]
    received(&state, false, true);
    Ok(())
}

#[cfg(target_os = "macos")]
pub mod native {
    use crate::commands::DesktopState;
    use std::{
        ffi::{c_char, CStr},
        io::{Read, Write},
        sync::OnceLock,
    };
    use tauri::{AppHandle, Manager};
    static APP: OnceLock<AppHandle> = OnceLock::new();
    extern "C" {
        fn ky_flash_setup(
            save: extern "C" fn(*const c_char, *const c_char) -> bool,
            activity: extern "C" fn(),
        );
        fn ky_flash_open(item: *const std::ffi::c_void, english: bool, locked: bool);
        fn ky_flash_lock();

    }
    extern "C" fn save(value: *const c_char, image: *const c_char) -> bool {
        let Some(app) = APP.get() else { return false };
        if value.is_null() || image.is_null() {
            return false;
        }
        let Ok(text) = unsafe { CStr::from_ptr(value) }.to_str() else {
            return false;
        };
        {
            let state = app.state::<DesktopState>();
            let Ok(image) = unsafe { CStr::from_ptr(image) }.to_str() else {
                return false;
            };
            let saved =
                crate::workspace::save_flash_content(&state, text, None, false, image.into())
                    .is_ok();
            if saved {
                super::received(&state, true, true);
            }
            saved
        }
    }
    extern "C" fn activity() {
        if let Some(app) = APP.get() {
            drop(crate::commands::active(&app.state::<DesktopState>()));
        }
    }
    pub fn setup(app: &AppHandle) {
        let _ = APP.set(app.clone());
        let handle = app.clone();
        if crate::flash_socket::serve(&app.state::<DesktopState>().data_root, move |text| {
            let state = handle.state::<DesktopState>();
            let saved = crate::workspace::save_flash(&state, text, None, false).is_ok();
            if saved {
                super::received(&state, true, true);
            }
            saved
        })
        .is_err()
        {
            eprintln!("Ky: browser quick notes unavailable");
        }
        unsafe { ky_flash_setup(save, activity) }
    }
    pub fn open(app: &AppHandle) {
        let Some(tray) = app.tray_by_id("main") else {
            return;
        };
        let handle = app.clone();
        let _ = tray.with_inner_tray_icon(move |inner| {
            let state = handle.state::<DesktopState>();
            let locked = crate::commands::active(&state).is_err();
            let english = crate::commands::load_prefs(&state).lang == "en";
            let item = inner.ns_status_item();
            let ptr = item
                .as_deref()
                .map_or(std::ptr::null(), |item| std::ptr::from_ref(item).cast());
            unsafe { ky_flash_open(ptr, english, locked) };
        });
    }
    pub fn lock(app: &AppHandle) {
        let _ = app.run_on_main_thread(|| unsafe { ky_flash_lock() });
    }
    // Chrome native messaging: one bounded frame per sendNativeMessage.
    pub fn browser() {
        let ok = (|| -> Option<bool> {
            let mut input = std::io::stdin().lock();
            let mut size = [0; 4];
            input.read_exact(&mut size).ok()?;
            let len = u32::from_ne_bytes(size) as usize;
            if len > 512 * 1024 {
                return None;
            }
            let mut bytes = vec![0; len];
            input.read_exact(&mut bytes).ok()?;
            #[derive(serde::Deserialize)]
            struct Capture {
                text: String,
                url: String,
            }
            let capture: Capture = serde_json::from_slice(&bytes).ok()?;
            if capture.text.trim().is_empty() || capture.text.len() > 65536 {
                return None;
            }
            let mut url = tauri::Url::parse(&capture.url).ok()?;
            if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
                return None;
            }
            url.set_query(None);
            url.set_fragment(None);
            url.set_username("").ok()?;
            url.set_password(None).ok()?;
            let root = std::env::var_os("KY_DESKTOP_DATA_DIR")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|h| {
                        std::path::PathBuf::from(h)
                            .join("Library/Application Support/com.kyvault.desktop")
                    })
                })?;
            let body = format!("{}\n\n{}", capture.text, url);
            crate::flash_socket::send(&root, &body).ok()
        })()
        .unwrap_or(false);
        let response = if ok {
            r#"{"ok":true}"#
        } else {
            r#"{"ok":false}"#
        };
        let mut out = std::io::stdout().lock();
        let _ = out
            .write_all(&(response.len() as u32).to_ne_bytes())
            .and_then(|_| out.write_all(response.as_bytes()));
    }
}
