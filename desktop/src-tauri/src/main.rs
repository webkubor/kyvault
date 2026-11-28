#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;
mod commands;
mod export;
mod flash;
#[cfg(target_os = "macos")]
mod flash_socket;
mod repo_create;
mod share_png;
mod tray;
mod workspace;

use commands::{DesktopState, Session};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{tray::TrayIconBuilder, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    if std::env::args()
        .nth(1)
        .is_some_and(|arg| arg == "chrome-extension://fkgfhoglncnijiadgohpgnjllphjcibg/")
    {
        flash::native::browser();
        return;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            // Agent 通过 `ky request declare --open` 传 `--request <id>` 唤起已运行的
            // Ky.app。这里把请求 id 转给前端弹出预填的录入框。
            if let Some(id) = args
                .iter()
                .position(|a| a == "--request")
                .and_then(|i| args.get(i + 1))
            {
                let _ = app.emit("request-credential", id.to_string());
            }
            show(app)
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let data_root = std::env::var_os("KY_DESKTOP_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(app.path().app_local_data_dir()?);
            std::fs::create_dir_all(&data_root).ok();
            let prefs = commands::load_prefs_for_root(&data_root);
            app.manage(DesktopState {
                history: kyvault::desktop::DesktopHistory::new(data_root.clone()),
                session: Mutex::new(Session::resumed(prefs.monitoring)),
                data_root,
            });

            #[cfg(target_os = "macos")]
            {
                flash::native::setup(app.handle());
                if app
                    .global_shortcut()
                    .on_shortcut("Command+Option+N", |app, _, event| {
                        if event.state() == ShortcutState::Pressed {
                            flash::native::open(app);
                        }
                    })
                    .is_err()
                {
                    eprintln!("Ky: quick note shortcut unavailable");
                }
            }
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("Ky · 本地工作台")
                .on_menu_event(|app, event| tray::on_menu_event(app, event.id.as_ref()));
            #[cfg(target_os = "macos")]
            {
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
                tray = tray
                    .show_menu_on_left_click(false)
                    .on_tray_icon_event(|tray, event| {
                        if matches!(
                            event,
                            TrayIconEvent::Click {
                                button: MouseButton::Left,
                                button_state: MouseButtonState::Up,
                                ..
                            }
                        ) {
                            flash::native::open(tray.app_handle());
                        }
                    });
            }
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            let tray_icon = tray.build(app)?;
            app.manage(Mutex::new(tray_icon));
            tray::refresh(app.handle()).ok();

            // A conflicting global shortcut must not prevent users opening their vault.
            if app
                .global_shortcut()
                .on_shortcut("CommandOrControl+Shift+Space", |app, _, event| {
                    if event.state() == ShortcutState::Pressed {
                        show(app);
                    }
                })
                .is_err()
            {
                eprintln!("Ky: global shortcut unavailable; use the tray or Dock.");
            }

            // Present the window explicitly: a hidden-title transparent window
            // is not always shown by the system on first launch.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            // 冷启动也会带 --request <id>（第一次启动 Ky.app 时）。
            let args: Vec<String> = std::env::args().collect();
            if let Some(id) = args
                .iter()
                .position(|a| a == "--request")
                .and_then(|i| args.get(i + 1))
            {
                let handle = app.handle().clone();
                let id = id.to_string();
                // 前端模块加载需要一点时间；延迟发射避免事件被吞。
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(800));
                    let _ = handle.emit("request-credential", id);
                });
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut clipboard = arboard::Clipboard::new().ok();
                let mut previous_change = clipboard_change();
                let mut previous_fingerprint = None;
                let mut previous_tray_state = None;
                loop {
                    std::thread::park_timeout(Duration::from_secs(1));
                    let state = handle.state::<DesktopState>();
                    // Menus reflect both manual and automatic history changes. Never
                    // hold the session mutex while building a native menu.
                    let stamp = std::fs::metadata(state.data_root.join("history.enc"))
                        .ok()
                        .and_then(|m| m.modified().ok());
                    let tray_state = match state.session.lock() {
                        Ok(session) => (
                            stamp,
                            session.locked,
                            session.monitoring,
                            commands::load_prefs(&state).lang,
                        ),
                        Err(_) => continue,
                    };
                    if previous_tray_state.as_ref() != Some(&tray_state) {
                        let _ = tray::refresh(&handle);
                        previous_tray_state = Some(tray_state);
                    }
                    let Ok(mut session) = state.session.lock() else {
                        continue;
                    };
                    if !session.locked
                        && session.last_activity.elapsed() >= Duration::from_secs(300)
                    {
                        // The idle lock hides content only. Clipboard history
                        // stays on: the capture loop skips while locked, and
                        // the user's choice survives in prefs.json.
                        session.locked = true;
                        drop(session);
                        #[cfg(target_os = "macos")]
                        flash::native::lock(&handle);
                        let _ = handle.emit("app-locked", ());
                        let _ = tray::refresh(&handle);
                        continue;
                    }
                    if let Some((deadline, fingerprint)) = session.clear_at {
                        if Instant::now() >= deadline {
                            if clipboard.is_none() {
                                clipboard = arboard::Clipboard::new().ok();
                            }
                            if let Some(cb) = clipboard.as_mut() {
                                if let Ok(text) = cb.get_text() {
                                    if commands::fingerprint(&text) != fingerprint
                                        || cb.set_text(String::new()).is_ok()
                                    {
                                        session.clear_at = None;
                                    }
                                }
                            }
                        }
                    }
                    let change = clipboard_change();
                    let changed = change.is_none() || change != previous_change;
                    previous_change = change;
                    if !session.monitoring
                        || session.locked
                        || !changed
                        || share_png::clipboard_is_share()
                    {
                        continue;
                    }
                    if clipboard.is_none() {
                        clipboard = arboard::Clipboard::new().ok();
                    }
                    if let Some(cb) = clipboard.as_mut() {
                        // 文本优先；剪贴板里是图片时 get_text 为空/报错 → 尝试图片。
                        let text = cb.get_text().unwrap_or_default();
                        if !text.is_empty() && text.len() <= 65_536 && !session.is_sensitive(&text)
                        {
                            let fingerprint = commands::fingerprint(&text);
                            if previous_fingerprint == Some(fingerprint) {
                                continue;
                            }
                            previous_fingerprint = Some(fingerprint);
                            let source = frontmost_app();
                            match state.history.capture(&text, source.as_deref()) {
                                Ok(_) => {
                                    drop(session);
                                    #[cfg(target_os = "macos")]
                                    if let Some(generation) = change {
                                        flash::automatic(&state, generation, &text);
                                    }
                                    let _ = handle.emit("history-changed", ());
                                    let _ = tray::refresh(&handle);
                                    continue;
                                }
                                Err(_) => {
                                    session.monitoring = false;
                                    let _ = handle.emit(
                                        "capture-error",
                                        "历史保存失败，已暂停记录，请检查本地目录权限。",
                                    );
                                }
                            }
                        } else if text.is_empty() {
                            // 图片抓取：RGBA → PNG → 加密存储。超限/编码失败静默跳过。
                            if let Ok(img) = cb.get_image() {
                                let fp: [u8; 32] = {
                                    use sha2::{Digest, Sha256};
                                    let mut h = Sha256::new();
                                    h.update(b"img");
                                    h.update(img.width.to_le_bytes());
                                    h.update(img.height.to_le_bytes());
                                    h.update(&img.bytes);
                                    h.finalize().into()
                                };
                                if previous_fingerprint == Some(fp) {
                                    continue;
                                }
                                if let Some(png) =
                                    commands::encode_rgba_png_pub(img.width, img.height, &img.bytes)
                                {
                                    previous_fingerprint = Some(fp);
                                    let source = frontmost_app();
                                    let (w, h) = (img.width as u32, img.height as u32);
                                    if state
                                        .history
                                        .capture_image(&png, w, h, source.as_deref())
                                        .is_ok()
                                    {
                                        drop(session);
                                        let _ = handle.emit("history-changed", ());
                                        let _ = tray::refresh(&handle);
                                    }
                                }
                            }
                        }
                    }
                }
            });
            kyvault::analytics::app_open();
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            flash::flash_browser_setup,
            flash::flash_sound,
            flash::flash_choose_app,
            flash::flash_apps,
            flash::flash_app_rule,
            flash::flash_history,
            account::account_status,
            account::account_login,
            repo_create::account_create,
            account::account_bind,
            account::account_sync,
            account::account_unbind,
            workspace::workspace_list,
            workspace::workspace_read,
            workspace::workspace_save,
            workspace::workspace_delete,
            export::export_workspace_file,
            share_png::copy_share_png,
            share_png::export_share_png,
            commands::touch_activity,
            commands::snapshot,
            commands::capture_clipboard,
            commands::set_monitoring,
            commands::preview_text,
            commands::save_text,
            commands::delete_entry,
            commands::restore_entry,
            commands::copy_entry,
            commands::copy_reference,
            commands::show_secret,
            commands::copy_secret,
            commands::save_secret,
            commands::save_secret_with_meta,
            commands::delete_secret,
            commands::pending_requests,
            commands::fulfill_request,
            commands::discard_request,
            commands::lock_app,
            commands::unlock_app,
            commands::clear_history,
            commands::set_ui_language,
            commands::check_update,
            commands::install_update,
            commands::get_entry_image
        ])
        .run(tauri::generate_context!())
        .expect("Ky desktop failed to start");
}

// NSPasteboard has a change counter, not a public clipboard-change notification.
// Check the cheap counter once a second; only read/decrypt when needed.
// 复制时的来源应用名（macOS 前台 app）。抓取是秒级轮询，拿一次开销可忽略；
// 拿不到就返回 None，前端不显示来源，不影响功能。
#[cfg(target_os = "macos")]
fn frontmost_app() -> Option<String> {
    use objc2_app_kit::NSWorkspace;
    let ws = NSWorkspace::sharedWorkspace();
    let app = ws.frontmostApplication()?;
    let name = app.localizedName()?;
    let s = name.to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(not(target_os = "macos"))]
fn frontmost_app() -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn clipboard_change() -> Option<isize> {
    Some(objc2_app_kit::NSPasteboard::generalPasteboard().changeCount())
}
#[cfg(not(target_os = "macos"))]
fn clipboard_change() -> Option<isize> {
    None
}
