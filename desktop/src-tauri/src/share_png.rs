//! The WebView supplies a data URL and optionally a suggested basename.
//! Only the native save dialog supplies a destination.
use crate::commands::{active, DesktopState};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use std::io::Cursor;
use tauri::State;

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_SIDE: u32 = 8192;
const MAX_PIXELS: u64 = 16_000_000;
type Result<T> = std::result::Result<T, String>;

fn png_bytes(data: &str) -> Result<Vec<u8>> {
    let encoded = data
        .strip_prefix("data:image/png;base64,")
        .ok_or("分享卡片须为 PNG")?;
    if encoded.len() > MAX_BYTES.div_ceil(3) * 4 {
        return Err("PNG 超过 8 MiB".into());
    }
    let bytes = STANDARD.decode(encoded).map_err(|_| "PNG 编码无效")?;
    if bytes.len() > MAX_BYTES || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("PNG 格式或大小无效".into());
    }
    // Some decoders stop after the last row. Reject incomplete containers,
    // animation and any content after the actual end chunk before export.
    let mut chunks = &bytes[8..];
    loop {
        let prefix = chunks.get(..8).ok_or("PNG 不完整")?;
        let size = u32::from_be_bytes(prefix[..4].try_into().unwrap()) as usize;
        let total = size.checked_add(12).ok_or("PNG 块无效")?;
        let end_crc_ok = chunks.get(8..12) == Some(&[0xae, 0x42, 0x60, 0x82]);
        chunks = chunks.get(total..).ok_or("PNG 块无效")?;
        if &prefix[4..] == b"acTL" {
            return Err("分享卡片须为静态 PNG".into());
        }
        if &prefix[4..] == b"IEND" {
            if size != 0 || !end_crc_ok || !chunks.is_empty() {
                return Err("PNG 尾部无效".into());
            }
            break;
        }
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(128 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|_| "PNG 损坏或尺寸过大")?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("PNG 尺寸过大".into());
    }
    image::DynamicImage::from_decoder(decoder).map_err(|_| "PNG 像素数据损坏")?;
    Ok(bytes)
}

fn valid_name(name: &str) -> bool {
    !name.starts_with('.')
        && name.ends_with(".png")
        && name.len() <= 240
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2_app_kit::NSPasteboard;
    use std::{
        ffi::{c_char, c_void, CStr, CString},
        os::unix::ffi::OsStrExt,
        path::{Path, PathBuf},
    };
    extern "C" {
        fn ky_write_share_png(board: *const c_void, bytes: *const u8, len: usize) -> bool;
        fn ky_board_has_share_marker(board: *const c_void) -> bool;
        fn ky_choose_export(name: *const c_char) -> *mut c_char;
        fn ky_release_path(path: *mut c_char);
    }

    pub(super) fn write(board: &NSPasteboard, bytes: &[u8]) -> Result<()> {
        let ok = unsafe {
            ky_write_share_png(
                (board as *const NSPasteboard).cast(),
                bytes.as_ptr(),
                bytes.len(),
            )
        };
        if ok {
            Ok(())
        } else {
            Err("无法复制分享卡片".into())
        }
    }

    pub(super) fn marked(board: &NSPasteboard) -> bool {
        unsafe { ky_board_has_share_marker((board as *const NSPasteboard).cast()) }
    }

    pub(super) fn select(app: &tauri::AppHandle, name: &str) -> Result<Option<PathBuf>> {
        let name = CString::new(name).map_err(|_| "文件名无效")?;
        let (send, recv) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let ptr = unsafe { ky_choose_export(name.as_ptr()) };
            let path = if ptr.is_null() {
                None
            } else {
                let bytes = unsafe { CStr::from_ptr(ptr) }.to_bytes();
                let path = Path::new(std::ffi::OsStr::from_bytes(bytes)).to_owned();
                unsafe { ky_release_path(ptr) };
                Some(path)
            };
            let _ = send.send(path);
        })
        .map_err(|_| "无法打开保存对话框")?;
        recv.recv().map_err(|_| "保存对话框不可用".into())
    }
}

pub(super) fn clipboard_is_share() -> bool {
    #[cfg(target_os = "macos")]
    {
        mac::marked(&objc2_app_kit::NSPasteboard::generalPasteboard())
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

fn copy(state: &DesktopState, data: &str, write: impl FnOnce(&[u8]) -> Result<()>) -> Result<()> {
    drop(active(state)?);
    let bytes = png_bytes(data)?;
    let _session = active(state)?;
    write(&bytes)
}

#[tauri::command(async)]
pub fn copy_share_png(state: State<'_, DesktopState>, data: String) -> Result<()> {
    copy(&state, &data, |bytes| {
        #[cfg(target_os = "macos")]
        {
            mac::write(&objc2_app_kit::NSPasteboard::generalPasteboard(), bytes)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = bytes;
            Err("此平台暂不支持复制分享卡片".into())
        }
    })
}

fn export(
    state: &DesktopState,
    data: &str,
    name: &str,
    select: impl FnOnce(&str) -> Result<Option<std::path::PathBuf>>,
) -> Result<bool> {
    drop(active(state)?);
    if !valid_name(name) {
        return Err("PNG 文件名无效".into());
    }
    let bytes = png_bytes(data)?;
    drop(active(state)?);
    let Some(target) = select(name)? else {
        return Ok(false);
    };
    let _session = active(state)?;
    if !target.is_absolute() || target.extension() != Some(std::ffi::OsStr::new("png")) {
        return Err("请选择 .png 文件".into());
    }
    let mut tmp = tempfile::NamedTempFile::new_in(target.parent().ok_or("保存位置无效")?)
        .map_err(|_| "无法创建文件")?;
    std::io::Write::write_all(&mut tmp, &bytes).map_err(|_| "写入 PNG 失败")?;
    tmp.as_file().sync_all().map_err(|_| "保存 PNG 失败")?;
    tmp.persist(target).map_err(|_| "保存 PNG 失败")?;
    Ok(true)
}

#[tauri::command(async)]
pub fn export_share_png(
    state: State<'_, DesktopState>,
    app: tauri::AppHandle,
    data: String,
    name: String,
) -> Result<bool> {
    export(&state, &data, &name, |name| {
        #[cfg(target_os = "macos")]
        {
            mac::select(&app, name)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, name);
            Err("此平台暂不支持导出分享卡片".into())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Session;
    use kyvault::desktop::DesktopHistory;
    use std::{fs, path::Path, sync::Mutex};

    fn state(root: &Path) -> DesktopState {
        DesktopState {
            history: DesktopHistory::new(root),
            session: Mutex::new(Session::default()),
            data_root: root.into(),
        }
    }

    fn fixture() -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 3)
            .write_to(&mut buf, ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    fn url(bytes: &[u8]) -> String {
        format!("data:image/png;base64,{}", STANDARD.encode(bytes))
    }

    #[test]
    fn only_accepts_complete_bounded_static_png() {
        let bytes = fixture();
        assert_eq!(png_bytes(&url(&bytes)).unwrap(), bytes);
        let mut appended = bytes.clone();
        appended.push(0);
        let mut bad_pixels = bytes.clone();
        let pos = bad_pixels.windows(4).position(|w| w == b"IDAT").unwrap();
        bad_pixels[pos + 4] ^= 255;
        for data in [
            "data:image/jpeg;base64,aA==".into(),url(b"RIFFbad"),
            "data:image/png;base64,!".into(),url(&bytes[..bytes.len()-12]),
            url(&bad_pixels),url(&appended),
            format!("data:image/png;base64,{}", "A".repeat(MAX_BYTES.div_ceil(3)*4+1)),
            url(&STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAIAEAAAABCAIAAAAW69wJAAAADElEQVR4nGNgYGAAAAAEAAH2FzhVAAAAAElFTkSuQmCC").unwrap()),
            url(&STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAE4gAAA+gCAIAAACZe2F9AAAADElEQVR4nGNgYGAAAAAEAAH2FzhVAAAAAElFTkSuQmCC").unwrap()),
        ] { assert!(png_bytes(&data).is_err()); }
    }

    #[test]
    fn destination_comes_only_from_native_picker_and_lock_is_rechecked() {
        let root = tempfile::tempdir().unwrap();
        let state = state(root.path());
        let bytes = fixture();
        let data = url(&bytes);
        for bad_name in [
            "../card.png",
            "/tmp/card.png",
            "D:\\card.png",
            "card:other.png",
            ".png",
            "a\nb.png",
            "card.jpg",
        ] {
            assert!(export(&state, &data, bad_name, |_| panic!(
                "unsafe name reached picker"
            ))
            .is_err());
        }
        assert!(!export(&state, &data, "card.png", |_| Ok(None)).unwrap());
        let target = root.path().join("chosen.png");
        fs::write(&target, b"previous").unwrap();
        assert!(export(&state, &data, "card.png", |_| {
            state.session.lock().unwrap().locked = true;
            Ok(Some(target.clone()))
        })
        .is_err());
        assert_eq!(fs::read(&target).unwrap(), b"previous");
        assert!(copy(&state, &data, |_| panic!("locked copy")).is_err());
        assert!(export(&state, &data, "card.png", |_| panic!("locked export")).is_err());
        state.session.lock().unwrap().locked = false;
        assert!(export(&state, &data, "card.png", |_| Ok(Some(
            root.path().join("no.txt")
        )))
        .is_err());
        assert!(export(&state, &data, "card.png", |_| Ok(Some(target.clone()))).unwrap());
        assert_eq!(fs::read(target).unwrap(), bytes);
        assert!(!root.path().join("card.png").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unique_native_pasteboard_carries_public_png_and_capture_exclusion() {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
        let root = tempfile::tempdir().unwrap();
        let board = NSPasteboard::pasteboardWithUniqueName();
        let bytes = fixture();
        assert!(!mac::marked(&board));
        copy(&state(root.path()), &url(&bytes), |png| {
            mac::write(&board, png)
        })
        .unwrap();
        assert!(mac::marked(&board));
        let data = board.dataForType(unsafe { NSPasteboardTypePNG }).unwrap();
        assert_eq!(data.to_vec(), bytes);
        board.clearContents();
        assert!(!mac::marked(&board));
    }
}
