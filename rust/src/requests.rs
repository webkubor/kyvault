//! Agent 侧的凭据录入请求 —— 「Agent 声明要存什么，App 负责收密文」。
//!
//! ## 为什么要有这个箱子
//!
//! 原本的录入路径有两条断点：
//!
//! 1. **Agent 不知道该怎么描述一条凭据**。让 agent 拼 `secret://platform/name` 再敲
//!    `ky set`，它得同时知道 ref 语法、平台 id、kind 措辞，猜错就存歪了。
//! 2. **明文不能经过 agent**。让 agent 直接 `ky set` 就意味着密文先经过模型上下文，
//!    这正是本项目要消灭的事 —— 所以明文必须由人直接交给本地界面。
//!
//! 于是把「描述」和「密文」拆开：agent 只写**元信息**（平台、名称、账号、别名），
//! 人把密文粘进本机界面。中间的交接点就是这个 inbox。
//!
//! ## 安全边界
//!
//! - 箱子里**只有元信息**，永远没有密文。文件权限 0600。
//! - 校验在写入前完成（`providers::validate_key`），验证不过不写库。
//! - 请求有有效期，过期自动作废 —— 免得三个月后弹出一个「请录入」的僵尸条目。
//! - 完成后的回执只带 ref 与末 4 位，不回显密文。
//!
//! ## 一次完整流转
//!
//! ```text
//! agent                inbox/req-xxxx.json            Ky.app / ky ui
//!   │  declare  ────────▶ {platform,name,alias,...} ───▶ 弹出录入框（预填）
//!   │                                                    │ 用户粘密文
//!   │  wait/status ◀────────────────────────── 完成回执 ◀─┘ 校验→加密→写库
//!   │  {status:"done", ref:"secret://github/pat"}
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::parse_ref;

/// 请求默认 30 分钟过期。过期后界面不再提示，Agent 侧读到的是 expired。
const DEFAULT_TTL_MINUTES: u64 = 30;

/// 平台 id / 名称里允许出现的字符。名字会拼进 `secret://<platform>/<name>`，
/// 路径分隔符和空白都不该进来。
const NAME_MAX: usize = 64;
const NOTE_MAX: usize = 200;

/// 一次录入请求的描述。全部是元信息，不含任何密钥材料。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Spec {
    /// 平台 id，对应 `providers::PROVIDERS` 或 `category::CATEGORIES` 里的平台。
    pub platform: String,
    /// 条目名称，会拼成 `secret://<platform>/<name>`。
    pub name: String,
    /// 账号（GitHub 用户名、邮箱、组织……），可空。
    #[serde(default)]
    pub account: String,
    /// 条目类型，如 `Token`、`API Key`。空则由平台推断。
    #[serde(default)]
    pub kind: String,
    /// 存完后自动建立的别名，agent 之后就用它取用。
    #[serde(default)]
    pub alias: String,
    /// 给人看的备注 —— 为什么需要这把凭据。
    #[serde(default)]
    pub note: String,
}

/// 请求的落盘状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// 等待人录入。
    Pending,
    /// 已成功入库。
    Done,
    /// 人明确放弃或录入失败。
    Failed,
    /// 过期作废。
    Expired,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Done => "done",
            Status::Failed => "failed",
            Status::Expired => "expired",
        }
    }
}

/// 一条请求在 inbox 里的完整记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    #[serde(flatten)]
    pub spec: Spec,
    /// 谁发起的（agent 名 / 终端用户名），用于界面上显示来源。
    #[serde(default)]
    pub requested_by: String,
    pub created_at: String,
    /// Unix 秒。超过这个时间仍未完成就是过期。
    pub expires_at: u64,
    pub status: Status,
    /// 完成回执：ref、末 4 位、校验结论。不含密文。
    #[serde(default)]
    pub result: Option<Result_>,
}

/// 录入完成后的回执。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result_ {
    /// 入库的完整引用。
    pub r#ref: String,
    /// 末 4 位，仅供人眼确认「存的是哪一把」。
    #[serde(default)]
    pub last4: String,
    /// 校验结论原文（valid / invalid / unknown …）。
    #[serde(default)]
    pub status: String,
    /// 建立成功的别名。
    #[serde(default)]
    pub alias: String,
}

/// 一次录入的结论。给 CLI 和界面用。
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub id: String,
    pub status: Status,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last4: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

/// inbox 根目录：跟密钥库同根，`requests/` 子目录。
///
/// 跟 store 同根是刻意的：请求里没有密文，但「哪台机器上的哪个库」这件事
/// 跟着 store 走才不会认错库。
pub fn inbox_dir() -> Result<PathBuf> {
    Ok(crate::store::default_store_dir()?.join("requests"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_iso() -> String {
    crate::model::now_utc()
}

fn random_id() -> String {
    let mut bytes = [0u8; 6];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("req-{hex}")
}

/// 平台 id 与名称的字符集。挡住路径穿越与空白。
fn valid_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= NAME_MAX
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn valid_alias(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= NAME_MAX
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// 规范化并校验描述。平台 id 小写；名称保留原样但禁掉危险字符。
pub fn normalize(mut spec: Spec) -> Result<Spec> {
    spec.platform = spec.platform.trim().to_lowercase();
    spec.name = spec.name.trim().to_lowercase();
    spec.account = spec.account.trim().to_string();
    spec.kind = spec.kind.trim().to_string();
    spec.alias = spec.alias.trim().to_lowercase();
    spec.note = spec.note.trim().to_string();

    if !valid_token(&spec.platform) {
        bail!("平台 id 不合法：只能是小写字母、数字、- _ .");
    }
    if !valid_token(&spec.name) {
        bail!("名称不合法：只能是小写字母、数字、- _ .");
    }
    if spec.account.chars().any(|c| c.is_control()) {
        bail!("账号不能包含控制字符");
    }
    if !spec.alias.is_empty() && !valid_alias(&spec.alias) {
        bail!("别名不合法：只能是小写字母、数字、- _ .");
    }
    if spec.note.chars().count() > NOTE_MAX {
        bail!("备注不能超过 {NOTE_MAX} 字");
    }
    // ref 拼装后再验一次 —— 这是最终入库地址，值得多一道检查。
    parse_ref(&format!("secret://{}/{}", spec.platform, spec.name))?;
    Ok(spec)
}

/// 写入一条请求，返回落盘记录。
pub fn declare(spec: Spec, requested_by: &str, ttl_minutes: Option<u64>) -> Result<Request> {
    let spec = normalize(spec)?;
    let dir = inbox_dir()?;
    fs::create_dir_all(&dir).context("无法创建请求目录")?;
    let ttl = ttl_minutes.unwrap_or(DEFAULT_TTL_MINUTES).clamp(1, 24 * 60);
    let now = now_secs();
    let req = Request {
        id: random_id(),
        spec,
        requested_by: requested_by.trim().to_string(),
        created_at: now_iso(),
        expires_at: now + ttl * 60,
        status: Status::Pending,
        result: None,
    };
    write_request(&dir, &req)?;
    Ok(req)
}

fn write_request(dir: &Path, req: &Request) -> Result<()> {
    let path = dir.join(format!("{}.json", req.id));
    let body = serde_json::to_vec_pretty(req)?;
    fs::write(&path, body).context("无法写入请求文件")?;
    crate::store::harden(&path)?;
    Ok(())
}

/// 读一条请求。
pub fn get(id: &str) -> Result<Request> {
    let dir = inbox_dir()?;
    let path = dir.join(format!("{}.json", id));
    let body = fs::read_to_string(&path).with_context(|| format!("请求不存在：{id}"))?;
    let mut req: Request = serde_json::from_str(&body).context("请求文件已损坏")?;
    refresh_expiry(&mut req);
    Ok(req)
}

/// 列出全部请求。过期但还没落盘的，顺手把状态改成 expired。
///
/// 返回按创建时间倒序：新的在上，Agent 与界面看的是「最近要处理的」。
pub fn list() -> Result<Vec<Request>> {
    let dir = inbox_dir()?;
    let mut out = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(err) => return Err(err).context("无法读取请求目录"),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut req) = serde_json::from_str::<Request>(&body) else {
            continue;
        };
        if refresh_expiry(&mut req) {
            let _ = write_request(&dir, &req);
        }
        out.push(req);
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

/// 待处理的请求。界面上只弹这些。
pub fn pending() -> Result<Vec<Request>> {
    Ok(list()?
        .into_iter()
        .filter(|r| r.status == Status::Pending)
        .collect())
}

/// 标记完成。ref 与末 4 位是给人和 Agent 看的回执。
pub fn complete(id: &str, r#ref: &str, last4: &str, status: &str, alias: &str) -> Result<Request> {
    let mut req = get(id)?;
    req.status = Status::Done;
    req.result = Some(Result_ {
        r#ref: r#ref.to_string(),
        last4: last4.to_string(),
        status: status.to_string(),
        alias: alias.to_string(),
    });
    write_request(&inbox_dir()?, &req)?;
    Ok(req)
}

/// 标记失败。人放弃、或校验怎么都过不去。
pub fn fail(id: &str, message: &str) -> Result<Request> {
    let mut req = get(id)?;
    req.status = Status::Failed;
    req.result = Some(Result_ {
        r#ref: String::new(),
        last4: String::new(),
        status: message.to_string(),
        alias: String::new(),
    });
    write_request(&inbox_dir()?, &req)?;
    Ok(req)
}

/// 删除一条请求（人处理完了，清理 inbox）。
pub fn remove(id: &str) -> Result<bool> {
    let path = inbox_dir()?.join(format!("{}.json", id));
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).context("无法删除请求文件"),
    }
}

/// 超过有效期就作废。返回是否发生了状态变化。
fn refresh_expiry(req: &mut Request) -> bool {
    if req.status == Status::Pending && now_secs() > req.expires_at {
        req.status = Status::Expired;
        return true;
    }
    false
}

/// 等待一条请求落到终态。Agent 用它阻塞到「人存好了」为止。
///
/// 超时不是失败 —— 人可能还在想。超时后返回当前状态，Agent 可以继续等。
pub fn wait(id: &str, timeout: Duration, poll: Duration) -> Result<Outcome> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let req = get(id)?;
        match req.status {
            Status::Done => {
                let result = req.result.unwrap_or(Result_ {
                    r#ref: String::new(),
                    last4: String::new(),
                    status: String::new(),
                    alias: String::new(),
                });
                return Ok(Outcome {
                    id: req.id,
                    status: Status::Done,
                    message: result.status,
                    r#ref: Some(result.r#ref),
                    last4: Some(result.last4),
                    alias: Some(result.alias),
                });
            }
            Status::Failed => {
                let message = req
                    .result
                    .map(|r| r.status)
                    .unwrap_or_else(|| "录入失败".to_string());
                return Ok(Outcome {
                    id: req.id,
                    status: Status::Failed,
                    message,
                    r#ref: None,
                    last4: None,
                    alias: None,
                });
            }
            Status::Expired => {
                return Ok(Outcome {
                    id: req.id,
                    status: Status::Expired,
                    message: "请求已过期".to_string(),
                    r#ref: None,
                    last4: None,
                    alias: None,
                })
            }
            Status::Pending => {}
        }
        if std::time::Instant::now() >= deadline {
            return Ok(Outcome {
                id: req.id,
                status: Status::Pending,
                message: "仍在等待人工录入".to_string(),
                r#ref: None,
                last4: None,
                alias: None,
            });
        }
        std::thread::sleep(poll);
    }
}

/// 给界面用的轻量视图：不含任何路径与内部字段。
pub fn summaries() -> Result<Vec<BTreeMap<String, String>>> {
    Ok(pending()?
        .into_iter()
        .map(|r| {
            let mut m = BTreeMap::new();
            m.insert("id".to_string(), r.id);
            m.insert("platform".to_string(), r.spec.platform.clone());
            m.insert("name".to_string(), r.spec.name.clone());
            m.insert("account".to_string(), r.spec.account.clone());
            m.insert("kind".to_string(), r.spec.kind.clone());
            m.insert("alias".to_string(), r.spec.alias.clone());
            m.insert("note".to_string(), r.spec.note.clone());
            m.insert("requested_by".to_string(), r.requested_by.clone());
            m.insert("created_at".to_string(), r.created_at.clone());
            m
        })
        .collect())
}

/// 一次「校验 → 加密 → 写库 → 建别名 → 回执」的完整落地。
///
/// CLI 和桌面端都走这里，免得两条路径各写一遍然后慢慢分叉。
/// 校验不通过（含网络判断不了）返回 Err，绝不写库 —— 一把没验过的密钥进了库，
/// 下游 401 时会先怀疑别处，排查成本远高于重录一次。
pub fn fulfill(
    store: &crate::store::Store,
    aliases: &crate::alias::Aliases,
    id: &str,
    value: &str,
) -> Result<Outcome> {
    let req = get(id)?;
    if req.status != Status::Pending {
        bail!("请求已是 {} 状态，不能重复录入", req.status.as_str());
    }
    let value = value.trim();
    if value.is_empty() {
        bail!("密钥为空");
    }
    if value.len() > 65_536 {
        bail!("密钥超过 64 KiB");
    }

    let r#ref = format!("secret://{}/{}", req.spec.platform, req.spec.name);
    let (check_status, check_message) = verify(&req.spec.platform, value);
    if check_status != "valid" && check_status != "unverified" {
        // 记录失败原因（不外泄密钥），把请求留在 pending，人可以重试。
        let _ = note_failure(id, &check_message);
        bail!("{}", check_message);
    }

    let kind = if req.spec.kind.is_empty() {
        default_kind(&req.spec.platform)
    } else {
        req.spec.kind.to_string()
    };
    store.set_secret_with_meta(
        &r#ref,
        value,
        Some(kind.as_str()),
        (!req.spec.account.is_empty()).then_some(req.spec.account.as_str()),
    )?;

    let alias = req.spec.alias.to_string();
    if !alias.is_empty() {
        aliases.set(&alias, &r#ref)?;
    }

    let last4 = crate::model::last4(value);
    let done = complete(id, &r#ref, &last4, &check_status, &alias)?;
    Ok(Outcome {
        id: done.id,
        status: Status::Done,
        message: check_status,
        r#ref: Some(r#ref),
        last4: Some(last4),
        alias: (!alias.is_empty()).then_some(alias),
    })
}

/// 校验一把密钥。平台不在校验表里就放行 —— 只能保证「语法没问题」，
/// 但这个判断必须说出来，不能假装验过了。
fn verify(platform: &str, value: &str) -> (String, String) {
    if crate::providers::get_provider(platform).is_none() {
        return (
            "unverified".to_string(),
            format!("平台 {platform} 没有在线校验接口，已按原样入库（未验证有效性）"),
        );
    }
    let result = crate::providers::validate_key(platform, value);
    if result.valid {
        (result.status.to_string(), result.message)
    } else if result.status == "unsupported" {
        (
            "unverified".to_string(),
            format!("平台 {platform} 不支持在线校验，已入库（未验证有效性）"),
        )
    } else {
        (result.status.to_string(), result.message)
    }
}

/// 没写 kind 时按平台推断。校验表里的平台基本都是 API Key，其余按通用 Token 处理。
fn default_kind(platform: &str) -> String {
    match platform {
        "github" | "gitlab" => "Token",
        "openrouter" | "cloudflare" | "feishu" => "Token",
        _ => "API Key",
    }
    .to_string()
}

/// 把失败原因挂到请求上，但不改状态 —— 人还可以再粘一次。
fn note_failure(id: &str, message: &str) -> Result<()> {
    let mut req = get(id)?;
    req.result = Some(Result_ {
        r#ref: String::new(),
        last4: String::new(),
        status: message.to_string(),
        alias: String::new(),
    });
    write_request(&inbox_dir()?, &req)
}

/// 唤起本机 Ky.app，把请求 id 作为参数传进去。
///
/// 返回 false 表示没找到 App —— 调用方应退回 `ky add` 的 Web 录入页。
/// 这里只负责「叫醒」，真正的解密与写库由 App 进程完成，CLI 不碰密文。
#[cfg(target_os = "macos")]
pub fn open_desktop(id: &str) -> bool {
    // withGlobalTauri 的 App 注册了 single-instance，第二次启动会把 argv 转给
    // 已在运行的实例，所以这里既能在未启动时拉起，也能在已运行时唤起窗口。
    use std::process::{Command, Stdio};

    let candidates = [
        "/Applications/Ky.app".to_string(),
        format!(
            "{}/Applications/Ky.app",
            std::env::var("HOME").unwrap_or_default()
        ),
    ];
    let Some(app) = candidates.iter().find(|p| Path::new(p).is_dir()) else {
        return false;
    };
    Command::new("open")
        .arg("-a")
        .arg(app)
        .arg("--args")
        .arg("--request")
        .arg(id)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
pub fn open_desktop(_id: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(platform: &str, name: &str) -> Spec {
        Spec {
            platform: platform.into(),
            name: name.into(),
            account: String::new(),
            kind: String::new(),
            alias: String::new(),
            note: String::new(),
        }
    }

    #[test]
    fn normalize_rejects_path_traversal() {
        assert!(normalize(spec("github", "../etc")).is_err());
        assert!(normalize(spec("github", "a b")).is_err());
        assert!(normalize(spec("git/hub", "pat")).is_err());
        assert!(normalize(spec("github", "")).is_err());
    }

    #[test]
    fn normalize_lowercases_platform_and_name() {
        let s = normalize(spec("GitHub", "PAT")).unwrap();
        assert_eq!(s.platform, "github");
        assert_eq!(s.name, "pat");
    }

    #[test]
    fn normalize_rejects_bad_alias() {
        let mut s = spec("github", "pat");
        s.alias = "has space".into();
        assert!(normalize(s).is_err());
    }

    #[test]
    fn summaries_hide_internal_paths() {
        let m = summaries().unwrap();
        for item in m {
            assert!(!item.contains_key("path"));
            assert!(!item.contains_key("secret"));
        }
    }
}
