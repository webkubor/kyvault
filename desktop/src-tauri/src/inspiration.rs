//! Explicit idea capture and opening paths. No arbitrary paths from the UI.
use base64::{engine::general_purpose::STANDARD, Engine};
use kyvault::inspiration::{Idea, InspirationStore};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, State};

static LAST_CAPTURE: Mutex<Option<([u8; 32], Instant)>> = Mutex::new(None);
use tauri_plugin_opener::OpenerExt;

use crate::commands::{active, encode_rgba_png_pub, DesktopState};

type Result<T> = std::result::Result<T, String>;

fn store() -> Result<InspirationStore> {
    InspirationStore::default_location().map_err(|e| e.to_string())
}

pub fn capture(state: &DesktopState) -> Result<Idea> {
    let session = active(state)?;
    let mut clipboard = arboard::Clipboard::new().map_err(|_| "剪贴板不可用")?;
    let text = clipboard.get_text().unwrap_or_default();
    let source = crate::frontmost_app();
    if !text.trim().is_empty() {
        if session.is_sensitive(&text) {
            return Err("这是从密钥库复制的敏感值，不能保存为明文灵感".into());
        }
        if text.len() > 65_536 {
            return Err("文本不能超过 64 KiB".into());
        }
        let title: String = text
            .lines()
            .find(|s| !s.trim().is_empty())
            .unwrap_or("")
            .trim()
            .chars()
            .take(60)
            .collect();
        return save_once(text.as_bytes(), || {
            store()?
                .add_with_source(&title, &text, vec![], source.as_deref())
                .map_err(|e| e.to_string())
        });
    }
    let image = clipboard.get_image().map_err(|_| "剪贴板没有文本或图片")?;
    let png = encode_rgba_png_pub(image.width, image.height, &image.bytes)
        .ok_or("无法保存图片，请检查图片大小")?;
    save_once(&png, || {
        store()?
            .capture_image(&png, "剪贴板图片", source.as_deref())
            .map_err(|e| e.to_string())
    })
}

fn save_once(bytes: &[u8], save: impl FnOnce() -> Result<Idea>) -> Result<Idea> {
    let hash: [u8; 32] = Sha256::digest(bytes).into();
    let mut last = LAST_CAPTURE.lock().map_err(|_| "收录暂不可用")?;
    if last
        .as_ref()
        .is_some_and(|(prev, at)| *prev == hash && at.elapsed() < Duration::from_secs(3))
    {
        return Err("刚刚已收录相同内容".into());
    }
    let idea = save()?;
    *last = Some((hash, Instant::now()));
    Ok(idea)
}

#[tauri::command]
pub fn capture_idea_clipboard(state: State<DesktopState>) -> Result<Idea> {
    capture(&state)
}

#[tauri::command]
pub fn save_idea_image(data: String, state: State<DesktopState>) -> Result<Idea> {
    let _session = active(&state)?;
    if data.len() > 12 * 1024 * 1024 {
        return Err("图片不能超过 8 MiB".into());
    }
    let bytes = STANDARD.decode(data).map_err(|_| "图片数据无效")?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("图片不能超过 8 MiB".into());
    }
    let image = image::load_from_memory(&bytes)
        .map_err(|_| "无法读取图片")?
        .to_rgba8();
    let png = encode_rgba_png_pub(image.width() as usize, image.height() as usize, &image)
        .ok_or("无法保存图片，请检查图片大小")?;
    store()?
        .capture_image(&png, "图片灵感", None)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_idea_file(id: String, app: AppHandle, state: State<DesktopState>) -> Result<()> {
    let _session = active(&state)?;
    let path = store()?.file_path(&id).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_ideas_folder(app: AppHandle, state: State<DesktopState>) -> Result<()> {
    let _session = active(&state)?;
    let store = store()?;
    std::fs::create_dir_all(store.dir()).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(store.dir().to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}
