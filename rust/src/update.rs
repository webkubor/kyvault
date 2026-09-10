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

const RELEASES_API: &str = "https://api.github.com/repos/webkubor/kyvault/releases/latest";
const INSTALL_SH: &str = "https://raw.githubusercontent.com/webkubor/kyvault/main/install.sh";

/// 取最新 release 的 tag（去掉前导 v）。
pub fn latest_version() -> Result<String> {
    let body: Value = ureq::get(RELEASES_API)
        .set("Accept", "application/vnd.github+json")
        // GitHub 对无 UA 的请求会 403
        .set("User-Agent", concat!("kyvault/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .map_err(|e| anyhow!("查 GitHub Release 失败：{e}"))?
        .into_json()
        .map_err(|e| anyhow!("GitHub 返回的不是 JSON：{e}"))?;
    let tag = body
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Release 里没有 tag_name"))?;
    Ok(tag.trim().trim_start_matches('v').to_string())
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
}
