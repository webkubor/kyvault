//! Signed desktop updates. Endpoint and verification key come from Tauri config.
use serde::Serialize;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};

type Result<T> = std::result::Result<T, String>;
static INSTALLING: AtomicBool = AtomicBool::new(false);
static READY_TO_RESTART: AtomicBool = AtomicBool::new(false);

// Release on failure/cancellation. Once installed, keep the gate until restart.
struct InstallGuard<'a> {
    flag: &'a AtomicBool,
    installed: bool,
}
impl<'a> InstallGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Result<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "更新正在进行，请稍候".to_string())?;
        Ok(Self {
            flag,
            installed: false,
        })
    }
}
impl Drop for InstallGuard<'_> {
    fn drop(&mut self) {
        if !self.installed {
            self.flag.store(false, Ordering::Release);
        }
    }
}

#[derive(Serialize)]
pub struct UpdateStatus {
    current: &'static str,
    version: Option<String>,
}

async fn find_update(app: &AppHandle, timeout: Duration) -> Result<Option<Update>> {
    // Do not override endpoints or pubkey: both use the bundled updater config.
    app.updater_builder()
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateStatus> {
    if INSTALLING.load(Ordering::Acquire) {
        return Err("更新正在进行，请稍候".into());
    }
    let update = find_update(&app, Duration::from_secs(20)).await?;
    Ok(UpdateStatus {
        current: env!("CARGO_PKG_VERSION"),
        version: update.map(|u| u.version),
    })
}

pub async fn check_in_background(app: &AppHandle) -> Result<()> {
    let status = check_update(app.clone()).await?;
    if !INSTALLING.load(Ordering::Acquire) {
        if let Some(version) = status.version {
            let _ = app.emit(
                "update-available",
                serde_json::json!({"version":version,"current":status.current}),
            );
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<()> {
    let mut guard = InstallGuard::acquire(&INSTALLING)?;
    let update = find_update(&app, Duration::from_secs(120))
        .await?
        .ok_or_else(|| "已经是最新版本".to_string())?;
    let version = update.version.clone();
    let mut downloaded = 0_u64;
    let _ = app.emit(
        "update-progress",
        serde_json::json!({"phase":"downloading","version":version}),
    );
    // The plugin verifies the configured public-key signature before installation.
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app.emit("update-progress", serde_json::json!({
                "phase":"downloading", "version":version, "downloaded":downloaded, "total":total
            }));
            },
            || {
                let _ = app.emit(
                    "update-progress",
                    serde_json::json!({"phase":"installing","version":version}),
                );
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    guard.installed = true;
    READY_TO_RESTART.store(true, Ordering::Release);
    let _ = app.emit(
        "update-progress",
        serde_json::json!({"phase":"installed","version":version}),
    );
    Ok(())
}

#[tauri::command]
pub fn restart_after_update(app: AppHandle) -> Result<()> {
    if !READY_TO_RESTART.load(Ordering::Acquire) {
        return Err("尚未安装更新".into());
    }
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_gate_blocks_overlapping_attempts_and_releases_after_failure() {
        let flag = AtomicBool::new(false);
        let first = InstallGuard::acquire(&flag).unwrap();
        assert!(InstallGuard::acquire(&flag).is_err());
        drop(first); // Network, signature, download and install failures all drop here.
        assert!(InstallGuard::acquire(&flag).is_ok());
    }

    #[test]
    fn installed_gate_stays_closed_until_process_restarts() {
        let flag = AtomicBool::new(false);
        let mut first = InstallGuard::acquire(&flag).unwrap();
        first.installed = true;
        drop(first);
        assert!(InstallGuard::acquire(&flag).is_err());
    }
}
