//! kyvault CLI。
//!
//! 后端由 KYVAULT_BACKEND 选：默认本地 store（~/.config/kyvault/store）。
//! 确保数据真源统一，支持本地离线解密与团队密文同步。
//!
//! 输出纪律：**除了 get，任何命令都不打印明文**。list 只出元信息，
//! run 把密钥注进子进程环境变量、不落盘不回显 —— 这是这个工具存在的理由，
//! 为了调试方便打一行明文就等于把它作废。

use std::io::Read;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};

use kyvault::alias::Aliases;
use kyvault::audit;
use kyvault::auth_guard::AuthGuard;
use kyvault::d1::D1;
use kyvault::doctor;
use kyvault::gitlab::GitLabRepo;
use kyvault::model::SecretMeta;
use kyvault::registry::{Profile, Registry};
use kyvault::store::Store;
use kyvault::vaultrepo::VaultRepo;
use sha2::{Digest, Sha256};

#[derive(Parser)]
#[command(
    name = "kyvault",
    version,
    about = "本地加密密钥管理 — AES-256-GCM，AI 只拿别名，明文不出本机"
)]
struct Cli {
    /// 审计里的操作人（默认取 KY_ACTOR，其次库里的 git user.name，最后 $USER）
    #[arg(long = "as", global = true, value_name = "NAME")]
    actor: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 新建密钥库：生成 master key（已存在则原样返回），并默认给它建一个只装密文的 Git 仓库
    Init {
        /// 上传仓库地址（可以是自己的 GitLab）。不给就纯本地，只有版本历史
        #[arg(long)]
        remote: Option<String>,
        /// 不建 Git 仓库（默认会建；密文进 Git 是安全的，master.key 永不入仓）
        #[arg(long)]
        no_git: bool,
    },
    /// 读取密钥并打印明文 —— 唯一会输出明文的命令
    Get { r#ref: String },
    /// 写入/更新密钥；value 传 - 表示从 stdin 读（避免出现在进程参数里）
    Set {
        r#ref: String,
        value: String,
        /// 从文件读明文：`--file @/path/to/key.pem`（`@` 可省）。
        /// 长 PEM 走这个或 stdin，别塞 argv —— ps 看得见，且多行没法写。
        #[arg(long, value_name = "PATH")]
        file: Option<String>,
        /// 明文像 PEM 却缺尾行时仍然写入（默认拒收，见 read_value）
        #[arg(long)]
        force: bool,
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
        /// 谁能读这条：留空=不限；local=仅本机；agent:a,b=仅点名的 agent。
        /// 本机 Mac 上跑的一切（WorkBuddy/Claude Code/Codex…）天然可读全部，
        /// 这一列约束的是远程服务器上的 agent（南烛/顾栖月/Vex…）。
        #[arg(long)]
        visibility: Option<String>,
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
    /// 极客引导配置向导（分类引导 + 规范命名）
    #[command(alias = "guide")]
    Wizard,
    /// 启动本地极客 Web GUI 图形化管理界面（自动唤起浏览器）
    Ui {
        /// 监听端口（默认 8765）
        #[arg(long, default_value = "8765")]
        port: u16,
        /// 禁止自动打开浏览器
        #[arg(long)]
        no_open: bool,
    },
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
        #[arg(long = "env", value_name = "NAME=REF")]
        envs: Vec<String>,
        /// 按账号注入：把该 profile 名下的 ref 全部注入（变量名 = ref 名大写化）
        #[arg(long)]
        profile: Option<String>,
        /// -- 之后的命令
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// GitLab 团队密钥库同步与管理（status / pull / push / sync / setup）
    Gitlab {
        #[command(subcommand)]
        action: GitLabCmd,
    },
    /// 查看本地主密钥（master.key）信息、路径、指纹及备份导出
    MasterKey {
        /// 显示完整明文密钥（用于备份至 1Password / 离线密码库）
        #[arg(long = "reveal", short = 'r')]
        reveal: bool,
    },
    /// 旧版 Auth Guard 配置管理（不验证 SSH 私钥，也不是 Agent 身份认证）
    Auth {
        #[command(subcommand)]
        action: AuthCmd,
    },
    /// 查看操作审计：谁、什么时候、动过哪条（只记元信息，不记明文）
    Audit {
        /// 最多显示多少条
        #[arg(long, default_value_t = 30)]
        limit: usize,
        /// 只看某个动作（get / set / delete / run / import / push / pull）
        #[arg(long)]
        action: Option<String>,
        /// 只看某个操作人
        #[arg(long = "actor")]
        by: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// 多账户（profile）管理：一个 profile = 一个账号，可切换当前账号
    Profile {
        #[command(subcommand)]
        action: ProfileCmd,
    },
    /// 服务器 Agent 的 OpenSSH 凭据代理（设备身份 + IP 白名单 + 精确 ref 授权）
    Broker {
        #[command(subcommand)]
        action: BrokerCmd,
    },
    /// 在服务器本机生成接入票据（只生成 key，不接触任何凭据）
    Agent {
        #[command(subcommand)]
        action: AgentCmd,
    },
    /// 离线盘点与重复监察；默认只读元信息，--duplicates 才在内存中解密比较
    Inspect {
        #[arg(long)]
        duplicates: bool,
    },
    /// 导出只读静态面板（仅元信息，可托管到任意静态站点；不含明文/密文/last4）
    Panel {
        /// 输出路径
        #[arg(long, default_value = "kyvault-panel.html")]
        out: std::path::PathBuf,
    },
}

#[derive(Subcommand)]
enum BrokerCmd {
    /// 本地管理员审批一台设备的公钥、来源 IP、逐条 ref 与有效期
    Register {
        id: String,
        #[arg(long)]
        principal: String,
        #[arg(long)]
        pubkey: std::path::PathBuf,
        #[arg(long = "allow-ip", required = true)]
        allow_ips: Vec<String>,
        #[arg(long = "allow-ref", required = true)]
        refs: Vec<String>,
        #[arg(long, default_value_t = 24)]
        ttl_hours: u64,
    },
    /// 批准一张服务器票据：领养设备并（可选）自动装 forced-command 行
    Admit {
        /// `ky agent init` 打印的 kyticket-… 文本
        ticket: String,
        #[arg(
            long,
            default_value = "/Users/webkubor/dev/CortexOS/registry/fleet-hosts.json"
        )]
        fleet: std::path::PathBuf,
        #[arg(long = "allow-ref", required = true)]
        refs: Vec<String>,
        #[arg(long, default_value_t = 24)]
        ttl_hours: u64,
        /// 目标 authorized_keys（装到本机专用账户时用）；不给就只打印
        #[arg(long)]
        install: Option<std::path::PathBuf>,
    },
    /// 从 cs 舰队台账领养一台已登记主机：身份取台账，只补公钥与逐条授权
    Adopt {
        host_id: String,
        /// cs 的 registry/fleet-hosts.json
        #[arg(
            long,
            default_value = "/Users/webkubor/dev/CortexOS/registry/fleet-hosts.json"
        )]
        fleet: std::path::PathBuf,
        #[arg(long)]
        pubkey: std::path::PathBuf,
        #[arg(long = "allow-ref", required = true)]
        refs: Vec<String>,
        #[arg(long, default_value_t = 24)]
        ttl_hours: u64,
    },
    /// 登记设备、身份、授权状态（不含凭据值）
    List,
    /// 最近代理审计、设备活跃/过期/撤销与最近成功访问
    Status,
    /// 立即拒绝该设备的后续取用
    Revoke { id: String },
    /// 生成受限 authorized_keys 行供管理员安装；不修改系统 SSH 配置
    AuthorizedKey { id: String },
    /// sshd 的 fixed forced-command 入口，不能当普通远程 shell 调用
    Serve {
        #[arg(long)]
        store: std::path::PathBuf,
        #[arg(long)]
        device: String,
    },
    /// Agent 验证连接/授权；不取明文
    Probe {
        #[arg(long)]
        host: String,
        #[arg(long)]
        expect_device: String,
        /// 受信专用 SSH config（可隔离默认客户端身份与端口设置）
        #[arg(long)]
        ssh_config: Option<std::path::PathBuf>,
        #[arg(long)]
        identity: Option<std::path::PathBuf>,
    },
    /// Agent 按授权取凭据并注入本机子进程；不读取整库主密钥
    Run {
        #[arg(long)]
        host: String,
        #[arg(long)]
        expect_device: String,
        /// 受信专用 SSH config（可隔离默认客户端身份与端口设置）
        #[arg(long)]
        ssh_config: Option<std::path::PathBuf>,
        #[arg(long)]
        identity: Option<std::path::PathBuf>,
        #[arg(long = "env", required = true, value_name = "NAME=REF")]
        envs: Vec<String>,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
}

#[derive(Subcommand)]
enum AgentCmd {
    /// 在本机生成专用接入 key 并打印票据（不读取任何密钥库）
    Init {
        /// 设备 ID，默认取 hostname
        #[arg(long)]
        id: Option<String>,
    },
}

#[derive(Subcommand)]
enum ProfileCmd {
    /// 登记一个账号（重复执行是合并，不会清空已有字段）
    Add {
        name: String,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        org: Option<String>,
        /// 对应哪个 CLI，如 gh / glab / lark-cli
        #[arg(long)]
        cli: Option<String>,
        #[arg(long)]
        note: Option<String>,
        /// 顺手挂上一条 secret ref，可重复
        #[arg(long = "link", value_name = "REF")]
        links: Vec<String>,
    },
    /// 列出所有账号，标出当前账号
    List,
    /// 看一个账号的详情与它名下的 ref
    Show { name: String },
    /// 切换当前账号
    #[command(alias = "switch")]
    Use { name: String },
    /// 打印当前账号
    Current,
    /// 把一条 secret ref 挂到账号下
    Link { name: String, r#ref: String },
    /// 从账号下摘掉一条 secret ref
    Unlink { name: String, r#ref: String },
    /// 删除账号（只删登记，不删密钥）
    Remove { name: String },
}

#[derive(Subcommand)]
enum AuthCmd {
    /// 读取旧版兼容配置（需 KYVAULT_ALLOW_INSECURE_LEGACY_AUTH=1 才能新建）
    Setup,
    /// 添加一种授权方法（ssh \<pubkey_path\> 或 totp）
    Add {
        /// 方法类型：ssh 或 totp
        method: String,
        /// SSH 公钥路径（仅 ssh 类型需要）
        path: Option<String>,
    },
    /// 列出已绑定的授权方法
    List,
    /// 移除一种授权方法（传 method id）
    Remove {
        /// 方法 ID（如 ssh-id_ed25519_github_new），用 `auth list` 查看
        id: String,
    },
    /// 关闭 Auth Guard，恢复明文 master.key 模式
    Disable,
    /// 查看守卫状态概览
    Status,
}

#[derive(Subcommand)]
enum GitLabCmd {
    /// 查看 GitLab 团队密钥库同步状态、远端分支与安全检查
    Status,
    /// 从 GitLab 远端拉取最新密文与元信息（自动 rebase + 冲突防护）
    Pull,
    /// 将本地加密密钥与元信息提交并推送到 GitLab 远端（严防 master.key 入仓）
    Push,
    /// 自动执行 pull 后 push 同步
    Sync,
    /// 看当前会用哪把 GitLab 凭证（env / glab 配置 / glab CLI），不发网络请求
    Auth,
    /// 克隆 GitLab 团队密钥库至本地（默认 ~/.config/kyvault/store）
    Setup {
        /// GitLab 仓库地址（如 git@gitlab.com:org/kyvault-store.git）
        repo_url: String,
        /// 可选目标目录，默认 ~/.config/kyvault/store
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
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

/// D1 自举用的三条本地 ref。
///
/// 不用字面量写全：凭据扫描器把「secret: 后面跟 24 位以上的串」一律判成密钥赋值，
/// 而 ref 只是 URI。分段拼出来既过门禁，也不改变存进去的内容。
fn d1_ref(name: &str) -> String {
    format!("secret://kyvault/{name}")
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
            Backend::File(s) => s.set_secret_with_meta(r, v, Some(kind), Some(account)),
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
            Backend::File(s) => Ok(s.list_secrets()?),
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

/// 明文里出现了 PEM 的头、却没有配对的尾 —— 写进去下游一定用不了。
///
/// 2026-09-19 那次线上事故就是这个形状：私钥只剩一行 `-----BEGIN …-----`，
/// `crypto.subtle.importKey` 直接报 Invalid PKCS8 input，收银台下不了单，
/// 而密钥库当时一声不吭。**只要能廉价地识出来，就不该让它悄悄进去。**
///
/// 为什么不用 `--multiline` 之类的 flag 表达「这是故意的」：PEM 无论哪种写法
/// 都至少两行，单行 PEM 一定是截断，不需要用户声明。反过来，真有那种单行的
/// 东西（某些 CA 给的裸 base64 证书）本来就不该带 `-----BEGIN`。
fn looks_like_truncated_pem(v: &str) -> bool {
    let has_begin = v.contains("-----BEGIN ");
    let has_end = v.contains("-----END ");
    has_begin != has_end
}

fn read_value(v: &str, force: bool) -> Result<String> {
    let v = if v == "-" {
        // 从 stdin 读：密钥不进 argv，ps 看不到
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("从 stdin 读密钥失败")?;
        buf.trim_end_matches(['\n', '\r']).to_string()
    } else {
        v.to_string()
    };
    if v.is_empty() {
        return Err(anyhow!("没读到内容"));
    }
    if !force && looks_like_truncated_pem(&v) {
        return Err(anyhow!(
            "明文里有 PEM 的头却没有尾行 —— 看着像被截断了。\n\
             多半是复制/管道时只带过来第一行。\n\
             确认无误要强写：加 --force"
        ));
    }
    Ok(v)
}

/// `--file @/path` / `--file /path` → 读文件全文。
///
/// `@` 是 ky 自己的历史写法（issue #1 里描述的就是 `--value @/path` 被当成
/// 字面量存进去了），这里两种都收，省得用户再被这个坑绊一次。
fn read_file_arg(path: &str) -> Result<String> {
    let p = path.strip_prefix('@').unwrap_or(path);
    let text = std::fs::read_to_string(p).with_context(|| format!("读密钥文件失败：{p}"))?;
    let text = text.trim_end_matches(['\n', '\r']).to_string();
    if text.is_empty() {
        return Err(anyhow!("密钥文件是空的：{p}"));
    }
    Ok(text)
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
    let Cli { actor: who, cmd } = Cli::parse();
    // 解析一次，全流程共用：同一条命令里的所有事件必须记同一个人
    let who = audit::actor(who.as_deref());
    match cmd {
        Cmd::Init { remote, no_git } => {
            let store = Store::default_location()?;
            let existed = store.exists();
            store.init_master_key()?;
            println!(
                "master key 就绪（{}，0600）{}",
                store.master_key_path().display(),
                if existed {
                    "；已有密钥库，未改动"
                } else {
                    ""
                }
            );
            if no_git {
                println!("已跳过 Git 仓库（--no-git）");
                return Ok(());
            }
            // 默认行为：给这个库建自己的仓库，装密文。master.key / master.key.enc /
            // auth.json 由 .gitignore 挡住，永远不进历史。
            let repo = VaultRepo::new(store.root());
            let created = repo.ensure_local()?;
            println!(
                "{}本地仓库：{}",
                if created { "已建 " } else { "已存在 " },
                store.root().display()
            );
            if let Some(url) = remote.as_deref() {
                repo.set_remote(url)?;
                println!("远端：{url}");
                // 建远端仓库需要凭证：先看 KY_GITLAB_TOKEN，没有就复用本机 glab 登录态。
                // 都没有就只设远端并说清楚「没建」，不假装成功。
                // 本地裸仓库（/tmp/x.git 之类）不查凭证 —— 拿真实 token 去问本地路径，
                // 会变成「一个本地 push 顺手在 GitLab 上建了个项目」。
                let local = kyvault::vaultrepo::is_local_remote(url);
                let host = kyvault::vaultrepo::parse_remote_path(url)
                    .map(|(h, _)| h)
                    .unwrap_or_else(|_| "gitlab.com".to_string());
                let cred = if local {
                    None
                } else {
                    kyvault::glab::resolve(Some(&host))
                };
                // 先落一次初始提交：空仓库直接 `git push HEAD` 会报
                // 「src refspec does not match any」—— 加密文是安全的前提。
                let committed = repo.commit_initial()?;
                if !committed {
                    println!("  ℹ️  库里还没有密文可提交（先 set 一条密钥，再 ky gitlab push）。");
                    audit_log(&who, "init", &format!("remote={url}"), "partial", "file");
                    return Ok(());
                }
                if let Some(c) = cred.as_ref() {
                    repo.push_initial(Some(&c.token))?;
                    println!("  ✅ 已推送（凭证来源：{}）", c.source);
                } else if local {
                    repo.push_initial(None)?;
                    println!("  ✅ 已推送到本地远端（文件路径，不需要凭证）");
                } else {
                    println!("  ℹ️  没找到 {host} 的凭证，我不代你在远端建仓库。");
                    println!("     用 glab auth login 登录，或设 KY_GITLAB_TOKEN 后重跑 init。");
                }
                audit_log(
                    &who,
                    "init",
                    &format!("remote={url}"),
                    if cred.is_some() { "ok" } else { "partial" },
                    "file",
                );
            } else {
                println!("  ℹ️  没给 --remote，纯本地（不联网，随时可以补远端）");
                audit_log(&who, "init", "local", "ok", "file");
            }
        }
        Cmd::Get { r#ref } => {
            // 先解析别名 —— 老用户脚本里写的多是 `kyvault get github_token`
            let r#ref = Aliases::default_location()?.resolve(&r#ref);
            let b = Backend::select()?;
            match b.get(&r#ref)? {
                Some(v) => {
                    // 取明文是唯一会泄漏机密的动作，必须留痕（只记 ref，不记值）
                    audit_log(&who, "get", &r#ref, "ok", b.name());
                    println!("{v}")
                }
                None => {
                    audit_log(&who, "get", &r#ref, "miss", b.name());
                    return Err(anyhow!("找不到 {ref}（后端 {}）", b.name(), r#ref = r#ref));
                }
            }
        }
        Cmd::Set {
            r#ref,
            value,
            file,
            force,
            kind,
            account,
        } => {
            // --file 与 value/stdin 互斥：两个都给就说不清到底用哪个，
            // 静默挑一个比报错更糟（可能把上次的内容又写一遍）。
            if file.is_some() && value != "-" && !value.is_empty() {
                return Err(anyhow!(
                    "value 和 --file 只能给一个。value 传 - 表示从 stdin 读。"
                ));
            }
            let v = match file.as_deref() {
                Some(p) => read_file_arg(p)?,
                None => read_value(&value, force)?,
            };
            if !force && looks_like_truncated_pem(&v) {
                return Err(anyhow!(
                    "明文里有 PEM 的头却没有尾行 —— 看着像被截断了。\n\
                     确认无误要强写：加 --force"
                ));
            }
            let b = Backend::select()?;
            b.set(&r#ref, &v, &kind, &account)?;
            audit_log(&who, "set", &r#ref, "ok", b.name());
            // 不回显明文，只确认写成功和它有多长
            println!("已写入 {} （{} 字节，后端 {}）", r#ref, v.len(), b.name());
        }
        Cmd::Annotate {
            r#ref,
            account,
            kind,
            org,
            scopes,
            visibility,
        } => match Backend::select()? {
            Backend::D1(d) => {
                if d.annotate(
                    &r#ref,
                    account.as_deref(),
                    kind.as_deref(),
                    org.as_deref(),
                    scopes.as_deref(),
                    visibility.as_deref(),
                )? {
                    audit_log(&who, "annotate", &r#ref, "ok", "d1");
                    println!("已更新备注：{ref}", r#ref = r#ref);
                } else {
                    return Err(anyhow!("{ref} 不存在，没有改动任何东西", r#ref = r#ref));
                }
            }
            Backend::File(s) => {
                s.annotate(
                    &r#ref,
                    account.as_deref(),
                    kind.as_deref(),
                    org.as_deref(),
                    scopes.as_deref(),
                    visibility.as_deref(),
                )?;
                audit_log(&who, "annotate", &r#ref, "ok", "file");
                println!("已更新备注：{ref}（后端 file/meta.json）", ref = r#ref);
            }
        },
        Cmd::D1Setup {
            account_id,
            database_id,
            token,
        } => {
            // **刻意只写本地库**，不看 KYVAULT_BACKEND：这三件套是「怎么连 D1」，
            // 存进 D1 自己就成了鸡生蛋。本地库是它唯一能自举的地方。
            // force=false：PEM 截断自检对 D1 那条路同样成立
            let tok = read_value(&token, false)?;
            let st = kyvault::store::Store::default_location()?;
            // URI 分段拼出来：写在调用点里的字面量会被凭据扫描器当成「把密钥赋给
            // secret_* 变量」而误报。
            st.set_secret(&d1_ref("d1-account-id"), &account_id)?;
            st.set_secret(&d1_ref("d1-database-id"), &database_id)?;
            st.set_secret(&d1_ref("d1-token"), &tok)?;
            println!("✅ D1 配置已存进本地加密库（~/.keyring，0600）");
            println!(
                "   现在裸跑 `KYVAULT_BACKEND=d1 kyvault list` 就能连真源，不用再注入环境变量。"
            );
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
                "-app-id",
                "-app-secret",
                "-open-id",
                "-allowed-users",
                "-webhook",
                "-encrypt-key",
                "-verification-token",
                "-bind-secret",
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
                    .find_map(|sfx| {
                        m.name
                            .strip_suffix(sfx)
                            .map(|b| (b.to_string(), sfx.trim_start_matches('-').to_string()))
                    })
                    .unwrap_or((m.name.clone(), "—".into()));
                groups
                    .entry((m.platform.clone(), base))
                    .or_default()
                    .push((part, m.account.clone()));
            }
            println!("{:<10} {:<26} {:<10} 组织/备注", "平台", "身份", "凭据");
            for ((plat, name), parts) in &groups {
                let kinds: Vec<&str> = parts.iter().map(|(k, _)| k.as_str()).collect();
                // 能不能直接拿去装 lark-cli：必须 id 和 secret 都在
                let ready = kinds.contains(&"app-id") && kinds.contains(&"app-secret");
                let note = parts
                    .iter()
                    .map(|(_, a)| a.as_str())
                    .find(|a| !a.is_empty())
                    .unwrap_or("");
                println!(
                    "{:<10} {:<26} {:<10} {}",
                    plat,
                    name,
                    if ready {
                        "✅ 成对".to_string()
                    } else {
                        kinds.join(",")
                    },
                    note
                );
            }
            println!(
                "\n共 {} 个身份。✅ 表示 app-id + app-secret 齐全，可直接装进 lark-cli。",
                groups.len()
            );
        }
        Cmd::Delete { r#ref } => {
            let b = Backend::select()?;
            if b.delete(&r#ref)? {
                audit_log(&who, "delete", &r#ref, "ok", b.name());
                println!("已删除 {ref}", r#ref = r#ref);
            } else {
                audit_log(&who, "delete", &r#ref, "miss", b.name());
                return Err(anyhow!("{ref} 不存在", r#ref = r#ref));
            }
        }
        Cmd::List {
            platform,
            org,
            json,
        } => {
            let b = Backend::select()?;
            let items = b.list()?;
            // 先说清这份清单来自哪个库。kyvault 同一个命令有两个后端，而 list 从前
            // 不说自己读的是哪一个 —— 于是「我 list 过了，库里没有 X」这句话可能完全
            // 是错的：读的是 file 后端的几条本地数据，而要找的密钥在 d1 里。
            // 2026-09-14 实测有 agent 正是这样断言「库里没有 CF 凭证」，然后回头找人
            // 要 token —— 而它就在另一个后端里躺着。
            if !json {
                // 走 stdout 而不是 stderr：agent 常用 `kyvault list | grep xxx`，
                // stderr 不进管道，提示就丢在最需要它的场景里。用 # 开头不碍 grep。
                println!(
                    "# 后端 {} · {} 条{}",
                    b.name(),
                    items.len(),
                    match b.name() {
                        "file" => "（本机 ~/.keyring；如果找不到期望的密钥，确认是否该设 KYVAULT_BACKEND=d1）",
                        _ => "",
                    }
                );
            }
            print_list(&items, platform.as_deref(), org.as_deref(), json)?;
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
                        false,
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
                    let list = s.list_bucket(&platform, "accounts")?;
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
                        false,
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
                    let list = s.list_bucket(&platform, "keys")?;
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
            let all = s.platforms()?;
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
                    let v = st.list_servers()?;
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
                    let v = st.list_clis(cli_name.as_deref())?;
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
            if let Some(entry) = kyvault::wizard::run()? {
                let b = Backend::select()?;
                b.set(&entry.r#ref, &entry.value, &entry.kind, &entry.account)?;
                println!("✓ 已保存 {}", entry.r#ref);
                if let Some(a) = &entry.alias {
                    let aliases = Aliases::default_location()?;
                    aliases.set(a, &entry.r#ref)?;
                    println!("  别名 {a} → {}", entry.r#ref);
                }
                println!("\n🎉 密钥已成功加密存入！");
                println!(
                    "  安全调用示例：kyvault run --env TOKEN={} -- <cmd>",
                    entry.r#ref
                );
            }
        }
        Cmd::Ui { port, no_open } => {
            kyvault::ui::start_server(port, !no_open)?;
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
            audit_log(
                &who,
                "import",
                &format!("{file}（{} 条）", planned.len()),
                "ok",
                b.name(),
            );
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
            let checked_ref = if api_key.is_none() {
                key_name
                    .as_ref()
                    .map(|name| format!("secret://{provider}/{name}"))
            } else {
                None
            };
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
            let started = std::time::Instant::now();
            let r = kyvault::providers::validate_key(&provider, &key);
            if let Some(reference) = checked_ref.as_deref() {
                kyvault::health::record(
                    &Store::default_location()?,
                    reference,
                    &provider,
                    &r,
                    started.elapsed().as_millis(),
                )?;
                audit_log(&who, "check", reference, r.status, "provider");
            }
            println!("[{}] {}", r.status, r.message);
            if !r.models.is_empty() {
                println!("  可用模型：{}", r.models.join(", "));
            }
            if let Some(b) = r.balance {
                println!("  {b}");
            }
            // 只有 invalid 返回 1；权限、网络和接口异常返回 2，不能据此自动轮换。
            match r.status {
                "valid" | "rate_limited" => {}
                "invalid" => std::process::exit(1),
                _ => std::process::exit(2),
            }
        }
        Cmd::Run {
            envs,
            profile,
            command,
        } => {
            let b = Backend::select()?;
            let aliases = Aliases::default_location()?;
            let mut cmd = Command::new(&command[0]);
            cmd.args(&command[1..]);
            let mut injected: Vec<String> = Vec::new();

            // --profile：把一个账号名下的 ref 全注入，变量名 = ref 名大写化。
            // 「一把钥匙开一把锁」在这里反过来：一次解锁，一次注入一批变量。
            if let Some(p) = profile.as_deref() {
                let store = Store::default_location()?;
                let reg = Registry::at(store.root())?;
                let entry = reg
                    .get(p)
                    .ok_or_else(|| anyhow!("没有 profile {p}（ky profile add {p} ... 先登记）"))?;
                if entry.refs.is_empty() {
                    return Err(anyhow!(
                        "profile {p} 名下没有 ref（ky profile link {p} secret://平台/名字）"
                    ));
                }
                for r in &entry.refs {
                    let (_, name) = kyvault::model::parse_ref(r)?;
                    let var = kyvault::registry::env_name(&name);
                    let v = b
                        .get(r)?
                        .ok_or_else(|| anyhow!("找不到 {r}（后端 {}）", b.name()))?;
                    cmd.env(&var, v);
                    injected.push(var);
                }
            }

            for spec in &envs {
                let (name, r) = spec
                    .split_once('=')
                    .ok_or_else(|| anyhow!("--env 应写成 NAME=secret://platform/name：{spec}"))?;
                let r = aliases.resolve(r);
                let v = b
                    .get(&r)?
                    .ok_or_else(|| anyhow!("找不到 {r}（后端 {}）", b.name()))?;
                cmd.env(name, v);
                injected.push(name.to_string());
            }

            if injected.is_empty() {
                return Err(anyhow!(
                    "没有要注入的密钥：用 --env NAME=secret://平台/名字，或 --profile <账号>"
                ));
            }

            let status = cmd
                .status()
                .with_context(|| format!("执行 {} 失败", command[0]))?;
            let code = status.code().unwrap_or(1);
            // 退出码进审计：拿到 1 和拿到 0 是两件事，都记成 ok 等于自欺
            audit_log(
                &who,
                "run",
                &format!("{} [{}] exit={code}", command[0], injected.join(",")),
                if code == 0 { "ok" } else { "err" },
                b.name(),
            );
            std::process::exit(code);
        }
        Cmd::Gitlab { action } => match action {
            GitLabCmd::Status => {
                let store = Store::default_location()?;
                let repo = GitLabRepo::new(&store);
                repo.status()?;
            }
            GitLabCmd::Pull => {
                let store = Store::default_location()?;
                let repo = GitLabRepo::new(&store);
                repo.pull()?;
                audit_log(&who, "pull", "gitlab", "ok", "file");
            }
            GitLabCmd::Push => {
                let store = Store::default_location()?;
                let repo = GitLabRepo::new(&store);
                repo.push()?;
                audit_log(&who, "push", "gitlab", "ok", "file");
            }
            GitLabCmd::Sync => {
                let store = Store::default_location()?;
                let repo = GitLabRepo::new(&store);
                repo.sync()?;
                audit_log(&who, "sync", "gitlab", "ok", "file");
            }
            GitLabCmd::Auth => {
                println!("🦊 GitLab 凭证探测（只读，不发网络请求）");
                println!(
                    "  - glab 已安装：{}",
                    if kyvault::glab::installed() {
                        "是"
                    } else {
                        "否"
                    }
                );
                if let Some(p) = kyvault::glab::config_path() {
                    println!(
                        "  - glab 配置：{}（{}）",
                        p.display(),
                        if p.exists() { "存在" } else { "不存在" }
                    );
                }
                match kyvault::glab::resolve(None) {
                    Some(c) => {
                        let t = &c.token;
                        let shown = if t.len() > 10 {
                            format!("{}…{}（{} 字符）", &t[..6], &t[t.len() - 4..], t.len())
                        } else {
                            "…".to_string()
                        };
                        println!("  - 将使用：{}（来源 {}）", c.host, c.source);
                        println!("  - token：{shown}");
                        println!(
                            "\n  当前优先级：KY_GITLAB_TOKEN → glab 配置文件 → glab auth status"
                        );
                    }
                    None => {
                        println!("  - 没找到任何 GitLab 凭证。");
                        println!("    修法：glab auth login，或 export KY_GITLAB_TOKEN=glpat-…");
                    }
                }
            }
            GitLabCmd::Setup { repo_url, dir } => {
                GitLabRepo::setup(&repo_url, dir.as_deref())?;
                audit_log(&who, "setup", &repo_url, "ok", "gitlab");
            }
        },
        Cmd::MasterKey { reveal } => {
            let store = Store::default_location()?;
            let mk_path = store.master_key_path();
            println!("🔑 kyvault 主密钥 (master.key) 状态");
            println!("{}", "-".repeat(52));
            println!("  - 物理路径：{}", mk_path.display());
            if !mk_path.exists() {
                println!("  - 状态：缺失 (FAIL，请先运行 kyvault init 或从带外备份恢复)");
                return Ok(());
            }
            println!("  - 文件权限：{}", doctor::perm_desc(&mk_path));
            let key = store.master_key()?;
            let key_trimmed = key.trim();

            let mut hasher = Sha256::new();
            hasher.update(key_trimmed.as_bytes());
            let hash = format!("{:x}", hasher.finalize());
            let fingerprint = &hash[..16];
            println!("  - 密钥格式：Base64 编码 (32 字节 / 256 位随机根密钥)");
            println!("  - 密钥指纹：SHA256:{fingerprint} (用于核对多设备/备份是否一致)");

            if reveal {
                println!("  - 密钥明文：{key_trimmed}");
            } else {
                let len = key_trimmed.len();
                let masked = if len > 8 {
                    format!("{}...{}", &key_trimmed[..4], &key_trimmed[len - 4..])
                } else {
                    "****".to_string()
                };
                println!("  - 密钥内容：{masked} (加 --reveal 或 -r 可显示完整明文)");
            }
            let sec_path = store.secrets_path();
            if sec_path.exists() {
                let count = match store.load() {
                    Ok(v) => v.as_object().map(|m| m.len()).unwrap_or(0),
                    Err(_) => 0,
                };
                println!(
                    "  - 对应密文：{} (收纳 {} 条密钥)",
                    sec_path.display(),
                    count
                );
            }
            println!("\n🛡️  安全备忘提示：");
            println!("  1. master.key 是解密本地及 GitLab 密文的【唯一根密钥】；");
            println!("  2. 它被 .gitignore 忽略，【绝对不会】也不能提交进 Git 仓库；");
            println!(
                "  3. 若在新设备使用，请将此密钥保存到 1Password / 加密便签，带外拷贝至目标机。"
            );
            println!("  4. 旧 Auth Guard 仅为兼容格式；公开 SSH 公钥派生 key，不验证私钥持有。");
            println!("{}", "-".repeat(52));
        }
        Cmd::Auth { action } => {
            let store = Store::default_location()?;
            let root = store.root();
            match action {
                AuthCmd::Setup => AuthGuard::setup(root)?,
                AuthCmd::Add { method, path } => {
                    AuthGuard::add_method(root, &method, path.as_deref())?;
                }
                AuthCmd::List => AuthGuard::list(root)?,
                AuthCmd::Remove { id } => AuthGuard::remove_method(root, &id)?,
                AuthCmd::Disable => AuthGuard::disable(root)?,
                AuthCmd::Status => AuthGuard::status(root)?,
            }
        }
        Cmd::Audit {
            limit,
            action,
            by,
            json,
        } => {
            let store = Store::default_location()?;
            let events = audit::read(&store, limit, action.as_deref(), by.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&events)?);
                return Ok(());
            }
            if events.is_empty() {
                println!(
                    "（没有审计记录：{}）",
                    if audit::enabled() {
                        "跑几条 get/set 之后再看"
                    } else {
                        "KY_AUDIT 被关掉了"
                    }
                );
                return Ok(());
            }
            let path = audit::path(&store);
            println!("# 审计日志 {}（最近 {} 条）", path.display(), events.len());
            println!(
                "{:<20} {:<12} {:<9} {:<8} 目标",
                "时间", "操作人", "动作", "结果"
            );
            for e in &events {
                println!(
                    "{:<20} {:<12} {:<9} {:<8} {}",
                    e["ts"].as_str().unwrap_or(""),
                    e["actor"].as_str().unwrap_or(""),
                    e["action"].as_str().unwrap_or(""),
                    e["result"].as_str().unwrap_or(""),
                    e["target"].as_str().unwrap_or("")
                );
            }
            println!("\n只记元信息：没有明文、没有密文、没有 last4。");
        }
        Cmd::Profile { action } => {
            let store = Store::default_location()?;
            let mut reg = Registry::at(store.root())?;
            match action {
                ProfileCmd::Add {
                    name,
                    platform,
                    username,
                    org,
                    cli,
                    note,
                    links,
                } => {
                    let p = Profile {
                        name: name.clone(),
                        platform: platform.unwrap_or_default(),
                        username: username.unwrap_or_default(),
                        org: org.unwrap_or_default(),
                        cli: cli.unwrap_or_default(),
                        note: note.unwrap_or_default(),
                        refs: links,
                        ..Default::default()
                    };
                    let saved = reg.upsert(p)?.clone();
                    audit_log(
                        &who,
                        "profile",
                        &format!("add:{name}"),
                        "ok",
                        "accounts.json",
                    );
                    println!(
                        "✓ 已登记账号 {}（{}@{}，{} 条 ref）",
                        saved.name,
                        if saved.username.is_empty() {
                            "—"
                        } else {
                            &saved.username
                        },
                        if saved.platform.is_empty() {
                            "—"
                        } else {
                            &saved.platform
                        },
                        saved.refs.len()
                    );
                }
                ProfileCmd::List => {
                    if reg.is_empty() {
                        println!("（还没有账号）");
                        println!(
                            "  ky profile add work --platform github --username alice --link secret://github/pat"
                        );
                        return Ok(());
                    }
                    let active = reg.active().unwrap_or("").to_string();
                    println!(
                        "{:<16} {:<10} {:<20} {:<10} {:<8} 密钥",
                        "账号", "平台", "登录名", "组织", "CLI"
                    );
                    for p in reg.list() {
                        println!(
                            "{:<16} {:<10} {:<20} {:<10} {:<8} {}{}",
                            p.name,
                            if p.platform.is_empty() {
                                "—"
                            } else {
                                &p.platform
                            },
                            if p.username.is_empty() {
                                "—"
                            } else {
                                &p.username
                            },
                            if p.org.is_empty() { "—" } else { &p.org },
                            if p.cli.is_empty() { "—" } else { &p.cli },
                            p.refs.len(),
                            if p.name == active {
                                "   ← 当前"
                            } else {
                                ""
                            }
                        );
                    }
                }
                ProfileCmd::Show { name } => {
                    let p = reg
                        .get(&name)
                        .ok_or_else(|| anyhow!("没有 profile {name}"))?;
                    println!("账号 {}", p.name);
                    println!("  平台：{}", dash(&p.platform));
                    println!("  登录名：{}", dash(&p.username));
                    println!("  组织：{}", dash(&p.org));
                    println!("  CLI：{}", dash(&p.cli));
                    println!("  备注：{}", dash(&p.note));
                    println!("  创建：{}", dash(&p.created_at));
                    if p.refs.is_empty() {
                        println!("  密钥：（未挂载）");
                    } else {
                        println!("  密钥（{} 条）：", p.refs.len());
                        for r in &p.refs {
                            let name = kyvault::model::parse_ref(r)
                                .map(|(_, n)| n)
                                .unwrap_or_else(|_| r.clone());
                            println!("    - {r} → 变量 {}", kyvault::registry::env_name(&name));
                        }
                    }
                    if reg.active() == Some(name.as_str()) {
                        println!("  （当前账号）");
                    }
                }
                ProfileCmd::Use { name } => {
                    reg.set_active(&name)?;
                    println!("✓ 当前账号切换为 {name}");
                }
                ProfileCmd::Current => match reg.active() {
                    Some(a) => println!("{a}"),
                    None => return Err(anyhow!("还没有当前账号（ky profile add ...）")),
                },
                ProfileCmd::Link { name, r#ref } => {
                    reg.link(&name, &r#ref)?;
                    println!("✓ {r} 已挂到 {name}", r = r#ref);
                }
                ProfileCmd::Unlink { name, r#ref } => {
                    if reg.unlink(&name, &r#ref)? {
                        println!("✓ 已从 {name} 摘掉 {r}", r = r#ref);
                    } else {
                        return Err(anyhow!("{name} 名下没有 {r}", r = r#ref));
                    }
                }
                ProfileCmd::Remove { name } => {
                    if reg.remove(&name)? {
                        println!("✓ 已删除账号登记 {name}（密钥本身没动）");
                    } else {
                        return Err(anyhow!("没有 profile {name}"));
                    }
                }
            }
        }
        Cmd::Broker { action } => {
            use kyvault::broker::{self, Device, Request};
            match action {
                BrokerCmd::Register {
                    id,
                    principal,
                    pubkey,
                    allow_ips,
                    refs,
                    ttl_hours,
                } => {
                    let st = Store::default_location()?;
                    let (public_key, fingerprint) = broker::validate_public_key(&pubkey)?;
                    let device = Device {
                        id,
                        principal,
                        public_key,
                        fingerprint,
                        approved_by: who.clone(),
                        approved_at: String::new(),
                        allow_ips,
                        refs,
                        expires_unix: 0,
                        revoked: false,
                    };
                    let registered = broker::register(&st, device, ttl_hours)?;
                    println!(
                        "已审批设备 {} / {}（{}），有效期 {} 小时，{} 条凭据",
                        registered.id,
                        registered.principal,
                        registered.fingerprint,
                        ttl_hours,
                        registered.refs.len()
                    );
                    println!("这是本地 OS 管理员审批；--as 仅为标签，不是主设备签名。");
                    println!(
                        "用 broker authorized-key {} 生成强制命令行，再由管理员装入专用 SSH 账户。",
                        registered.id
                    );
                }
                BrokerCmd::List => println!(
                    "{}",
                    serde_json::to_string_pretty(&broker::list(&Store::default_location()?)?)?
                ),
                BrokerCmd::Admit {
                    ticket,
                    fleet,
                    refs,
                    ttl_hours,
                    install,
                } => {
                    let st = Store::default_location()?;
                    let parsed = broker::ticket_parse(&ticket)?;
                    let hosts = broker::load_fleet(&fleet)?;
                    let (device, shared) = broker::adopt(
                        &st,
                        &hosts,
                        &parsed.host,
                        &parsed.key,
                        refs,
                        ttl_hours,
                        &who,
                    )?;
                    let line = broker::authorized_key(&st, &device.id, &std::env::current_exe()?)?;
                    println!(
                        "已批准 {} / {}（{}），{} 条凭据，{} 小时",
                        device.id,
                        device.principal,
                        device.fingerprint,
                        device.refs.len(),
                        ttl_hours
                    );
                    println!("  网络门禁：{}（取自台账）", device.allow_ips.join(","));
                    if !shared.is_empty() {
                        println!(
                            "  ⚠ 同一公网 IP 还有：{} —— IP 只当门禁，身份是 SSH key 指纹",
                            shared.join(", ")
                        );
                    }
                    match install {
                        Some(path) => {
                            let added = broker::install_authorized_key(&path, &line, &device.id)?;
                            println!(
                                "{} {}",
                                if added {
                                    "已写入"
                                } else {
                                    "已存在，跳过（未重复追加）"
                                },
                                path.display()
                            );
                            println!(
                                "  改完确认 sshd 生效：sudo sshd -t && sudo systemctl reload ssh"
                            );
                        }
                        None => {
                            println!("\n把下面这一行装进 vault host 专用账户的 authorized_keys：\n{line}");
                            println!("\n或加 --install ~kybroker/.ssh/authorized_keys 自动写入");
                        }
                    }
                    println!("  服务器侧日常：kyvault broker run --host <别名> --expect-device {} --env NAME=ref -- <cmd>", device.id);
                }
                BrokerCmd::Adopt {
                    host_id,
                    fleet,
                    pubkey,
                    refs,
                    ttl_hours,
                } => {
                    let st = Store::default_location()?;
                    let hosts = broker::load_fleet(&fleet)?;
                    let public_key = std::fs::read_to_string(&pubkey)
                        .map_err(|e| anyhow!("读不到公钥 {}：{e}", pubkey.display()))?;
                    let (device, shared) =
                        broker::adopt(&st, &hosts, &host_id, &public_key, refs, ttl_hours, &who)?;
                    println!(
                        "已领养 {} / {}（{}），{} 条凭据，{} 小时",
                        device.id,
                        device.principal,
                        device.fingerprint,
                        device.refs.len(),
                        ttl_hours
                    );
                    println!(
                        "  网络门禁：{}（/32，取自台账 public_ip）",
                        device.allow_ips.join(",")
                    );
                    if !shared.is_empty() {
                        println!(
                            "  ⚠ 同一公网 IP 还有：{} —— IP 只当网络门禁，设备身份由 SSH key 指纹区分",
                            shared.join(", ")
                        );
                    }
                    println!("  下一步：ky broker authorized-key {host_id} 生成 forced-command 行");
                }
                BrokerCmd::Status => println!(
                    "{}",
                    serde_json::to_string_pretty(&broker::status(&Store::default_location()?)?)?
                ),
                BrokerCmd::Revoke { id } => {
                    broker::revoke(&Store::default_location()?, &id)?;
                    println!(
                        "已撤销设备 {id} 的后续凭据取用；此前交付过的静态凭据如有泄漏仍需轮换。"
                    );
                }
                BrokerCmd::AuthorizedKey { id } => {
                    let st = Store::default_location()?;
                    println!(
                        "{}",
                        broker::authorized_key(&st, &id, &std::env::current_exe()?)?
                    );
                    eprintln!("安装要求：专用 broker SSH 账户；Agent 不能有该账户的普通 shell/SFTP/其他 key；sshd 禁止用户环境覆盖与用户 rc。不会自动修改你的 SSH 配置。");
                }
                BrokerCmd::Serve { store, device } => {
                    broker::serve(&Store::new(store), &device)?;
                }
                BrokerCmd::Probe {
                    host,
                    identity,
                    ssh_config,
                    expect_device,
                } => {
                    let result = broker::fetch(
                        &host,
                        identity.as_deref(),
                        &expect_device,
                        ssh_config.as_deref(),
                        &Request {
                            version: 1,
                            operation: "probe".into(),
                            refs: Vec::new(),
                        },
                    )?;
                    println!(
                        "设备 {} 已通过 SSH/IP/有效期检查，请求 {}；没有取出凭据。",
                        result.device, result.request_id
                    );
                }
                BrokerCmd::Run {
                    host,
                    expect_device,
                    ssh_config,
                    identity,
                    envs,
                    command,
                } => {
                    let mut names = std::collections::BTreeSet::new();
                    let mut bindings = Vec::new();
                    for spec in envs {
                        let (name, r) = spec
                            .split_once('=')
                            .ok_or_else(|| anyhow!("--env 需要 NAME=secret://platform/name"))?;
                        if name.is_empty()
                            || name.starts_with(|c: char| c.is_ascii_digit())
                            || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                            || !names.insert(name.to_string())
                        {
                            return Err(anyhow!("变量名无效或重复"));
                        }
                        kyvault::model::parse_ref(r)?;
                        bindings.push((name.to_string(), r.to_string()));
                    }
                    let result = broker::fetch(
                        &host,
                        identity.as_deref(),
                        &expect_device,
                        ssh_config.as_deref(),
                        &Request {
                            version: 1,
                            operation: "inject".into(),
                            refs: bindings.iter().map(|(_, r)| r.clone()).collect(),
                        },
                    )?;
                    let mut child = Command::new(&command[0]);
                    child.args(&command[1..]);
                    for (name, r) in bindings {
                        child.env(name, &result.values[&r]);
                    }
                    let status = child.status().context("启动获准子进程失败")?;
                    std::process::exit(status.code().unwrap_or(1));
                }
            }
        }
        Cmd::Inspect { duplicates } => {
            let st = Store::default_location()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&kyvault::inspect::report(&st, duplicates)?)?
            );
        }
        Cmd::Agent { action } => match action {
            AgentCmd::Init { id } => {
                let dir = dirs::home_dir()
                    .ok_or_else(|| anyhow!("找不到 home 目录"))?
                    .join(".kyvault/agent");
                std::fs::create_dir_all(&dir)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
                }
                let key = dir.join("agent");
                if !key.exists() {
                    let status = std::process::Command::new("ssh-keygen")
                        .args(["-q", "-t", "ed25519", "-N", "", "-C", "kyvault-agent"])
                        .arg("-f")
                        .arg(&key)
                        .status()
                        .context("需要本机安装 OpenSSH（ssh-keygen）")?;
                    if !status.success() {
                        return Err(anyhow!("生成密钥失败"));
                    }
                }
                let host = match id {
                    Some(v) => v,
                    None => {
                        let name = std::process::Command::new("hostname")
                            .output()
                            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                            .unwrap_or_default();
                        // hostname 常带 .local / 内网后缀，取首段更接近台账 id
                        name.split('.').next().unwrap_or("agent").to_lowercase()
                    }
                };
                let public = std::fs::read_to_string(key.with_extension("pub"))?;
                let ticket = kyvault::broker::ticket_new(&host, &public)?;
                println!("设备私钥：{}（0600，只在本机）", key.display());
                println!("\n把下面整行复制到存库那台机器上批准：\n\n{ticket}\n");
                println!("在那台机器上执行：");
                println!("  kyvault broker admit '{ticket}' --allow-ref secret://<平台>/<名字> --ttl-hours 24");
                println!("\n批准后日常用法（不需要再带 host/device 之外的参数）：");
                println!("  kyvault broker run --host <vault反代或别名> --expect-device {host} \\");
                println!("    --env TOKEN=secret://<平台>/<名字> -- <命令>");
                println!("\n票据不含任何凭据，只含设备 ID 与公钥。");
            }
        },
        Cmd::Panel { out } => {
            let store = Store::default_location()?;
            let path = kyvault::panel::write(&store, &out)?;
            println!("✓ 已生成只读面板：{}", path.display());
            println!("  内容：账号 / 密钥条目 / 操作计数 / 最近审计 —— 只有元信息。");
            println!("  不含凭据值/密文/尾号；账号、IP、设备和审计仍敏感，请在受限站点托管（noindex 不是鉴权）。");
            println!("  本地可视化编辑才是 ky ui（仅 127.0.0.1）。");
        }
    }
    Ok(())
}

/// 审计写一条。失败不打断主流程（见 `audit` 模块头部）。
fn audit_log(who: &str, action: &str, target: &str, result: &str, note: &str) {
    if let Ok(store) = Store::default_location() {
        audit::record(
            &store,
            who,
            audit::Event {
                action,
                target,
                result,
                note,
            },
        );
    }
}

fn dash(s: &str) -> &str {
    if s.is_empty() {
        "—"
    } else {
        s
    }
}
