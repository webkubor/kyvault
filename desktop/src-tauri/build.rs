fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "touch_activity",
            "snapshot",
            "capture_clipboard",
            "set_monitoring",
            "preview_text",
            "save_text",
            "delete_entry",
            "restore_entry",
            "copy_entry",
            "copy_reference",
            "copy_secret",
            "save_secret",
            "delete_secret",
            "lock_app",
            "unlock_app",
            "clear_history",
            "set_ui_language",
        ]),
    ))
    .expect("Ky build configuration");
}
