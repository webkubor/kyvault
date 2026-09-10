//! kyvault CLI。
//!
//! 后端由 KYVAULT_BACKEND 选：默认 file（~/.keyring），设成 d1 走 Cloudflare D1。
//! 这和 Python 版完全一致 —— 同一台机器上两个实现必须看到同一份数据，
//! 否则「用 kyvault 存了、cs kyvault 读不到」这类事故会立刻发生。
//!
//! 输出纪律：**除了 get，任何命令都不打印明文**。list 只出元信息，
//! run 把密钥注进子进程环境变量、不落盘不回显 —— 这是这个工具存在的理由，
//! 为了调试方便打一行明文就等于把它作废。

use std::io::Read;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};

use kyvault::d1::D1;
use kyvault::model::SecretMeta;
use kyvault::store::Store;

#[derive(Parser)]
#[command(
    name = "kyvault",
    version,
    about = "本地加密密钥管理 — AES-256-GCM，AI 只拿别名，明文不出本机"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 生成 master key（已存在则原样返回，绝不覆盖）
    Init,
    /// 读取密钥并打印明文 —— 唯一会输出明文的命令
    Get { r#ref: String },
    /// 写入/更新密钥；value 传 - 表示从 stdin 读（避免出现在进程参数里）
    Set {
        r#ref: String,
        value: String,
        #[arg(long, default_value = "API Key")]
        kind: String,
        #[arg(long, default_value = "")]
        account: String,
    },
    /// 只改备注/类型，不需要提供密钥明文（仅 D1 后端）
    Annotate {
        r#ref: String,
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        kind: Option<String>,
    },
    /// 删除密钥
    Delete { r#ref: String },
    /// 列出密钥元信息（不含明文）
    List {
        #[arg(long)]
        platform: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// 列出所有平台及密钥数量
    Platforms,
    /// 把密钥注入子进程环境变量后执行命令（不打印明文）
    Run {
        /// NAME=secret://platform/name，可重复
        #[arg(long = "env", value_name = "NAME=REF", required = true)]
        envs: Vec<String>,
        /// -- 之后的命令
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
}

enum Backend {
    File(Store),
    D1(D1),
}

impl Backend {
    fn select() -> Result<Self> {
        let want_d1 = std::env::var("KYVAULT_BACKEND")
            .unwrap_or_default()
            .trim()
            .eq_ignore_ascii_case("d1");
        if want_d1 {
            Ok(Backend::D1(D1::from_env()?))
        } else {
            Ok(Backend::File(Store::default_location()?))
        }
    }

    fn get(&self, r: &str) -> Result<Option<String>> {
        match self {
            Backend::File(s) => s.get_secret(r),
            Backend::D1(d) => d.get_secret(r),
        }
    }

    fn set(&self, r: &str, v: &str, kind: &str, account: &str) -> Result<()> {
        match self {
            Backend::File(s) => s.set_secret(r, v),
            Backend::D1(d) => d.set_secret(r, v, kind, account),
        }
    }

    fn delete(&self, r: &str) -> Result<bool> {
        match self {
            Backend::File(s) => s.delete_secret(r),
            Backend::D1(d) => d.delete_secret(r),
        }
    }

    fn list(&self) -> Result<Vec<SecretMeta>> {
        match self {
            Backend::File(s) => Ok(s.list_secrets()),
            Backend::D1(d) => d.list_secrets(),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Backend::File(_) => "file",
            Backend::D1(_) => "d1",
        }
    }
}

fn read_value(v: &str) -> Result<String> {
    if v != "-" {
        return Ok(v.to_string());
    }
    // 从 stdin 读：密钥不进 argv，ps 看不到
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).context("从 stdin 读密钥失败")?;
    let v = buf.trim_end_matches(['\n', '\r']).to_string();
    if v.is_empty() {
        return Err(anyhow!("stdin 没读到内容"));
    }
    Ok(v)
}

fn print_list(items: &[SecretMeta], platform: Option<&str>, as_json: bool) -> Result<()> {
    let filtered: Vec<&SecretMeta> = items
        .iter()
        .filter(|m| platform.is_none_or(|p| m.platform == p))
        .collect();
    if as_json {
        println!("{}", serde_json::to_string_pretty(&filtered)?);
        return Ok(());
    }
    if filtered.is_empty() {
        println!("（没有密钥）");
        return Ok(());
    }
    let w = filtered.iter().map(|m| m.r#ref().chars().count()).max().unwrap_or(20).min(60);
    for m in filtered {
        let mut line = format!("{:<w$}  {:<14}", m.r#ref(), m.kind, w = w);
        if !m.last4.is_empty() {
            line.push_str(&format!("  …{}", m.last4));
        }
        if !m.account.is_empty() {
            line.push_str(&format!("  {}", m.account));
        }
        if !m.updated_at.is_empty() {
            line.push_str(&format!("  {}", m.updated_at));
        }
        println!("{}", line.trim_end());
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("错误: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init => {
            let store = Store::default_location()?;
            let existed = store.exists();
            store.init_master_key()?;
            println!(
                "master key 就绪（~/.keyring/master.key，0600）{}",
                if existed { "；已有密钥库，未改动" } else { "" }
            );
        }
        Cmd::Get { r#ref } => {
            let b = Backend::select()?;
            match b.get(&r#ref)? {
                Some(v) => println!("{v}"),
                None => return Err(anyhow!("找不到 {ref}（后端 {}）", b.name(), r#ref = r#ref)),
            }
        }
        Cmd::Set { r#ref, value, kind, account } => {
            let v = read_value(&value)?;
            let b = Backend::select()?;
            b.set(&r#ref, &v, &kind, &account)?;
            // 不回显明文，只确认写成功和它有多长
            println!("已写入 {} （{} 字节，后端 {}）", r#ref, v.len(), b.name());
        }
        Cmd::Annotate { r#ref, account, kind } => match Backend::select()? {
            Backend::D1(d) => {
                if d.annotate(&r#ref, account.as_deref(), kind.as_deref())? {
                    println!("已更新备注：{ref}", r#ref = r#ref);
                } else {
                    return Err(anyhow!("{ref} 不存在，没有改动任何东西", r#ref = r#ref));
                }
            }
            Backend::File(_) => {
                return Err(anyhow!(
                    "annotate 仅 D1 后端可用：本地 JSON 后端的 keys 结构里没有 account/kind 两列，\
                     没有地方可写。设 KYVAULT_BACKEND=d1 后重试。"
                ))
            }
        },
        Cmd::Delete { r#ref } => {
            let b = Backend::select()?;
            if b.delete(&r#ref)? {
                println!("已删除 {ref}", r#ref = r#ref);
            } else {
                return Err(anyhow!("{ref} 不存在", r#ref = r#ref));
            }
        }
        Cmd::List { platform, json } => {
            let b = Backend::select()?;
            print_list(&b.list()?, platform.as_deref(), json)?;
        }
        Cmd::Platforms => {
            let b = Backend::select()?;
            let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
            for m in b.list()? {
                *counts.entry(m.platform).or_default() += 1;
            }
            for (p, n) in counts {
                println!("{p:<24} {n}");
            }
        }
        Cmd::Run { envs, command } => {
            let b = Backend::select()?;
            let mut cmd = Command::new(&command[0]);
            cmd.args(&command[1..]);
            for spec in &envs {
                let (name, r) = spec
                    .split_once('=')
                    .ok_or_else(|| anyhow!("--env 应写成 NAME=secret://platform/name：{spec}"))?;
                let v = b
                    .get(r)?
                    .ok_or_else(|| anyhow!("找不到 {r}（后端 {}）", b.name()))?;
                cmd.env(name, v);
            }
            let status = cmd
                .status()
                .with_context(|| format!("执行 {} 失败", command[0]))?;
            std::process::exit(status.code().unwrap_or(1));
        }
    }
    Ok(())
}
