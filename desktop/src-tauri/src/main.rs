#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod tray;

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
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state() == ShortcutState::Pressed {
                        show(app);
                    }
                })
                .build(),
        )
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

            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("Ky · 本地工作台")
                .on_menu_event(|app, event| tray::on_menu_event(app, event.id.as_ref()));
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            let tray_icon = tray.build(app)?;
            app.manage(Mutex::new(tray_icon));
            tray::refresh(app.handle()).ok();

            // A conflicting global shortcut must not prevent users opening their vault.
            if app
                .global_shortcut()
                .register("CommandOrControl+Shift+Space")
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
                        session.locked = true;
                        session.monitoring = false;
                        drop(session);
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
                    if !session.monitoring || session.locked || !changed {
                        continue;
                    }
                    if clipboard.is_none() {
                        clipboard = arboard::Clipboard::new().ok();
                    }
                    if let Some(cb) = clipboard.as_mut() {
                        if let Ok(text) = cb.get_text() {
                            if text.is_empty() || text.len() > 65_536 || session.is_sensitive(&text)
                            {
                                continue;
                            }
                            let fingerprint = commands::fingerprint(&text);
                            if previous_fingerprint == Some(fingerprint) {
                                continue;
                            }
                            previous_fingerprint = Some(fingerprint);
                            match state.history.capture(&text) {
                                Ok(_) => {
                                    drop(session);
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
                        }
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
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
            commands::copy_secret,
            commands::save_secret,
            commands::delete_secret,
            commands::lock_app,
            commands::unlock_app,
            commands::clear_history,
            commands::set_ui_language
        ])
        .run(tauri::generate_context!())
        .expect("Ky desktop failed to start");
}

// NSPasteboard has a change counter, not a public clipboard-change notification.
// Check the cheap counter once a second; only read/decrypt when needed.
#[cfg(target_os = "macos")]
fn clipboard_change() -> Option<isize> {
    Some(objc2_app_kit::NSPasteboard::generalPasteboard().changeCount())
}
#[cfg(not(target_os = "macos"))]
fn clipboard_change() -> Option<isize> {
    None
}
