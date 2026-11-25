//! `kyvault update` —— 在线检查新版并升级。
//!
//! **语义与 Python 版不同，是刻意的。** Python 版跑 `pip install --upgrade kyvault`
//! 或 `uv tool upgrade`，还先查 PyPI 再退回 GitHub —— 而 PyPI 从来没成功发布过
//! （见 0952a6d「移除从未成功过的 PyPI 发布」），那条路径一直是死的。
//!
//! Rust 版只有一条真路径：GitHub Release 上的静态二进制，由 install.sh 按平台挑。
//! 所以这里做的是「查 tag → 比版本 → 交给 install.sh」，不自己下载解包 ——
//! 自更新锁定同一发布标签，更新当前可执行文件所在目录；安装器负责原子替换。

use std::io::{self, Write};
use std::process::Command;

use anyhow::{anyhow, Result};
use serde_json::Value;

const RELEASES_API: &str = "https://api.github.com/repos/webkubor/kyvault/releases";
const INSTALL_ROOT: &str = "https://raw.githubusercontent.com/webkubor/kyvault";

/// 产品线的发布标签前缀。CLI 用 `v`（v2.8.2），桌面 App 用 `desktop-v`（desktop-v0.2.0）。
/// 必须分开：桌面 v0.2.0 比 CLI v2.8.2 数字小，若共用一个 latest 会把 CLI 判成"需降级"。
const CLI_TAG_PREFIX: &str = "v";
const DESKTOP_TAG_PREFIX: &str = "desktop-v";

/// 取指定产品线的最新版本（去掉前缀），按 tag 前缀过滤而非依赖"latest"——
/// GitHub 的 latest 只有一条，CLI 与桌面各自发版时会互相顶掉（issue #15）。
fn latest_version_for(prefix: &str) -> Result<String> {
    let body: Value = ureq::get(&format!("{RELEASES_API}?per_page=100"))
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", concat!("kyvault/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(403, response)
                if response.header("X-RateLimit-Remaining") == Some("0") =>
            {
                anyhow!("GitHub 更新检查额度已用尽，请稍后重试；当前版本未改动。")
            }
            other => anyhow!("查 GitHub Release 失败：{other}"),
        })?
        .into_json()
        .map_err(|e| anyhow!("GitHub 返回的不是 JSON：{e}"))?;
    select_release(&body, prefix)
}

// 只选择正式发布的稳定版本，CLI 与 App 分别筛选；不依赖 GitHub 的全局 latest。
fn select_release(body: &Value, prefix: &str) -> Result<String> {
    let releases = body
        .as_array()
        .ok_or_else(|| anyhow!("GitHub Release 返回的不是数组"))?;
    let mut latest: Option<&str> = None;
    for release in releases {
        if release.get("draft").and_then(Value::as_bool) == Some(true)
            || release.get("prerelease").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        let Some(version) = release
            .get("tag_name")
            .and_then(Value::as_str)
            .and_then(|tag| tag.strip_prefix(prefix))
        else {
            continue;
        };
        let parts: Vec<_> = version.split('.').collect();
        if parts.len() != 3
            || parts.iter().any(|p| {
                p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()) || p.parse::<u32>().is_err()
            })
        {
            continue;
        }
        if latest.map_or(true, |current| is_newer(version, current)) {
            latest = Some(version);
        }
    }
    latest
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("没有找到 {prefix} 前缀的稳定 Release"))
}

/// CLI 线的最新版本（供 `kyvault update` 与后台提示用）。
pub fn latest_version() -> Result<String> {
    latest_version_for(CLI_TAG_PREFIX)
}

/// 桌面 App 线的最新版本（供 Ky.app 更新检测复用同一套版本比较）。
pub fn latest_desktop_version() -> Result<String> {
    latest_version_for(DESKTOP_TAG_PREFIX)
}

/// 纯函数：给定桌面线最新版与当前版本，判断是否需要提示升级。
/// 抽出来是为了单测不依赖网络（issue #15 的更新检测必须可验证）。
pub fn desktop_update_from(latest: &str, current: &str) -> Option<String> {
    if is_newer(latest, current) {
        Some(latest.to_string())
    } else {
        None
    }
}

/// 桌面 App 当前版本（来自 tauri.conf.json 同步到 Cargo.toml 的 CARGO_PKG_VERSION）。
/// 桌面与 CLI 共用 crate 版本号不同源，调用方传入当前版本比较。
pub fn desktop_update_available(current: &str) -> Result<Option<String>> {
    let latest = latest_desktop_version()?;
    Ok(desktop_update_from(&latest, current))
}

/// 语义化版本比较。解析不出来的段按 0 处理 —— 宁可判成"不需要升级"，
/// 也不要因为一个 rc 后缀就把人推去重装一遍。
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split('.')
            .map(|p| {
                p.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    let (a, b) = (parse(latest), parse(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

pub fn run(assume_yes: bool) -> Result<()> {
    run_with_options(assume_yes, false)
}

pub fn run_with_options(assume_yes: bool, check_only: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("🔄 检查最新版本…");
    let latest = latest_version()?;

    if !is_newer(&latest, current) {
        println!("✅ 已是最新：v{current}");
        return Ok(());
    }
    println!("🔥 发现新版本：v{current} → v{latest}");
    if check_only {
        println!("运行 `ky update` 安装更新。");
        return Ok(());
    }

    if !assume_yes {
        print!("升级吗？[y/N]: ");
        io::stdout().flush().ok();
        let mut ans = String::new();
        io::stdin().read_line(&mut ans)?;
        let ans = ans.trim().to_lowercase();
        if ans != "y" && ans != "yes" {
            println!("已取消。");
            return Ok(());
        }
    }

    if cfg!(windows) {
        return Err(anyhow!(
            "Windows 请从 GitHub Release 下载新版 kyvault.exe 并替换当前程序。"
        ));
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    if executable.components().any(|c| c.as_os_str() == "Cellar") {
        return Err(anyhow!(
            "当前由 Homebrew 管理，请运行 `brew upgrade kyvault`。"
        ));
    }
    let install_dir = executable
        .parent()
        .ok_or_else(|| anyhow!("无法确定安装目录"))?;
    let url = format!("{INSTALL_ROOT}/v{latest}/install.sh");
    println!("🚀 安装 v{latest} 到 {}…", install_dir.display());
    let script = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| anyhow!("下载发布版安装器失败：{e}"))?
        .into_string()?;
    // 独占临时文件，避免多个更新进程互相覆盖或跟随预置符号链接。
    let mut tmp = tempfile::NamedTempFile::new()?;
    tmp.write_all(script.as_bytes())?;
    tmp.flush()?;
    let status = Command::new("bash")
        .arg(tmp.path())
        .env("KYVAULT_VERSION", format!("v{latest}"))
        .env("KYVAULT_INSTALL_DIR", install_dir)
        .env("KYVAULT_UPDATE_ONLY", "1")
        .status()?;
    if !status.success() {
        return Err(anyhow!(
            "更新失败（安装器退出码 {:?}）；原程序保留，请稍后重试。",
            status.code()
        ));
    }
    let installed = Command::new(&executable).arg("--version").output()?;
    let expected = format!("kyvault {latest}");
    if !installed.status.success() || String::from_utf8_lossy(&installed.stdout).trim() != expected
    {
        return Err(anyhow!(
            "安装后版本核验未通过，请检查 {}。",
            executable.display()
        ));
    }
    println!("🎉 已升级到 v{latest}");
    Ok(())
}

/// 交互式 CLI 的"后台"更新提示：每次命令结束顺带轻量检查，24 小时一次，
/// 失败静默（网络不通不该打扰用密钥）。提示走 stderr，不污染 `get` 的 stdout 明文纪律。
/// 由 main() 在命令成功后调用；非 TTY 直接跳过（CI / agent 子进程不该看到提示）。
pub fn notify_if_due() {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return;
    }
    let arg0 = std::env::args().nth(1).unwrap_or_default();
    // 这些命令本身就在处理更新/诊断/补全，别叠加提示
    if matches!(
        arg0.as_str(),
        "update" | "doctor" | "completion" | "help" | "--help" | "-h"
    ) {
        return;
    }
    let state_path = update_state_path();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(s) = std::fs::read_to_string(&state_path) {
        if let Ok(last) = s.trim().parse::<u64>() {
            if now.saturating_sub(last) < 24 * 3600 {
                return;
            }
        }
    }
    // 记录本次检查时间（先写，避免每次失败都重试联网）
    if let Some(parent) = state_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&state_path, now.to_string());

    // 只查 CLI 线；发现新版提示，不自动升级（自动升级要用户拍板）
    if let Ok(latest) = latest_version() {
        let current = env!("CARGO_PKG_VERSION");
        if is_newer(&latest, current) {
            eprintln!(
                "\n💡 kyvault 有新版本：v{current} → v{latest}。运行 `kyvault update -y` 升级。"
            );
        }
    }
}

fn update_state_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("KY_UPDATE_STATE") {
        return std::path::PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join(".config")
        .join("kyvault")
        .join("update-check.txt")
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn stable_release_selection() {
        let releases = serde_json::json!([
            {"tag_name":"desktop-v0.3.0"},
            {"tag_name":"v9.0.0", "draft":true},
            {"tag_name":"v8.0.0", "prerelease":true},
            {"tag_name":"v7.0.0-rc1"},
            {"tag_name":"v2.9.0"},
            {"tag_name":"v2.10.0"},
            {"tag_name":"v../../unsafe"}
        ]);
        assert_eq!(super::select_release(&releases, "v").unwrap(), "2.10.0");
        assert_eq!(
            super::select_release(&releases, "desktop-v").unwrap(),
            "0.3.0"
        );
        assert!(super::select_release(&serde_json::json!([]), "v").is_err());
        assert!(super::select_release(&serde_json::json!({}), "v").is_err());
    }

    #[test]
    fn version_compare() {
        assert!(is_newer("2.1.0", "2.0.0"));
        assert!(is_newer("2.0.1", "2.0.0"));
        assert!(is_newer("10.0.0", "9.9.9"), "按数字比，不是按字典序");
        assert!(!is_newer("2.0.0", "2.0.0"));
        assert!(!is_newer("1.9.9", "2.0.0"));
        // 段数不齐要能比
        assert!(is_newer("2.1", "2.0.5"));
        assert!(!is_newer("2.0", "2.0.0"));
        // 带后缀的段按数字前缀取，解析不出就当 0 —— 不因为一个 rc 把人推去重装
        assert!(!is_newer("2.0.0-rc1", "2.0.0"));
        assert!(is_newer("2.0.1-rc1", "2.0.0"));
    }

    #[test]
    fn version_lines_are_independent() {
        // 桌面 v0.2.0 与 CLI v2.8.2 是两条线：共用一个 latest 会把 CLI 误判成需降级。
        // is_newer 只比数字，所以桌面 0.2.0 vs CLI 2.8.2 必然 false —— 这正是分离前缀的意义。
        assert!(!is_newer("0.2.0", "2.8.2"), "桌面版本不能被当成 CLI 升级");
        assert!(is_newer("2.9.0", "2.8.2"), "CLI 自己的新版本仍能被识别");
        assert_ne!("v", "desktop-v", "CLI 与桌面的 tag 前缀必须不同");
    }

    #[test]
    fn desktop_update_detection() {
        use super::desktop_update_from;
        // 装有旧版 0.2.0 的用户，检测到 0.2.1 应提示
        assert_eq!(
            desktop_update_from("0.2.1", "0.2.0"),
            Some("0.2.1".to_string())
        );
        // 已是最新 0.2.1，不提示
        assert_eq!(desktop_update_from("0.2.1", "0.2.1"), None);
        // 本地版本比线上新（开发版），不提示
        assert_eq!(desktop_update_from("0.2.1", "0.3.0"), None);
    }
}
