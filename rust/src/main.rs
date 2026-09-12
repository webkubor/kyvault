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
        /// 所属组织（如 hym / modelgo / museav / personal）—— 用来回答
        /// 「哪个公司有哪些机器人」，是过滤维度不是备注
        #[arg(long)]
        org: Option<String>,
        /// 权限范围，逗号分隔（如 im:message,bitable:record）——
        /// agent 拿到 key 之前就该知道自己能干什么，而不是试了才知道
        #[arg(long)]
        scopes: Option<String>,
    },
    /// 把连 D1 用的三件套存进本地加密库 —— 存完裸跑 kyvault 就能连真源，
    /// 不再需要外部注入环境变量（这是让调度系统不必持有密钥的前提）
    D1Setup {
        #[arg(long)]
        account_id: String,
        #[arg(long)]
        database_id: String,
        /// Cloudflare API Token。传 - 从 stdin 读，避免出现在 ps 和 history 里
        #[arg(long)]
        token: String,
    },
    /// 按「机器人身份」聚合查看 —— 一个机器人 = 一组凭据，不是散落的几条
    Identity {
        /// 只看某个组织（匹配备注里的组织名，如 hym / modelgo）
        #[arg(long)]
        org: Option<String>,
    },
    /// 删除密钥
    Delete { r#ref: String },
    /// 列出密钥元信息（不含明文）
    List {
        #[arg(long)]
        platform: Option<String>,
        /// 只看某个组织的（如 --org hym）
        #[arg(long)]
        org: Option<String>,
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
    /// 服务器资产台账（IP / root 密码 / 成本 / 云商）—— 仅本地 file 后端
    Server {
        /// set / get / list / delete
        action: String,
        hostname: Option<String>,
        ip: Option<String>,
        root_password: Option<String>,
        #[arg(long)]
        cost: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        /// get 时只读某个字段（ip / root-password / cost / provider）
        #[arg(long)]
        field: Option<String>,
    },
    /// CLI 多 Profile 凭证 —— 仅本地 file 后端
    Cli {
        /// set / get / list / delete
        action: String,
        cli_name: Option<String>,
        profile: Option<String>,
        token: Option<String>,
    },
    /// 交互式首次设置（逐条录密钥 + 建别名）
    Wizard,
    /// 把密钥使用规则/技能包写进本地各家 AI 助手
    Connect,
    /// 检查并升级到最新版（走 GitHub Release + install.sh）
    Update {
        /// 不问直接升（脚本/CI 用）
        #[arg(long = "yes", short = 'y')]
        assume_yes: bool,
    },
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

fn print_list(
    items: &[SecretMeta],
    platform: Option<&str>,
    org: Option<&str>,
    as_json: bool,
) -> Result<()> {
    let filtered: Vec<&SecretMeta> = items
        .iter()
        .filter(|m| platform.map_or(true, |p| m.platform == p))
        .filter(|m| org.map_or(true, |o| m.org == o))
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
        // org 放在 account 前面：回答「哪个公司的」比「备注写了啥」更常被问
        if !m.org.is_empty() {
            line.push_str(&format!("  [{}]", m.org));
        }
        if !m.account.is_empty() {
            line.push_str(&format!("  {}", m.account));
        }
        if !m.scopes.is_empty() {
            line.push_str(&format!("  scopes={}", m.scopes));
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
            org,
            scopes,
        } => match Backend::select()? {
            Backend::D1(d) => {
                if d.annotate(&r#ref, account.as_deref(), kind.as_deref(),
                              org.as_deref(), scopes.as_deref())? {
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
        Cmd::D1Setup {
            account_id,
            database_id,
            token,
        } => {
            // **刻意只写本地库**，不看 KYVAULT_BACKEND：这三件套是「怎么连 D1」，
            // 存进 D1 自己就成了鸡生蛋。本地库是它唯一能自举的地方。
            let tok = read_value(&token)?;
            let st = kyvault::store::Store::default_location()?;
            st.set_secret("secret://_kyvault/d1-account-id", &account_id)?;
            st.set_secret("secret://_kyvault/d1-database-id", &database_id)?;
            st.set_secret("secret://_kyvault/d1-token", &tok)?;
            println!("✅ D1 配置已存进本地加密库（~/.keyring，0600）");
            println!("   现在裸跑 `KYVAULT_BACKEND=d1 kyvault list` 就能连真源，不用再注入环境变量。");
        }
        Cmd::Identity { org } => {
            // 飞书/Lark 的机器人凭据是**一对**（app-id + app-secret），
            // 单独一条谁也用不了 —— lark-cli 要两个一起给才能装。
            // 但库里是拆成两条平铺的记录，list 出来看不出它们是一伙的，
            // 也看不出「这个机器人凭据齐不齐」。
            //
            // 这里按 name 去掉后缀聚合回「身份」：同一个前缀下的
            // app-id / app-secret / open-id / webhook 属于同一个机器人。
            const SUFFIXES: &[&str] = &[
                "-app-id", "-app-secret", "-open-id", "-allowed-users",
                "-webhook", "-encrypt-key", "-verification-token", "-bind-secret",
            ];
            let b = Backend::select()?;
            let mut groups: std::collections::BTreeMap<(String, String), Vec<(String, String)>> =
                Default::default();
            for m in b.list()? {
                if let Some(o) = org.as_deref() {
                    if !m.account.contains(o) && m.org != o {
                        continue;
                    }
                }
                let (base, part) = SUFFIXES
                    .iter()
                    .find_map(|sfx| m.name.strip_suffix(sfx).map(|b| (b.to_string(), sfx.trim_start_matches('-').to_string())))
                    .unwrap_or((m.name.clone(), "—".into()));
                groups
                    .entry((m.platform.clone(), base))
                    .or_default()
                    .push((part, m.account.clone()));
            }
            println!("{:<10} {:<26} {:<10} {}", "平台", "身份", "凭据", "组织/备注");
            for ((plat, name), parts) in &groups {
                let kinds: Vec<&str> = parts.iter().map(|(k, _)| k.as_str()).collect();
                // 能不能直接拿去装 lark-cli：必须 id 和 secret 都在
                let ready = kinds.contains(&"app-id") && kinds.contains(&"app-secret");
                let note = parts.iter().map(|(_, a)| a.as_str()).find(|a| !a.is_empty()).unwrap_or("");
                println!(
                    "{:<10} {:<26} {:<10} {}",
                    plat,
                    name,
                    if ready { "✅ 成对".to_string() } else { kinds.join(",") },
                    note
                );
            }
            println!("\n共 {} 个身份。✅ 表示 app-id + app-secret 齐全，可直接装进 lark-cli。", groups.len());
        }
        Cmd::Delete { r#ref } => {
            let b = Backend::select()?;
            if b.delete(&r#ref)? {
                println!("已删除 {ref}", r#ref = r#ref);
            } else {
                return Err(anyhow!("{ref} 不存在", r#ref = r#ref));
            }
        }
        Cmd::List { platform, org, json } => {
            let b = Backend::select()?;
            print_list(&b.list()?, platform.as_deref(), org.as_deref(), json)?;
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
        Cmd::Server {
            action,
            hostname,
            ip,
            root_password,
            cost,
            provider,
            field,
        } => {
            let st = Store::default_location()?;
            match action.as_str() {
                "set" => {
                    // 三个必填缺一个就拒 —— 半条台账比没有更坏：以后 get 出来
                    // 少个 root 密码，人会以为密码丢了而不是没存过
                    let (Some(h), Some(i), Some(pw)) = (hostname, ip, root_password) else {
                        return Err(anyhow!("set 需要 hostname、ip、root_password 三个参数"));
                    };
                    st.set_server(
                        &h,
                        &i,
                        &pw,
                        cost.as_deref().unwrap_or(""),
                        provider.as_deref().unwrap_or(""),
                    )?;
                    println!("✓ 服务器 {h} 已保存（本地 file 后端）");
                }
                "get" => {
                    let h = hostname.ok_or_else(|| anyhow!("get 需要 hostname"))?;
                    match st.get_server(&h, field.as_deref())? {
                        Some(fields) => {
                            // 单字段查询直接打值，方便 $(kyvault server get h --field ip)
                            if field.is_some() && fields.len() == 1 {
                                println!("{}", fields[0].1);
                            } else {
                                println!("🖥  {h}");
                                for (k, v) in fields {
                                    println!("  {k}: {v}");
                                }
                            }
                        }
                        None => return Err(anyhow!("未找到服务器 {h}（或该字段没存过）")),
                    }
                }
                "list" => {
                    let v = st.list_servers();
                    if v.is_empty() {
                        println!(
                            "（没有服务器台账 —— kyvault server set <主机名> <IP> <root密码>）"
                        );
                    } else {
                        println!("服务器台账（{} 台）：", v.len());
                        for h in v {
                            println!("  - {h}");
                        }
                    }
                }
                "delete" => {
                    let h = hostname.ok_or_else(|| anyhow!("delete 需要 hostname"))?;
                    if st.delete_server(&h)? {
                        println!("✓ 已删除服务器 {h}");
                    } else {
                        return Err(anyhow!("未找到服务器 {h}"));
                    }
                }
                a => return Err(anyhow!("未知操作 {a}（set / get / list / delete）")),
            }
        }
        Cmd::Cli {
            action,
            cli_name,
            profile,
            token,
        } => {
            let st = Store::default_location()?;
            match action.as_str() {
                "set" => {
                    let (Some(c), Some(pf), Some(t)) = (cli_name, profile, token) else {
                        return Err(anyhow!("set 需要 cli_name、profile、token 三个参数"));
                    };
                    st.set_cli_token(&c, &pf, &t)?;
                    println!("✓ {c} 的 profile {pf} 凭证已保存（本地 file 后端）");
                }
                "get" => {
                    let (Some(c), Some(pf)) = (cli_name, profile) else {
                        return Err(anyhow!("get 需要 cli_name 和 profile"));
                    };
                    match st.get_cli_token(&c, &pf)? {
                        // 不加换行：这个值通常被 $(...) 直接吃掉
                        Some(t) => print!("{t}"),
                        None => return Err(anyhow!("未找到 {pf}@{c}")),
                    }
                }
                "list" => {
                    let v = st.list_clis(cli_name.as_deref());
                    match (&cli_name, v.is_empty()) {
                        (_, true) => println!("（没有 CLI 凭证登记）"),
                        (Some(c), false) => {
                            println!("{c} 的 profile（{} 个）：", v.len());
                            for p in v {
                                println!("  - {p}");
                            }
                        }
                        (None, false) => {
                            println!("已登记的 CLI（{} 个）：", v.len());
                            for c in v {
                                println!("  - {c}");
                            }
                        }
                    }
                }
                "delete" => {
                    let (Some(c), Some(pf)) = (cli_name, profile) else {
                        return Err(anyhow!("delete 需要 cli_name 和 profile"));
                    };
                    if st.delete_cli_token(&c, &pf)? {
                        println!("✓ 已删除 {pf}@{c}");
                    } else {
                        return Err(anyhow!("未找到 {pf}@{c}"));
                    }
                }
                a => return Err(anyhow!("未知操作 {a}（set / get / list / delete）")),
            }
        }
        Cmd::Wizard => {
            let b = Backend::select()?;
            let entries = kyvault::wizard::run(b.name())?;
            let aliases = Aliases::default_location()?;
            for e in &entries {
                b.set(&e.r#ref, &e.value, &e.kind, &e.account)?;
                println!("✓ 已保存 {}", e.r#ref);
                if let Some(a) = &e.alias {
                    aliases.set(a, &e.r#ref)?;
                    println!("  别名 {a} → {}", e.r#ref);
                }
            }
            if !entries.is_empty() {
                println!("\n共 {} 条已落到后端 {}", entries.len(), b.name());
            }
        }
        Cmd::Connect => kyvault::connect::run()?,
        Cmd::Update { assume_yes } => kyvault::update::run(assume_yes)?,
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
                println!(
                    "没有可导入的变量（{file}，前缀 {:?}）—— 空值会被跳过",
                    prefix
                );
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
            println!(
                "check 支持的平台（{} 个）：\n",
                kyvault::providers::PROVIDERS.len()
            );
            for p in kyvault::providers::PROVIDERS {
                println!("  {} {:<14} {:<28} {}", p.logo, p.id, p.name, p.env_key);
            }
            println!(
                "\n用法：kyvault check <平台> <密钥名>   或   kyvault check <平台> --key <明文>"
            );
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
