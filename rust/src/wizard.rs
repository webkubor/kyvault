//! `kyvault wizard` —— 交互式首次设置。
//!
//! 与 Python 版（kyvault/wizard.py）同一套问答流程，两处刻意不同：
//!
//! ① **密钥值走不回显输入**。Python 版用 `input()`，明文会留在终端回滚里，
//!    也会被终端录制/共享屏幕看到 —— 一个专门用来藏密钥的工具，第一步就把密钥
//!    打在屏幕上说不过去。这里在 unix 下关掉 echo 读取。
//! ② **非交互环境直接拒绝**，而不是卡在等输入。Python 版在管道里跑会读到 EOF
//!    然后抛异常；agent/CI 拉起它时表现成"莫名失败"。这里明确报错并指路 set。

use std::io::{self, BufRead, Write};

use anyhow::{anyhow, Result};

use crate::alias::Aliases;
use crate::store::Store;

/// 问答收集到的一条。落库不在这里做 —— wizard 只管交互，写入交给 main 的
/// Backend（那样 file / d1 都能落；Python 版写死 file 后端，在 cs kyvault 下
/// 跑 wizard 会把密钥存到本地而不是 D1 真源，人却以为存进去了）。
pub struct Entry {
    pub r#ref: String,
    pub value: String,
    pub kind: String,
    pub account: String,
    pub alias: Option<String>,
}

fn ask(prompt: &str, default: &str) -> Result<String> {
    if default.is_empty() {
        print!("{prompt}: ");
    } else {
        print!("{prompt} [{default}]: ");
    }
    io::stdout().flush()?;
    let mut s = String::new();
    // 读到 EOF（0 字节）说明 stdin 不是终端或已关闭 —— 别装作用户按了回车
    if io::stdin().lock().read_line(&mut s)? == 0 {
        return Err(anyhow!(
            "stdin 已结束：wizard 需要交互终端。非交互场景用 kyvault set / kyvault import"
        ));
    }
    let s = s.trim().to_string();
    Ok(if s.is_empty() { default.to_string() } else { s })
}

/// 读一行但不回显。拿不到 termios（非 unix / 非 tty）就退回明文读并出声提醒 ——
/// 静默回显密钥比提醒一句更糟。
fn ask_secret(prompt: &str) -> Result<String> {
    print!("{prompt}: ");
    io::stdout().flush()?;

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        // 用 stty 关 echo：比手写 termios FFI 短得多，且不引入依赖。
        // ponytail: 依赖系统有 stty；没有就落回明文读（下面有提醒）
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
            println!(); // 用户按的回车没回显，补一个换行
        } else {
            println!("  ⚠ 关不掉终端回显，刚才的输入是明文可见的");
        }
        if n == 0 {
            return Err(anyhow!("stdin 已结束：wizard 需要交互终端"));
        }
        return Ok(s.trim().to_string());
    }

    #[cfg(not(unix))]
    {
        println!("  ⚠ 非 unix 平台，输入将明文可见");
        let mut s = String::new();
        if io::stdin().lock().read_line(&mut s)? == 0 {
            return Err(anyhow!("stdin 已结束：wizard 需要交互终端"));
        }
        Ok(s.trim().to_string())
    }
}

#[cfg(unix)]
fn libc_dup(fd: std::os::unix::io::RawFd) -> Result<std::os::unix::io::RawFd> {
    // 不引 libc crate：dup 只为了把 stdin 交给 stty 而不夺走自己的
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

pub fn run(backend_name: &str) -> Result<Vec<Entry>> {
    println!("🔐 kyvault 设置向导");
    println!("{}", "=".repeat(40));

    // file 后端要 master key；d1 后端不用，但 init 是幂等的（已存在原样返回），
    // 无条件跑一次比分支判断简单，也免得人之后裸跑时才发现没初始化
    let store = Store::default_location()?;
    store.init_master_key()?;
    println!("✓ master key 已就绪（已存在则原样保留，绝不覆盖）");
    println!("  当前后端：{backend_name}\n");

    println!("接下来逐条录密钥，平台名输 q 结束。\n");
    let mut out: Vec<Entry> = Vec::new();

    loop {
        println!("--- 密钥 #{} ---", out.len() + 1);
        let platform = ask("平台（如 github / deepseek / zhipu）", "")?;
        if platform.eq_ignore_ascii_case("q") || platform.is_empty() {
            break;
        }
        let name = ask("名称（如 my-pat / api-key）", "")?;
        if name.eq_ignore_ascii_case("q") || name.is_empty() {
            break;
        }
        let value = ask_secret("密钥值（不回显）")?;
        if value.is_empty() {
            println!("  · 空值，跳过这条\n");
            continue;
        }
        let kind = ask("类型", "API Key")?;
        let account = ask("账号/备注（可选）", "")?;

        let r#ref = format!("secret://{platform}/{name}");
        let lower = name.to_lowercase();
        let default_alias = if lower.contains("token") || lower.contains("pat") {
            format!("{platform}_token")
        } else {
            format!("{platform}_{name}")
        };
        let alias = ask(&format!("别名（回车用 {default_alias}，输 n 跳过）"), &default_alias)?;
        let alias = if alias.eq_ignore_ascii_case("n") || alias.is_empty() {
            None
        } else {
            Some(alias)
        };
        out.push(Entry {
            r#ref,
            value,
            kind,
            account,
            alias,
        });
        println!();
    }

    println!("\n{}", "=".repeat(40));
    if out.is_empty() {
        println!("没录入密钥。之后可以用：kyvault set secret://平台/名称 - （明文走 stdin）");
    } else {
        println!("✅ 收集到 {} 条，开始写入…\n", out.len());
        println!("用法：");
        println!("  kyvault list                        看清单（不含明文）");
        println!("  kyvault get <别名>                  读明文");
        println!("  kyvault run --env X=<别名> -- cmd   注入子进程，不打印明文");
        println!("  kyvault check <平台> <名称>         验这把 key 还活着没");
    }
    let _ = &Aliases::default_location();
    Ok(out)
}
