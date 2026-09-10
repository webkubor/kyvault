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

use kyvault::alias::Aliases;
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
    /// 管理账户（用户名+密码）—— 仅本地 file 后端
    Account {
        /// set / get / list / delete
        action: String,
        platform: String,
        username: Option<String>,
        password: Option<String>,
    },
    /// 管理平台密钥 —— 仅本地 file 后端
    Key {
        /// set / get / list / delete
        action: String,
        platform: String,
        key_name: Option<String>,
        value: Option<String>,
    },
    /// 查看平台列表和详情 —— 仅本地 file 后端
    Platform { platform_name: Option<String> },
    /// 管理别名（人设好映射，AI 只用别名）
    Alias {
        #[command(subcommand)]
        action: AliasCmd,
    },
    /// 验证 API Key 是否还有效（顺带查余额）—— 对应密钥红线「失效就覆写」
    Check {
        /// 平台名（kyvault providers 看清单）
        provider: String,
        /// 库里的密钥名；不给就必须传 --key
        key_name: Option<String>,
        /// 直接给明文 key（不落库、只验一次）
        #[arg(long = "key")]
        api_key: Option<String>,
    },
    /// 列出 check 支持的平台
    Providers,
    /// 自检：后端/环境变量/文件权限/解密/AI 助手对接
    Doctor,
    /// 从 .env 批量导入密钥
    Import {
        #[arg(long, short = 'f', default_value = ".env")]
        file: String,
        /// 只导入以此开头的变量，如 GITHUB_
        #[arg(long, short = 'p', default_value = "")]
        prefix: String,
        /// 只打印会导入什么，不写库不建别名
        #[arg(long = "dry-run", short = 'n')]
        dry_run: bool,
        /// 不自动建别名（默认建，别名 = 变量名小写）
        #[arg(long = "no-alias")]
        no_alias: bool,
    },
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

#[derive(Subcommand)]
enum AliasCmd {
    /// 设置别名：github_token → secret://github/my-pat
    Set { name: String, r#ref: String },
    /// 查别名指向哪个 ref
    Get { name: String },
    /// 列出所有别名
    List,
    /// 删除别名
    Delete { name: String },
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
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("从 stdin 读密钥失败")?;
    let v = buf.trim_end_matches(['\n', '\r']).to_string();
    if v.is_empty() {
        return Err(anyhow!("stdin 没读到内容"));
    }
    Ok(v)
}

fn print_list(items: &[SecretMeta], platform: Option<&str>, as_json: bool) -> Result<()> {
    let filtered: Vec<&SecretMeta> = items
        .iter()
        .filter(|m| platform.map_or(true, |p| m.platform == p))
        .collect();
    if as_json {
        println!("{}", serde_json::to_string_pretty(&filtered)?);
        return Ok(());
    }
    if filtered.is_empty() {
        println!("（没有密钥）");
        return Ok(());
    }
    let w = filtered
        .iter()
        .map(|m| m.r#ref().chars().count())
        .max()
        .unwrap_or(20)
        .min(60);
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
                if existed {
                    "；已有密钥库，未改动"
                } else {
                    ""
                }
            );
        }
        Cmd::Get { r#ref } => {
            // 先解析别名 —— 老用户脚本里写的多是 `kyvault get github_token`
            let r#ref = Aliases::default_location()?.resolve(&r#ref);
            let b = Backend::select()?;
            match b.get(&r#ref)? {
                Some(v) => println!("{v}"),
                None => return Err(anyhow!("找不到 {ref}（后端 {}）", b.name(), r#ref = r#ref)),
            }
        }
        Cmd::Set {
            r#ref,
            value,
            kind,
            account,
        } => {
            let v = read_value(&value)?;
            let b = Backend::select()?;
            b.set(&r#ref, &v, &kind, &account)?;
            // 不回显明文，只确认写成功和它有多长
            println!("已写入 {} （{} 字节，后端 {}）", r#ref, v.len(), b.name());
        }
        Cmd::Annotate {
            r#ref,
            account,
            kind,
        } => match Backend::select()? {
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
        // account / key / platform 只有本地 file 后端有（D1 那张表是扁平的
        // id→密文，没有 accounts 这个桶），所以这里直接用 Store，不走后端分发
        Cmd::Account {
            action,
            platform,
            username,
            password,
        } => {
            let s = Store::default_location()?;
            match action.as_str() {
                "set" => {
                    let u = username.ok_or_else(|| anyhow!("set 需要 username"))?;
                    let p = read_value(
                        &password
                            .ok_or_else(|| anyhow!("set 需要 password（传 - 从 stdin 读）"))?,
                    )?;
                    s.set_account(&platform, &u, &p)?;
                    println!("已写入账户 {platform}/{u}（{} 字节）", p.len());
                }
                "get" => {
                    let u = username.ok_or_else(|| anyhow!("get 需要 username"))?;
                    match s.get_account(&platform, &u)? {
                        Some(v) => println!("{v}"),
                        None => return Err(anyhow!("找不到账户 {platform}/{u}")),
                    }
                }
                "list" => {
                    let list = s.list_bucket(&platform, "accounts");
                    if list.is_empty() {
                        println!("（没有账户）");
                    }
                    for u in list {
                        println!("  {u}");
                    }
                }
                "delete" => {
                    let u = username.ok_or_else(|| anyhow!("delete 需要 username"))?;
                    if s.delete_from_bucket(&platform, "accounts", &u)? {
                        println!("已删除账户 {platform}/{u}");
                    } else {
                        return Err(anyhow!("找不到账户 {platform}/{u}"));
                    }
                }
                other => return Err(anyhow!("未知操作 {other}，应为 set / get / list / delete")),
            }
        }
        Cmd::Key {
            action,
            platform,
            key_name,
            value,
        } => {
            let s = Store::default_location()?;
            match action.as_str() {
                "set" => {
                    let n = key_name.ok_or_else(|| anyhow!("set 需要 key_name"))?;
                    let v = read_value(
                        &value.ok_or_else(|| anyhow!("set 需要 value（传 - 从 stdin 读）"))?,
                    )?;
                    s.set_key(&platform, &n, &v)?;
                    println!("已写入 {platform}/{n}（{} 字节）", v.len());
                }
                "get" => {
                    let n = key_name.ok_or_else(|| anyhow!("get 需要 key_name"))?;
                    match s.get_key(&platform, &n)? {
                        Some(v) => println!("{v}"),
                        None => return Err(anyhow!("找不到 {platform}/{n}")),
                    }
                }
                "list" => {
                    let list = s.list_bucket(&platform, "keys");
                    if list.is_empty() {
                        println!("（没有密钥）");
                    }
                    for n in list {
                        println!("  {n}");
                    }
                }
                "delete" => {
                    let n = key_name.ok_or_else(|| anyhow!("delete 需要 key_name"))?;
                    if s.delete_from_bucket(&platform, "keys", &n)? {
                        println!("已删除 {platform}/{n}");
                    } else {
                        return Err(anyhow!("找不到 {platform}/{n}"));
                    }
                }
                other => return Err(anyhow!("未知操作 {other}，应为 set / get / list / delete")),
            }
        }
        Cmd::Platform { platform_name } => {
            let s = Store::default_location()?;
            let all = s.platforms();
            match platform_name {
                Some(p) => {
                    let Some((accounts, keys)) = all.get(&p) else {
                        return Err(anyhow!("找不到平台 {p}"));
                    };
                    println!("平台 {p}");
                    println!("  账户 ({}):", accounts.len());
                    for a in accounts {
                        println!("    {a}");
                    }
                    println!("  密钥 ({}):", keys.len());
                    for k in keys {
                        println!("    {k}");
                    }
                }
                None => {
                    if all.is_empty() {
                        println!("（没有平台）");
                    }
                    for (p, (accounts, keys)) in all {
                        println!("{p:<24} 账户 {:<3} 密钥 {}", accounts.len(), keys.len());
                    }
                }
            }
        }
        Cmd::Alias { action } => {
            let a = Aliases::default_location()?;
            match action {
                AliasCmd::Set { name, r#ref } => {
                    a.set(&name, &r#ref)?;
                    println!("✓ {name} → {ref}", r#ref = r#ref);
                }
                AliasCmd::Get { name } => match a.get(&name) {
                    Some(r) => println!("{r}"),
                    None => return Err(anyhow!("未找到别名：{name}")),
                },
                AliasCmd::List => {
                    let m = a.load();
                    if m.is_empty() {
                        println!("（空）");
                    }
                    for (n, r) in m {
                        println!("  {n} → {r}");
                    }
                }
                AliasCmd::Delete { name } => {
                    if a.delete(&name)? {
                        println!("✓ 已删除别名：{name}");
                    } else {
                        return Err(anyhow!("未找到别名：{name}"));
                    }
                }
            }
        }
        Cmd::Doctor => kyvault::doctor::run()?,
        Cmd::Import {
            file,
            prefix,
            dry_run,
            no_alias,
        } => {
            use kyvault::import_env as ie;
            let text = ie::read_file(&file)?;
            let planned = ie::plan(&text, &prefix);
            if planned.is_empty() {
                println!("没有可导入的变量（{file}，前缀 {:?}）—— 空值会被跳过", prefix);
                return Ok(());
            }
            if dry_run {
                println!("预览（不写库）：");
                for p in &planned {
                    println!("  {} → {} [{}]", p.env_key, p.r#ref, p.kind);
                }
                println!("\n共 {} 条。去掉 --dry-run 真导入。", planned.len());
                return Ok(());
            }
            let b = Backend::select()?;
            let aliases = Aliases::default_location()?;
            for p in &planned {
                b.set(&p.r#ref, &p.value, p.kind, "")?;
                println!("✓ {} → {}", p.env_key, p.r#ref);
                if !no_alias {
                    aliases.set(&p.env_key.to_lowercase(), &p.r#ref)?;
                }
            }
            println!("\n共导入 {} 条（后端 {}）", planned.len(), b.name());
        }
        Cmd::Providers => {
            println!("check 支持的平台（{} 个）：\n", kyvault::providers::PROVIDERS.len());
            for p in kyvault::providers::PROVIDERS {
                println!("  {} {:<14} {:<28} {}", p.logo, p.id, p.name, p.env_key);
            }
            println!("\n用法：kyvault check <平台> <密钥名>   或   kyvault check <平台> --key <明文>");
        }
        Cmd::Check {
            provider,
            key_name,
            api_key,
        } => {
            // 三种来源，优先级同 Python 版：--key > 库里的 key_name > 报错
            // 不做「按平台猜唯一一把 key」那种便利：猜错了会去验错的 key，
            // 然后把一把好 key 判成失效 —— 这类便利的代价比省下的一次输入大得多。
            let key = match (api_key, key_name) {
                (Some(k), _) => k,
                (None, Some(name)) => {
                    let r#ref = format!("secret://{provider}/{name}");
                    let b = Backend::select()?;
                    b.get(&r#ref)?
                        .ok_or_else(|| anyhow!("找不到 {}（后端 {}）", r#ref, b.name()))?
                }
                (None, None) => {
                    return Err(anyhow!(
                        "要么给库里的密钥名，要么用 --key 传明文：kyvault check {provider} <密钥名>"
                    ))
                }
            };
            let r = kyvault::providers::validate_key(&provider, &key);
            println!("{}", r.message);
            if !r.models.is_empty() {
                println!("  可用模型：{}", r.models.join(", "));
            }
            if let Some(b) = r.balance {
                println!("  {b}");
            }
            // 退出码要能被脚本用：无效返回 1，这样 `kyvault check x y || 轮换` 成立
            if !r.valid {
                std::process::exit(1);
            }
        }
        Cmd::Run { envs, command } => {
            let b = Backend::select()?;
            let aliases = Aliases::default_location()?;
            let mut cmd = Command::new(&command[0]);
            cmd.args(&command[1..]);
            for spec in &envs {
                let (name, r) = spec
                    .split_once('=')
                    .ok_or_else(|| anyhow!("--env 应写成 NAME=secret://platform/name：{spec}"))?;
                let r = aliases.resolve(r);
                let v = b
                    .get(&r)?
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
