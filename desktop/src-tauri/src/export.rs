// The renderer supplies a share document, never a destination path. The native
// save panel is the only way to authorize an export outside the encrypted store.
use crate::commands::{active, DesktopState};
use tauri::State;

#[tauri::command]
pub fn export_workspace_file(
    name: String,
    content: String,
    state: State<DesktopState>,
) -> Result<bool, String> {
    drop(active(&state)?);
    if !name.ends_with(".html")
        || name.len() > 240
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
        || content.len() > 32 * 1024 * 1024
    {
        return Err("分享文件名或大小无效".into());
    }
    #[cfg(target_os = "macos")]
    {
        use std::ffi::{c_char, CStr, CString};
        use std::os::unix::ffi::OsStrExt;
        extern "C" {
            fn ky_choose_export(name: *const c_char) -> *mut c_char;
            fn ky_release_path(path: *mut c_char);
        }
        let name = CString::new(name).map_err(|_| "无效文件名")?;
        let selected = unsafe { ky_choose_export(name.as_ptr()) };
        if selected.is_null() {
            return Ok(false);
        }
        let bytes = unsafe { CStr::from_ptr(selected).to_bytes().to_owned() };
        unsafe { ky_release_path(selected) };
        let target = std::path::Path::new(std::ffi::OsStr::from_bytes(&bytes));
        if target.extension().and_then(|s| s.to_str()) != Some("html") {
            return Err("分享文件需要使用 .html 扩展名".into());
        }
        let _session = active(&state)?;
        let mut tmp = tempfile::NamedTempFile::new_in(target.parent().ok_or("保存位置无效")?)
            .map_err(|_| "无法创建分享文件")?;
        std::io::Write::write_all(&mut tmp, content.as_bytes()).map_err(|_| "写入失败")?;
        tmp.as_file().sync_all().map_err(|_| "保存失败")?;
        tmp.persist(target).map_err(|_| "保存失败")?;
        Ok(true)
    }
    #[cfg(not(target_os = "macos"))]
    Err("此平台暂不支持桌面分享导出".into())
}
