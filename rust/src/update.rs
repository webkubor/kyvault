//! `kyvault update` —— 在线检查新版并升级。
//!
//! **语义与 Python 版不同，是刻意的。** Python 版跑 `pip install --upgrade kyvault`
//! 或 `uv tool upgrade`，还先查 PyPI 再退回 GitHub —— 而 PyPI 从来没成功发布过
//! （见 0952a6d「移除从未成功过的 PyPI 发布」），那条路径一直是死的。
//!
//! Rust 版只有一条真路径：GitHub Release 上的静态二进制，由 install.sh 按平台挑。
//! 所以这里做的是「查 tag → 比版本 → 交给 install.sh」，不自己下载解包 ——
//! install.sh 还顺手清理旧 Python 版（pipx 包 / usr-local shim / 数据目录模块副本），
//! 那些清理逻辑不该在两处各写一遍。

use std::io::{self, Write};
use std::process::Command;

use anyhow::{anyhow, Result};
use serde_json::Value;

const RELEASES_API: &str = "https://api.github.com/repos/webkubor/kyvault/releases";
const INSTALL_SH: &str = "https://raw.githubusercontent.com/webkubor/kyvault/main/install.sh";

/// 产品线的发布标签前缀。CLI 用 `v`（v2.8.2），桌面 App 用 `desktop-v`（desktop-v0.2.0）。
/// 必须分开：桌面 v0.2.0 比 CLI v2.8.2 数字小，若共用一个 latest 会把 CLI 判成"需降级"。
const CLI_TAG_PREFIX: &str = "v";
const DESKTOP_TAG_PREFIX: &str = "desktop-v";

/// 取指定产品线的最新版本（去掉前缀），按 tag 前缀过滤而非依赖"latest"——
/// GitHub 的 latest 只有一条，CLI 与桌面各自发版时会互相顶掉（issue #15）。
fn latest_version_for(prefix: &str) -> Result<String> {
    let body: Value = ureq::get(RELEASES_API)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", concat!("kyvault/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .map_err(|e| anyhow!("查 GitHub Release 失败：{e}"))?
        .into_json()
        .map_err(|e| anyhow!("GitHub 返回的不是 JSON：{e}"))?;
    let releases = body
        .as_array()
        .ok_or_else(|| anyhow!("GitHub Release 返回的不是数组"))?;
    // GitHub 按发布时间倒序，取第一条匹配该产品线前缀的 tag 即可。
    let tag = releases
        .iter()
        .filter_map(|r| r.get("tag_name").and_then(Value::as_str))
        .find(|t| t.starts_with(prefix))
        .ok_or_else(|| anyhow!("没有找到 {} 前缀的 Release", prefix))?;
    Ok(tag.trim().trim_start_matches(prefix).to_string())
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
    let current = env!("CARGO_PKG_VERSION");
    println!("🔄 检查最新版本…");
    let latest = latest_version()?;

    if !is_newer(&latest, current) {
        println!("✅ 已是最新：v{current}");
        return Ok(());
    }
    println!("🔥 发现新版本：v{current} → v{latest}");

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

    println!("🚀 交给 install.sh（它会按平台挑二进制，并清理旧 Python 版）…");
    // 不用 curl | bash 一条管道：那样 install.sh 的失败会被管道吃掉退出码。
    // 先下载到临时文件，再显式跑，出错能看见。
    let script = ureq::get(INSTALL_SH)
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| anyhow!("下载 install.sh 失败：{e}"))?
        .into_string()?;
    let tmp = std::env::temp_dir().join("kyvault-install.sh");
    std::fs::write(&tmp, script)?;

    let status = Command::new("bash").arg(&tmp).status()?;
    let _ = std::fs::remove_file(&tmp);
    if status.success() {
        println!("🎉 已升级到 v{latest}（新开一个 shell 或 hash -r 让 PATH 生效）");
        Ok(())
    } else {
        Err(anyhow!(
            "install.sh 退出码 {:?} —— 手动重试：curl -fsSL {INSTALL_SH} | bash",
            status.code()
        ))
    }
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
