//! Menu-bar (tray) menu with recent clipboard history and localized labels.
//!
//! Entries shown here are always the redacted texts persisted by the history
//! store, never raw secret values. When the workspace is locked the recent
//! section is hidden entirely.

use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIcon,
    AppHandle, Emitter, Manager,
};

use crate::commands::{self, DesktopState};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The most recent entries surfaced in the menu bar.
const MENU_RECENT_MAX: usize = 8;

/// Truncate and collapse whitespace into a single-line, menu-friendly label.
fn label_of(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.trim();
    let mut out = String::new();
    for ch in line.chars() {
        if ch.is_control() {
            out.push(' ');
        } else {
            out.push(ch);
        }
        if out.chars().count() >= 40 {
            out.push('…');
            break;
        }
    }
    if out.is_empty() {
        "(空白)".to_string()
    } else {
        out
    }
}

/// A planned menu row, resolved before any native object is created.
///
/// Keeping the decision pure makes the menu-bar content (recent history,
/// localization, lock behaviour) verifiable without a live UI session.
#[derive(Debug, PartialEq, Eq)]
pub enum PlanItem {
    Action { id: String, label: String },
    Separator,
    Header { label: String },
    Recent { id: String, label: String },
}

/// Build the full menu plan for the current session state.
pub fn plan(locked: bool, monitoring: bool, zh: bool, entries: &[(u64, String)]) -> Vec<PlanItem> {
    let mut items = vec![
        PlanItem::Action {
            id: "open".into(),
            label: if zh { "打开 Ky" } else { "Open Ky" }.into(),
        },
        PlanItem::Action {
            id: "toggle-monitor".into(),
            label: if monitoring {
                if zh {
                    "停止记录剪贴板"
                } else {
                    "Stop capturing clipboard"
                }
            } else if zh {
                "记录剪贴板"
            } else {
                "Capture clipboard"
            }
            .into(),
        },
    ];

    // A locked workspace must not leak any recent content through the menu.
    let recent: Vec<PlanItem> = if locked {
        Vec::new()
    } else {
        entries
            .iter()
            .take(MENU_RECENT_MAX)
            .map(|(id, text)| PlanItem::Recent {
                id: format!("recent:{id}"),
                label: label_of(text),
            })
            .collect()
    };

    if !recent.is_empty() {
        items.push(PlanItem::Separator);
        items.push(PlanItem::Header {
            label: if zh { "最近复制" } else { "Recent" }.into(),
        });
        items.extend(recent);
    }

    items.push(PlanItem::Separator);
    items.push(PlanItem::Action {
        id: "lock".into(),
        label: if zh {
            "隐藏敏感内容"
        } else {
            "Hide sensitive content"
        }
        .into(),
    });
    items.push(PlanItem::Action {
        id: "quit".into(),
        label: if zh { "退出 Ky" } else { "Quit Ky" }.into(),
    });
    items
}

fn copy_text(app: &AppHandle, text: &str) {
    let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string()));
    // Copying a redacted history entry is public-facing local text, never a
    // secret, so no auto-clear timer is attached here.
    let _ = app;
}

fn prefs(app: &AppHandle) -> commands::Prefs {
    let state = app.state::<DesktopState>();
    commands::load_prefs(&state)
}

/// Rebuild the menu bar menu: brand header → recent history → actions.
///
/// Native menu objects must be created and swapped on the main thread; the
/// clipboard worker thread therefore asks the event loop to do it.
pub fn refresh(app: &AppHandle) -> Result<()> {
    let handle = app.clone();
    let _ = handle.clone().run_on_main_thread(move || {
        if let Err(err) = refresh_now(&handle) {
            eprintln!("Ky: menu refresh failed: {err}");
        }
    });
    Ok(())
}

fn refresh_now(app: &AppHandle) -> Result<()> {
    let state = app.state::<DesktopState>();
    let prefs = commands::load_prefs(&state);
    let zh = prefs.lang == "zh-CN";
    let (locked, monitoring) = state
        .session
        .lock()
        .map(|s| (s.locked, s.monitoring))
        .unwrap_or((true, false));

    let entries: Vec<(u64, String)> = if locked {
        Vec::new()
    } else {
        state
            .history
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|e| (e.id, e.text))
            .collect()
    };
    let plan = plan(locked, monitoring, zh, &entries);

    // Owned bindings so the borrowed item slice stays valid for `with_items`.
    let separator_a = PredefinedMenuItem::separator(app)?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let mut owned: Vec<MenuItem<tauri::Wry>> = Vec::new();
    // Each plan row maps to either an owned item or one of the separators.
    let mut order: Vec<(bool, usize)> = Vec::new();
    let mut separators = 0usize;
    for entry in &plan {
        match entry {
            PlanItem::Action { id, label } | PlanItem::Recent { id, label } => {
                let item = MenuItem::with_id(app, id.as_str(), label.as_str(), true, None::<&str>)?;
                owned.push(item);
                order.push((true, owned.len() - 1));
            }
            PlanItem::Header { label } => {
                let item =
                    MenuItem::with_id(app, "__header__", label.as_str(), false, None::<&str>)?;
                owned.push(item);
                order.push((true, owned.len() - 1));
            }
            PlanItem::Separator => {
                separators += 1;
                order.push((false, separators - 1));
            }
        }
    }

    let mut items: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = Vec::new();
    for (is_item, index) in order {
        if is_item {
            items.push(&owned[index]);
        } else if index == 0 {
            items.push(&separator_a);
        } else {
            items.push(&separator_b);
        }
    }

    let menu = Menu::with_items(app, &items)?;
    let tray: tauri::State<'_, Mutex<TrayIcon>> = app.state();
    if let Ok(tray) = tray.lock() {
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

/// Handle a tray menu click without plumbing into the main closure.
pub fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        "open" => crate::show(app),
        "toggle-monitor" => {
            let state = app.state::<DesktopState>();
            let enabled = if let Ok(mut s) = state.session.lock() {
                if s.locked {
                    return;
                }
                s.monitoring = !s.monitoring;
                s.monitoring
            } else {
                return;
            };
            let _ = commands::save_prefs(
                &state,
                &commands::Prefs {
                    lang: prefs(app).lang,
                    monitoring: enabled,
                },
            );
            let _ = refresh(app);
            let _ = app.emit("tray-refresh", ());
        }
        "lock" => {
            let _ = commands::lock(app);
            let _ = refresh(app);
        }
        "quit" => app.exit(0),
        _ => {
            if let Some(rest) = id.strip_prefix("recent:") {
                let text = {
                    let state = app.state::<DesktopState>();
                    match state.session.lock() {
                        Ok(session) if !session.locked => {}
                        _ => return,
                    }
                    let id: u64 = rest.parse().unwrap_or(0);
                    state
                        .history
                        .list()
                        .unwrap_or_default()
                        .into_iter()
                        .find(|e| e.id == id)
                        .map(|e| e.text)
                };
                if let Some(text) = text {
                    copy_text(app, &text);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{label_of, plan, PlanItem};

    #[test]
    fn label_uses_the_first_non_empty_line() {
        assert_eq!(label_of("\n\n  hello world  \nsecond"), "hello world");
    }

    #[test]
    fn label_replaces_control_characters() {
        // Only the first non-empty line is shown, so the trailing line is dropped.
        assert_eq!(label_of("a\tb\nc"), "a b");
    }

    #[test]
    fn label_truncates_long_text() {
        let label = label_of(&"x".repeat(80));
        assert_eq!(
            label.chars().count(),
            41,
            "long labels are cut with an ellipsis"
        );
        assert!(label.ends_with('…'));
    }

    #[test]
    fn label_marks_blank_entries() {
        assert_eq!(label_of("   \n\t"), "(空白)");
    }

    #[test]
    fn plan_lists_recent_history_in_chinese() {
        let items = plan(false, true, true, &[(7, "菜单栏测试".to_string())]);
        assert_eq!(
            items,
            vec![
                PlanItem::Action {
                    id: "open".into(),
                    label: "打开 Ky".into()
                },
                PlanItem::Action {
                    id: "toggle-monitor".into(),
                    label: "停止记录剪贴板".into()
                },
                PlanItem::Separator,
                PlanItem::Header {
                    label: "最近复制".into()
                },
                PlanItem::Recent {
                    id: "recent:7".into(),
                    label: "菜单栏测试".into()
                },
                PlanItem::Separator,
                PlanItem::Action {
                    id: "lock".into(),
                    label: "隐藏敏感内容".into()
                },
                PlanItem::Action {
                    id: "quit".into(),
                    label: "退出 Ky".into()
                },
            ]
        );
    }

    #[test]
    fn plan_localizes_to_english() {
        let items = plan(false, false, false, &[(1, "hello".to_string())]);
        let labels: Vec<&str> = items
            .iter()
            .filter_map(|i| match i {
                PlanItem::Action { label, .. } | PlanItem::Header { label } => Some(label.as_str()),
                PlanItem::Recent { label, .. } => Some(label.as_str()),
                PlanItem::Separator => None,
            })
            .collect();
        assert_eq!(
            labels,
            vec![
                "Open Ky",
                "Capture clipboard",
                "Recent",
                "hello",
                "Hide sensitive content",
                "Quit Ky"
            ]
        );
    }

    #[test]
    fn plan_hides_recent_history_when_locked() {
        let items = plan(true, true, true, &[(3, "secret-ish".to_string())]);
        assert!(
            !items
                .iter()
                .any(|i| matches!(i, PlanItem::Recent { .. } | PlanItem::Header { .. })),
            "a locked workspace must not surface recent entries"
        );
    }

    #[test]
    fn plan_caps_recent_entries() {
        let entries: Vec<(u64, String)> = (0..20).map(|i| (i, format!("item {i}"))).collect();
        let items = plan(false, true, true, &entries);
        let recent = items
            .iter()
            .filter(|i| matches!(i, PlanItem::Recent { .. }))
            .count();
        assert_eq!(recent, super::MENU_RECENT_MAX);
    }
}
