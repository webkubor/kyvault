//! `kyvault wizard` —— 极客引导配置向导。
//!
//! 支持分类预设引导，规整化命名，自动组装规范 `secret://<platform>/<name>`，
//! 并提供启动本地 Web GUI 的快捷入口。

use std::io::{self, BufRead, Write};

use anyhow::{anyhow, Result};

use crate::category::{format_uri, CATEGORIES};
use crate::store::Store;
use crate::tui::*;

pub struct WizardEntry {
    pub r#ref: String,
    pub value: String,
    pub kind: String,
    pub account: String,
    pub alias: Option<String>,
}

fn prompt(msg: &str) -> Result<String> {
    print!("  {} ", cyan(msg));
    io::stdout().flush()?;
    let mut s = String::new();
    if io::stdin().lock().read_line(&mut s)? == 0 {
        return Err(anyhow!("stdin 已关闭"));
    }
    Ok(s.trim().to_string())
}

fn prompt_default(msg: &str, default: &str) -> Result<String> {
    print!("  {} {}: ", cyan(msg), dim(&format!("[默认: {default}]")));
    io::stdout().flush()?;
    let mut s = String::new();
    if io::stdin().lock().read_line(&mut s)? == 0 {
        return Err(anyhow!("stdin 已关闭"));
    }
    let s = s.trim().to_string();
    if s.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(s)
    }
}

/// 密码不回显安全输入
fn prompt_secret(msg: &str) -> Result<String> {
    print!("  {} ", yellow(msg));
    io::stdout().flush()?;

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        let off = std::process::Command::new("stty")
            .args(["-echo"])
            .stdin(std::process::Stdio::from(unsafe {
                use std::os::unix::io::FromRawFd;
                std::fs::File::from_raw_fd(libc_dup(fd)?)
            }))
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        let mut s = String::new();
        let n = io::stdin().lock().read_line(&mut s)?;

        if off {
            let _ = std::process::Command::new("stty").args(["echo"]).status();
            println!();
        } else {
            println!("  ⚠ 无法关闭终端回显，输入可见");
        }
        if n == 0 {
            return Err(anyhow!("stdin 已关闭"));
        }
        Ok(s.trim().to_string())
    }

    #[cfg(not(unix))]
    {
        let mut s = String::new();
        if io::stdin().lock().read_line(&mut s)? == 0 {
            return Err(anyhow!("stdin 已关闭"));
        }
        Ok(s.trim().to_string())
    }
}

#[cfg(unix)]
fn libc_dup(fd: std::os::unix::io::RawFd) -> Result<std::os::unix::io::RawFd> {
    let new = unsafe { dup_raw(fd) };
    if new < 0 {
        return Err(anyhow!("dup(stdin) 失败"));
    }
    Ok(new)
}

#[cfg(unix)]
unsafe fn dup_raw(fd: i32) -> i32 {
    extern "C" {
        fn dup(fd: i32) -> i32;
    }
    dup(fd)
}

/// 运行引导式向导
pub fn run() -> Result<Option<WizardEntry>> {
    box_top("🧭 kyvault 极客配置向导");
    println!();
    println!("  欢迎使用 kyvault 引导配置！通过标准化分类解决命名不统一问题。");
    println!();

    println!("  {} 请选择操作模式：", bold("Step 1/4"));
    println!("    [1] ➕ 录入新密钥 (命令行向导)");
    println!("    [2] 🌐 打开本地 Web GUI 图形界面 (推荐，更直观)");
    println!("    [q] 退出");
    println!();

    let choice = prompt("请输入选项 [默认: 1]:")?;
    if choice == "2" {
        println!();
        info_line("Web GUI", "正在启动本地 Web 界面并唤起浏览器...");
        box_bottom();
        // 唤起 Web GUI
        crate::ui::start_server(8765, true)?;
        return Ok(None);
    }
    if choice.eq_ignore_ascii_case("q") {
        box_bottom();
        return Ok(None);
    }

    // Step 1: 选分类
    println!();
    box_sep();
    println!();
    println!("  {} 请选择凭证大类：", bold("Step 2/4"));
    for (i, cat) in CATEGORIES.iter().enumerate() {
        println!("    [{}] {} {}", i + 1, cat.icon, bold(cat.name));
    }
    println!("    [c] ⚙️ 自定义其他平台");
    println!();

    let cat_choice = prompt_default("请选择分类编号", "1")?;
    let (selected_cat, custom_platform) = if cat_choice.eq_ignore_ascii_case("c") {
        (None, true)
    } else if let Ok(idx) = cat_choice.parse::<usize>() {
        if idx >= 1 && idx <= CATEGORIES.len() {
            (Some(&CATEGORIES[idx - 1]), false)
        } else {
            (Some(&CATEGORIES[0]), false)
        }
    } else {
        (Some(&CATEGORIES[0]), false)
    };

    // Step 2: 选平台
    let (platform_id, default_name, suggested_names, default_kind) =
        match (custom_platform, selected_cat) {
            (false, Some(cat)) => {
                println!();
                println!("  {} 请选择具体平台：", bold("Step 3/4"));
                for (i, p) in cat.platforms.iter().enumerate() {
                    println!("    [{}] {} {}", i + 1, p.icon, bold(p.name));
                }
                println!("    [o] 手动输入其他平台");
                println!();

                let p_choice = prompt_default("请选择平台编号", "1")?;
                if p_choice.eq_ignore_ascii_case("o") {
                    let p = prompt("请输入平台标识 (小写英文):")?;
                    (p, "main", &["main", "token"][..], cat.default_kind)
                } else if let Ok(idx) = p_choice.parse::<usize>() {
                    if idx >= 1 && idx <= cat.platforms.len() {
                        let p = &cat.platforms[idx - 1];
                        (
                            p.id.to_string(),
                            p.default_name,
                            p.suggested_names,
                            cat.default_kind,
                        )
                    } else {
                        let p = &cat.platforms[0];
                        (
                            p.id.to_string(),
                            p.default_name,
                            p.suggested_names,
                            cat.default_kind,
                        )
                    }
                } else {
                    let p = &cat.platforms[0];
                    (
                        p.id.to_string(),
                        p.default_name,
                        p.suggested_names,
                        cat.default_kind,
                    )
                }
            }
            _ => {
                println!();
                let p = prompt("请输入平台代码 (如 aws / my-site):")?;
                (p, "main", &["main", "token"][..], "API Key")
            }
        };

    // Step 3: 用途/环境 (生成规范 name)
    println!();
    println!("  {} 设定密钥用途标识：", bold("Step 4/4"));
    let sug_str = suggested_names.join(" / ");
    println!("    推荐命名候选：{}", dim(&sug_str));
    let name = prompt_default("标识名", default_name)?;

    let target_uri = format_uri(&platform_id, &name);
    println!();
    println!(
        "  {} 生成规范 URI：{}",
        bold_green("✓"),
        bold_cyan(&target_uri)
    );
    println!();

    // 输入密码
    let value = prompt_secret("请输入密钥/Token/密码（密文输入不回显）:")?;
    if value.is_empty() {
        println!("  ⚠ 空值，已取消");
        box_bottom();
        return Ok(None);
    }

    let account = prompt_default("备注说明或账号（可选）", "")?;

    // 保存确认
    let default_alias = format!("{}_{}", platform_id, name);
    let alias_prompt = prompt_default("是否创建快捷别名", &default_alias)?;
    let alias = if alias_prompt.eq_ignore_ascii_case("n") || alias_prompt.is_empty() {
        None
    } else {
        Some(alias_prompt)
    };

    box_sep();
    println!();
    status_line("URI 规范", true, &target_uri);
    status_line("密钥收集", true, "已就绪");
    println!();
    box_bottom();

    Ok(Some(WizardEntry {
        r#ref: target_uri,
        value,
        kind: default_kind.to_string(),
        account,
        alias,
    }))
}

/// 首次设置向导兼容入口（用于 init 或旧调用）
pub fn run_initial() -> Result<()> {
    let store = Store::default_location()?;
    store.init_master_key()?;
    if let Some(entry) = run()? {
        let _lock = store.lock()?;
        store.set_secret(&entry.r#ref, &entry.value)?;
        store.annotate(
            &entry.r#ref,
            Some(&entry.kind),
            if entry.account.is_empty() {
                None
            } else {
                Some(&entry.account)
            },
            None,
            None,
            None,
        )?;
        if let Some(alias) = entry.alias {
            let aliases = crate::alias::Aliases::default_location()?;
            aliases.set(&alias, &entry.r#ref)?;
        }
        println!();
        println!("🎉 {} 密钥已成功加密存入！", bold_green("SUCCESS"));
        println!("  安全调用示例：");
        println!(
            "    kyvault run --env TOKEN={} -- <cmd>",
            cyan(&entry.r#ref)
        );
        println!();
    }
    Ok(())
}
