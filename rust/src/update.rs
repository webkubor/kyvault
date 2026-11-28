//! CLI 原地更新只替换当前可执行文件。新装与旧 Python 入口迁移由 install.sh 处理。

use std::io::{self, Write};
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::Command;

use anyhow::{anyhow, Result};
use serde_json::Value;

const RELEASES_API: &str = "https://api.github.com/repos/webkubor/kyvault/releases";

const CLI_RELEASES_PER_PAGE: usize = 100;

/// 产品线的发布标签前缀。CLI 用 `v`（v2.8.2），桌面 App 用 `desktop-v`（desktop-v0.2.0）。
/// 必须分开：桌面 v0.2.0 比 CLI v2.8.2 数字小，若共用一个 latest 会把 CLI 判成"需降级"。
const CLI_TAG_PREFIX: &str = "v";
const DESKTOP_TAG_PREFIX: &str = "desktop-v";

fn fetch_releases(url: &str) -> Result<Value> {
    ureq::get(url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", concat!("kyvault/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .map_err(|e| anyhow!("查 GitHub Release 失败：{e}"))?
        .into_json()
        .map_err(|e| anyhow!("GitHub 返回的不是 JSON：{e}"))
}

/// 保留桌面版现有的前缀筛选，不使用全仓唯一的 GitHub latest（issue #15）。
fn latest_version_for(prefix: &str) -> Result<String> {
    let body = fetch_releases(RELEASES_API)?;
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

// 与 install.sh 一致，只接受没有后缀、没有多余前导 0 的 X.Y.Z。
// 先比位数再比字面值，相当于按数字比较，不受整数类型的范围限制。
fn stable_cli_version_key(version: &str) -> Option<[(usize, &str); 3]> {
    fn component(part: &str) -> Option<(usize, &str)> {
        (!part.is_empty()
            && part.bytes().all(|b| b.is_ascii_digit())
            && (part.len() == 1 || !part.starts_with('0')))
        .then_some((part.len(), part))
    }
    let mut parts = version.split('.');
    let key = [
        component(parts.next()?)?,
        component(parts.next()?)?,
        component(parts.next()?)?,
    ];
    parts.next().is_none().then_some(key)
}

fn latest_cli_version_from_pages(
    per_page: usize,
    mut fetch: impl FnMut(usize) -> Result<Value>,
) -> Result<String> {
    let mut latest = String::new();
    for page in 1.. {
        let response = fetch(page)?;
        let releases = response
            .as_array()
            .ok_or_else(|| anyhow!("GitHub Release 返回的不是数组"))?;
        for release in releases {
            if release.get("draft").and_then(Value::as_bool) != Some(false)
                || release.get("prerelease").and_then(Value::as_bool) != Some(false)
            {
                continue;
            }
            let Some(version) = release
                .get("tag_name")
                .and_then(Value::as_str)
                .and_then(|tag| tag.strip_prefix(CLI_TAG_PREFIX))
            else {
                continue;
            };
            if let Some(key) = stable_cli_version_key(version) {
                if Some(key) > stable_cli_version_key(&latest) {
                    latest = version.to_owned();
                }
            }
        }
        if releases.len() < per_page {
            break;
        }
    }
    if latest.is_empty() {
        return Err(anyhow!("没有找到稳定的 CLI Release（vX.Y.Z）"));
    }
    Ok(latest)
}

/// CLI 线的最新版本（供 `kyvault update` 与后台提示用）。
pub fn latest_version() -> Result<String> {
    latest_cli_version_from_pages(CLI_RELEASES_PER_PAGE, |page| {
        fetch_releases(&format!(
            "{RELEASES_API}?per_page={CLI_RELEASES_PER_PAGE}&page={page}"
        ))
    })
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

fn is_newer_cli(latest: &str, current: &str) -> bool {
    match (
        stable_cli_version_key(latest),
        stable_cli_version_key(current),
    ) {
        (Some(latest), Some(current)) => latest > current,
        _ => is_newer(latest, current),
    }
}

#[cfg(unix)]
fn managed_path(exe: &Path) -> bool {
    [
        "/usr",
        "/opt",
        "/bin",
        "/sbin",
        "/System",
        "/nix",
        "/snap",
        "/home/linuxbrew",
    ]
    .iter()
    .any(|prefix| exe.starts_with(prefix))
        || exe
            .ancestors()
            .any(|p| p.ends_with("Cellar") || p.ends_with(".cargo/bin"))
        || ["HOMEBREW_CELLAR", "CARGO_HOME", "CARGO_INSTALL_ROOT"]
            .iter()
            .filter_map(std::env::var_os)
            .filter_map(|p| std::fs::canonicalize(p).ok())
            .any(|root| exe.starts_with(root))
}

#[cfg(unix)]
fn replace_from_archive(exe: &Path, archive: &Path, latest: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let file = tempfile::NamedTempFile::new_in(exe.parent().unwrap())?;
    let unpack = Command::new("tar")
        .arg("-xzOf")
        .arg(archive)
        .arg("kyvault")
        .stdout(file.reopen()?)
        .status()?;
    anyhow::ensure!(unpack.success(), "CLI Release 解包失败");
    let stage = file.into_temp_path();
    std::fs::set_permissions(
        &stage,
        std::fs::Permissions::from_mode(std::fs::metadata(exe)?.permissions().mode() & 0o777),
    )?;
    let check = Command::new(&stage).arg("--version").output()?;
    anyhow::ensure!(
        check.status.success() && check.stdout == format!("kyvault {latest}\n").as_bytes(),
        "下载的 CLI 版本与 v{latest} 不符"
    );
    stage.persist(exe)?;
    Ok(())
}

#[cfg(unix)]
fn upgrade(latest: &str) -> Result<()> {
    use anyhow::Context;
    use std::os::unix::fs::MetadataExt;
    let exe = std::env::current_exe()?.canonicalize()?;
    anyhow::ensure!(
        !managed_path(&exe),
        "当前 CLI 位于系统或包管理器目录，请通过原安装渠道升级"
    );
    anyhow::ensure!(
        exe.metadata()?.nlink() == 1,
        "当前 CLI 有多个硬链接，请通过原安装渠道升级"
    );
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") if cfg!(target_env = "gnu") => "aarch64-unknown-linux-gnu",
        ("linux", "x86_64") if cfg!(target_env = "gnu") => "x86_64-unknown-linux-gnu",
        _ => return Err(anyhow!("当前平台请使用原安装渠道升级")),
    };
    let archive = tempfile::NamedTempFile::new_in(exe.parent().unwrap())
        .context("无法在 CLI 所在目录创建临时文件，请通过原安装渠道升级")?;
    let url = format!(
        "https://github.com/webkubor/kyvault/releases/download/v{latest}/kyvault-{target}.tar.gz"
    );
    let status = Command::new("curl")
        .args(["-fsSL", "--retry", "2", "-o"])
        .arg(archive.path())
        .arg(url)
        .status()?;
    anyhow::ensure!(status.success(), "下载 v{latest} 失败");
    replace_from_archive(&exe, archive.path(), latest)
}

#[cfg(not(unix))]
fn upgrade(latest: &str) -> Result<()> {
    Err(anyhow!("当前平台请通过原安装渠道升级，Windows 可退出 CLI 后解压 v{latest} 的官方 ZIP 并替换 kyvault.exe"))
}

pub fn run(assume_yes: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("🔄 检查最新版本…");
    let latest = latest_version()?;

    if !is_newer_cli(&latest, current) {
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

    upgrade(&latest)?;
    println!("🎉 当前 CLI 已原地升级到 v{latest}");
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
        if is_newer_cli(&latest, current) {
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
    use super::{is_newer, latest_cli_version_from_pages};
    use serde_json::json;

    #[test]
    fn latest_stable_cli_version_is_independent_from_desktop_and_publication_order() {
        let releases = [
            json!([
                {"tag_name":"desktop-v99.0.0","draft":false,"prerelease":false},
                {"tag_name":"v11.0.0","draft":true,"prerelease":false},
                {"tag_name":"v10.0.0","draft":false,"prerelease":true},
                {"tag_name":"v9.0.0-rc1","draft":false,"prerelease":false},
                {"tag_name":"v08.0.0","draft":false,"prerelease":false},
                {"tag_name":"v7.0.0","prerelease":false},
                {"tag_name":"v2.9.0","draft":false,"prerelease":false}
            ]),
            json!([{"tag_name":"v2.10.0","draft":false,"prerelease":false}]),
        ];
        let result = latest_cli_version_from_pages(7, |page| Ok(releases[page - 1].clone()));
        assert_eq!(result.unwrap(), "2.10.0");
    }

    #[test]
    fn no_stable_cli_release_is_an_error() {
        assert!(latest_cli_version_from_pages(100, |_| Ok(json!([]))).is_err());
        assert!(latest_cli_version_from_pages(100, |_| Ok(json!({}))).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn package_manager_paths_are_protected() {
        use std::path::Path;
        for path in [
            "/opt/homebrew/bin/kyvault",
            "/usr/bin/kyvault",
            "/custom/Cellar/kyvault/2/bin/kyvault",
            "/nix/store/pkg/bin/kyvault",
            "/home/user/.cargo/bin/kyvault",
        ] {
            assert!(super::managed_path(Path::new(path)));
        }
        assert!(!super::managed_path(Path::new(
            "/home/user/.local/bin/kyvault"
        )));
    }

    #[cfg(unix)]
    #[test]
    fn update_verifies_version_and_preserves_alias() {
        use std::{
            fs,
            os::unix::fs::{symlink, PermissionsExt},
            process::Command,
        };
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("custom cli");
        let alias = dir.path().join("ky");
        fs::write(&exe, "original").unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o750)).unwrap();
        symlink(&exe, &alias).unwrap();
        fs::write(
            dir.path().join("kyvault"),
            "#!/bin/sh\necho kyvault 2.10.0\n",
        )
        .unwrap();
        let archive = dir.path().join("test.tgz");
        assert!(Command::new("tar")
            .current_dir(dir.path())
            .args(["-czf", "test.tgz", "kyvault"])
            .status()
            .unwrap()
            .success());
        assert!(super::replace_from_archive(&exe, &archive, "2.11.0").is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"original");
        super::replace_from_archive(&exe, &archive, "2.10.0").unwrap();
        assert_eq!(fs::read_link(&alias).unwrap(), exe);
        assert_eq!(
            Command::new(alias)
                .arg("--version")
                .output()
                .unwrap()
                .stdout,
            b"kyvault 2.10.0\n"
        );
        assert_eq!(
            fs::metadata(exe).unwrap().permissions().mode() & 0o777,
            0o750
        );
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
