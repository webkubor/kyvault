//! `ky add`：浏览器录入凭据 —— 规范化 → 找旧条目 → 先验证 → 验证通过才写库。
//!
//! 与 `ky ui` 的区别：这里只收不吐，没有任何读明文的接口；并且按一次性服务设计：
//! - URL 带随机 token，不带 token 的请求一律 404；
//! - 校验 Host 只能是 127.0.0.1/localhost（挡 DNS rebinding）；
//! - POST 只收 application/json 且不回 CORS 头 —— 跨站页面发这种请求必先预检，
//!   预检拿不到许可就发不出来，别的网页没法借浏览器往库里塞东西；
//! - 空闲 15 分钟自动退出。
//!
//! 验证不通过（含「网络原因判断不了」）一律不写库：一把没验过的密码进了库，
//! 下游失败时会被当成「库里的是好的」去排查别处。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::mailcred;
use crate::model::{last4, parse_ref, SecretMeta};
use crate::providers::{self, CheckResult};
use crate::store::Store;

const IDLE_LIMIT: Duration = Duration::from_secs(15 * 60);
const MAX_BODY: usize = 64 * 1024;

/// main.rs 里的后端（file / D1）通过它接进来，本模块不关心具体是哪个。
pub trait Vault {
    fn list(&self) -> Result<Vec<SecretMeta>>;
    fn get(&self, r: &str) -> Result<Option<String>>;
    fn set(&self, r: &str, v: &str, kind: &str, account: &str) -> Result<()>;
    fn delete(&self, r: &str) -> Result<bool>;
    fn backend_name(&self) -> &'static str;
    fn audit(&self, action: &str, target: &str, result: &str);
}

pub struct Prefill {
    pub email: Option<String>,
    pub provider: Option<String>,
    /// 进入「邮箱登录密码」模式
    pub login: bool,
}

pub fn run(vault: &dyn Vault, port: u16, auto_open: bool, prefill: Prefill) -> Result<()> {
    let listener = (port..=port.saturating_add(50))
        .find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok())
        .ok_or_else(|| anyhow!("找不到可用的本地端口"))?;
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    let token = format!("{:032x}", rand::random::<u128>());

    let mut query = Vec::new();
    if let Some(e) = &prefill.email {
        query.push(format!("email={}", urlencode(e)));
    }
    if let Some(p) = &prefill.provider {
        query.push(format!("provider={}", urlencode(p)));
    }
    if prefill.login {
        query.push("mode=login".to_string());
    }
    let url = format!(
        "http://127.0.0.1:{port}/{token}/{}",
        if query.is_empty() {
            String::new()
        } else {
            format!("?{}", query.join("&"))
        }
    );
    println!("录入页：{url}");
    println!(
        "后端：{}。验证通过才写库；页面点「完成」或空闲 15 分钟自动退出。",
        vault.backend_name()
    );
    if auto_open {
        open_browser(&url);
    }

    let ctx = Ctx { vault, port, token };
    let mut last_active = Instant::now();
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                last_active = Instant::now();
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(10)))?;
                match handle(&ctx, &mut stream) {
                    Ok(Flow::Done) => {
                        println!("已关闭录入页。");
                        return Ok(());
                    }
                    Ok(Flow::Continue) => {}
                    Err(e) => {
                        let _ = respond(
                            &mut stream,
                            "400 Bad Request",
                            &json!({"ok": false, "error": format!("{e:#}")}),
                        );
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if last_active.elapsed() > IDLE_LIMIT {
                    println!("空闲超时，录入页已关闭。");
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

struct Ctx<'a> {
    vault: &'a dyn Vault,
    port: u16,
    token: String,
}

enum Flow {
    Continue,
    Done,
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if len > MAX_BODY {
        return Err(anyhow!("请求体过大"));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

fn handle(ctx: &Ctx, stream: &mut TcpStream) -> Result<Flow> {
    let req = read_request(stream)?;
    let host_ok = req.headers.get("host").is_some_and(|h| {
        h == &format!("127.0.0.1:{}", ctx.port) || h == &format!("localhost:{}", ctx.port)
    });
    let path = req.path.split('?').next().unwrap_or_default();
    let Some(sub) = path
        .strip_prefix('/')
        .and_then(|p| p.strip_prefix(ctx.token.as_str()))
        .filter(|_| host_ok)
    else {
        send(stream, "404 Not Found", "text/plain", b"Not Found")?;
        return Ok(Flow::Continue);
    };

    if req.method == "GET" && (sub.is_empty() || sub == "/") {
        send(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            PAGE.as_bytes(),
        )?;
        return Ok(Flow::Continue);
    }
    if req.method == "GET" && sub == "/api/providers" {
        let list: Vec<Value> = providers::PROVIDERS
            .iter()
            .map(|p| json!({"id": p.id, "name": p.name, "logo": p.logo}))
            .collect();
        respond(stream, "200 OK", &json!({"ok": true, "providers": list}))?;
        return Ok(Flow::Continue);
    }

    let is_json = req
        .headers
        .get("content-type")
        .is_some_and(|c| c.starts_with("application/json"));
    if req.method != "POST" || !is_json {
        send(stream, "404 Not Found", "text/plain", b"Not Found")?;
        return Ok(Flow::Continue);
    }
    match sub {
        "/api/lookup" => {
            let r: LookupReq = serde_json::from_slice(&req.body)?;
            let v = lookup(ctx.vault, &r)
                .unwrap_or_else(|e| json!({"ok": false, "error": format!("{e:#}")}));
            respond(stream, "200 OK", &v)?;
        }
        "/api/save" => {
            let r: SaveReq = serde_json::from_slice(&req.body)?;
            let v = save(ctx.vault, &r).unwrap_or_else(
                |e| json!({"ok": false, "status": "error", "message": format!("{e:#}")}),
            );
            respond(stream, "200 OK", &v)?;
        }
        "/api/done" => {
            respond(stream, "200 OK", &json!({"ok": true}))?;
            return Ok(Flow::Done);
        }
        _ => send(stream, "404 Not Found", "text/plain", b"Not Found")?,
    }
    Ok(Flow::Continue)
}

#[derive(Deserialize)]
struct LookupReq {
    mode: String,
    email: Option<String>,
    provider: Option<String>,
}

#[derive(Deserialize)]
struct SaveReq {
    mode: String,
    email: Option<String>,
    provider: Option<String>,
    value: String,
    target_ref: String,
    account: Option<String>,
    /// 登录密码模式的第二次输入
    confirm: Option<String>,
}

/// 邮箱的两类密码必须互不覆盖：授权码管 IMAP/SMTP，登录密码管网页登录，
/// 改了其中一个、另一个照常有效。存错位置的后果是丢掉另一把。
const AUTH_KINDS: &[&str] = &["授权码", "App Password", "API Key"];
const LOGIN_KINDS: &[&str] = &["登录密码", "邮箱", "密码", "Password"];

fn is_kind(m: &SecretMeta, kinds: &[&str]) -> bool {
    kinds.iter().any(|k| m.kind.eq_ignore_ascii_case(k))
}

/// 登录密码可能真的含空格，只去首尾空白和零宽字符；中间的空格原样保留并在页面提示。
fn normalize_login(raw: &str) -> String {
    raw.chars()
        .filter(|c| !matches!(*c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn meta_json(m: &SecretMeta, selectable: bool, note: &str) -> Value {
    json!({
        "ref": m.r#ref(),
        "kind": m.kind,
        "account": m.account,
        "last4": m.last4,
        "updated_at": m.updated_at,
        "selectable": selectable,
        "note": note,
    })
}

/// 同账号的 email/* 条目里，类型对得上的才可覆盖；别的平台（Apple ID 密码之类）
/// 和另一类邮箱密码只展示不让选 —— 它们是不同的钥匙，覆盖了就丢了。
fn mail_candidates(all: &[SecretMeta], email: &str, login: bool) -> Vec<(SecretMeta, bool)> {
    all.iter()
        .filter(|m| mailcred::account_has_email(&m.account, email))
        .map(|m| {
            let right_kind = if login {
                !is_kind(m, AUTH_KINDS)
            } else {
                !is_kind(m, LOGIN_KINDS)
            };
            (m.clone(), m.platform == "email" && right_kind)
        })
        .collect()
}

/// 一个邮箱只有一个登录密码：库里同邮箱的登录密码条目只留一条（名字带 login 的优先，
/// 其次最近更新的），其余视为重复，保存新密码时一并删掉 —— 留着旧的只会让人取错。
struct LoginPlan {
    target: String,
    existing: Option<SecretMeta>,
    duplicates: Vec<SecretMeta>,
}

fn login_plan(all: &[SecretMeta], email: &str) -> LoginPlan {
    let mut mine: Vec<SecretMeta> = mail_candidates(all, email, true)
        .into_iter()
        .filter(|(_, sel)| *sel)
        .map(|(m, _)| m)
        .collect();
    mine.sort_by_key(|m| (m.name.contains("login"), m.updated_at.clone()));
    let existing = mine.pop();
    let target = existing
        .as_ref()
        .map(|m| m.r#ref())
        .unwrap_or_else(|| free_ref(all, &format!("{}-login", mail_default_name(email))));
    LoginPlan {
        target,
        existing,
        duplicates: mine,
    }
}

fn mail_default_name(email: &str) -> String {
    let slug: String = email
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("secret://email/{slug}")
}

fn free_ref(all: &[SecretMeta], base: &str) -> String {
    let taken = |r: &str| all.iter().any(|m| m.r#ref() == r);
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base}-{i}"))
        .find(|r| !taken(r))
        .unwrap_or_default()
}

fn lookup(vault: &dyn Vault, r: &LookupReq) -> Result<Value> {
    let all = vault.list()?;
    match r.mode.as_str() {
        "mail" => {
            let email = r.email.as_deref().unwrap_or_default().trim().to_string();
            let profile = mailcred::profile_for(&email)?;
            let cands = mail_candidates(&all, &email, false);
            let preferred = cands
                .iter()
                .filter(|(_, sel)| *sel)
                .max_by_key(|(m, _)| {
                    ["smtp", "imap", "app", "auth", "pass"]
                        .iter()
                        .any(|k| m.name.contains(k))
                })
                .map(|(m, _)| m.r#ref());
            Ok(json!({
                "ok": true,
                "profile": {
                    "imap": format!("{}:{}", profile.imap_host, profile.imap_port),
                    "kind": profile.kind,
                    "hint": profile.hint,
                    "expected_len": profile.expected_len,
                },
                "candidates": cands.iter().map(|(m, sel)| meta_json(m, *sel, "")).collect::<Vec<_>>(),
                "default_ref": preferred,
                "new_ref": free_ref(&all, &mail_default_name(&email)),
            }))
        }
        "login" => {
            let email = r.email.as_deref().unwrap_or_default().trim().to_string();
            if !email.contains('@') {
                return Err(anyhow!("邮箱格式不对：{email}"));
            }
            let cands = mail_candidates(&all, &email, true);
            let plan = login_plan(&all, &email);
            Ok(json!({
                "ok": true,
                "profile": {
                    "hint": "一个邮箱只存一条登录密码，新的覆盖旧的。网页登录有验证码，无法在线验证，用两次输入一致防止输错。授权码请用「邮箱授权码」标签存，两者互不覆盖。",
                },
                "candidates": cands.iter().filter(|(_, sel)| !*sel).map(|(m, _)| meta_json(m, false, "")).collect::<Vec<_>>(),
                "login": {
                    "target": plan.target,
                    "existing": plan.existing.as_ref().map(|m| meta_json(m, true, "")),
                    "duplicates": plan.duplicates.iter().map(|m| meta_json(m, true, "")).collect::<Vec<_>>(),
                },
            }))
        }
        "apikey" => {
            let provider = r.provider.as_deref().unwrap_or_default();
            let p = providers::get_provider(provider)
                .ok_or_else(|| anyhow!("不支持的平台 {provider}（ky providers 看清单）"))?;
            let cands: Vec<&SecretMeta> = all.iter().filter(|m| m.platform == p.id).collect();
            Ok(json!({
                "ok": true,
                "profile": {"name": p.name, "env_key": p.env_key},
                "candidates": cands.iter().map(|m| meta_json(m, true, "")).collect::<Vec<_>>(),
                "default_ref": cands.first().map(|m| m.r#ref()),
                "new_ref": free_ref(&all, &format!("secret://{}/main", p.id)),
            }))
        }
        m => Err(anyhow!("未知模式 {m}")),
    }
}

fn save(vault: &dyn Vault, r: &SaveReq) -> Result<Value> {
    let all = vault.list()?;
    let plan = (r.mode == "login")
        .then(|| login_plan(&all, r.email.as_deref().unwrap_or_default().trim()));
    let target = match &plan {
        Some(p) => p.target.clone(),
        None => r.target_ref.trim().to_string(),
    };
    let target = target.as_str();
    let (platform, _) = parse_ref(target)?;
    let existing = all.iter().find(|m| m.r#ref() == target);

    type Verifier = Box<dyn Fn(&str) -> CheckResult>;
    let (value, account, kind, check_provider, verify): (String, String, String, String, Verifier) =
        match r.mode.as_str() {
            "mail" => {
                let email = r.email.as_deref().unwrap_or_default().trim().to_string();
                let profile = mailcred::profile_for(&email)?;
                if platform != "email" {
                    return Err(anyhow!("邮箱密码只能存到 secret://email/ 下"));
                }
                if let Some(m) = existing {
                    if !mailcred::account_has_email(&m.account, &email) {
                        return Err(anyhow!(
                            "{target} 已存在且属于 {}，不是 {email}，拒绝覆盖",
                            if m.account.is_empty() {
                                "未登记账号"
                            } else {
                                &m.account
                            }
                        ));
                    }
                }
                if existing.is_some_and(|m| is_kind(m, LOGIN_KINDS)) {
                    return Err(anyhow!("{target} 是登录密码条目，授权码不能覆盖它"));
                }
                let e2 = email.clone();
                (
                    mailcred::normalize(&r.value),
                    email,
                    profile.kind.to_string(),
                    "imap".to_string(),
                    Box::new(move |v: &str| mailcred::verify(&e2, v)),
                )
            }
            "login" => {
                let email = r.email.as_deref().unwrap_or_default().trim().to_string();
                if !email.contains('@') {
                    return Err(anyhow!("邮箱格式不对：{email}"));
                }
                if platform != "email" {
                    return Err(anyhow!("邮箱登录密码只能存到 secret://email/ 下"));
                }
                if let Some(m) = existing {
                    if !mailcred::account_has_email(&m.account, &email) {
                        return Err(anyhow!("{target} 不属于 {email}，拒绝覆盖"));
                    }
                    if is_kind(m, AUTH_KINDS) {
                        return Err(anyhow!("{target} 是授权码条目，登录密码不能覆盖它"));
                    }
                }
                let value = normalize_login(&r.value);
                if value != normalize_login(r.confirm.as_deref().unwrap_or_default()) {
                    return Err(anyhow!("两次输入不一致，没有保存"));
                }
                (
                    value,
                    email,
                    "登录密码".to_string(),
                    String::new(),
                    Box::new(|_: &str| CheckResult {
                        valid: true,
                        status: "valid",
                        http_status: None,
                        message: "两次输入一致（登录密码无法在线验证）".to_string(),
                        models: Vec::new(),
                        balance: None,
                    }),
                )
            }
            "apikey" => {
                let provider = r.provider.as_deref().unwrap_or_default().to_string();
                let p = providers::get_provider(&provider)
                    .ok_or_else(|| anyhow!("不支持的平台 {provider}"))?;
                if platform != p.id {
                    return Err(anyhow!("{} 的 key 只能存到 secret://{}/ 下", p.name, p.id));
                }
                let account = r
                    .account
                    .clone()
                    .filter(|a| !a.trim().is_empty())
                    .or_else(|| existing.map(|m| m.account.clone()))
                    .unwrap_or_default();
                let id = p.id.to_string();
                (
                    mailcred::normalize(&r.value),
                    account,
                    "API Key".to_string(),
                    id.clone(),
                    Box::new(move |v: &str| providers::validate_key(&id, v)),
                )
            }
            m => return Err(anyhow!("未知模式 {m}")),
        };

    if value.is_empty() {
        return Err(anyhow!("没填内容"));
    }
    let started = Instant::now();
    let check = verify(&value);
    let elapsed = started.elapsed().as_millis();
    let store = Store::default_location()?;
    vault.audit("check", target, check.status);
    if check.status != "valid" {
        println!("✗ {target}：{}（未写入）", check.message);
        return Ok(json!({
            "ok": false,
            "status": check.status,
            "message": check.message,
            "len": value.chars().count(),
            "last4": last4(&value),
        }));
    }

    let old_last4 = vault.get(target)?.map(|v| last4(&v));
    vault.set(target, &value, &kind, &account)?;
    if !check_provider.is_empty() {
        crate::health::record(&store, target, &check_provider, &check, elapsed)?;
    }
    vault.audit("set", target, "ok");
    let mut deleted = Vec::new();
    for d in plan.map(|p| p.duplicates).unwrap_or_default() {
        let r = d.r#ref();
        if vault.delete(&r)? {
            vault.audit("delete", &r, "ok");
            println!("  已删除重复的旧登录密码 {r}（…{}）", d.last4);
            deleted.push(r);
        }
    }
    let new_last4 = last4(&value);
    println!(
        "✓ {target}：{}，已{}（…{} → …{new_last4}）",
        check.message,
        if old_last4.is_some() {
            "覆盖"
        } else {
            "新建"
        },
        old_last4.as_deref().unwrap_or("无"),
    );
    Ok(json!({
        "ok": true,
        "status": "valid",
        "message": check.message,
        "ref": target,
        "old_last4": old_last4,
        "new_last4": new_last4,
        "len": value.chars().count(),
        "deleted": deleted,
    }))
}

fn respond(stream: &mut TcpStream, status: &str, v: &Value) -> Result<()> {
    send(stream, status, "application/json", v.to_string().as_bytes())
}

fn send(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(platform: &str, name: &str, kind: &str, account: &str, updated: &str) -> SecretMeta {
        SecretMeta {
            platform: platform.into(),
            name: name.into(),
            kind: kind.into(),
            account: account.into(),
            updated_at: updated.into(),
            ..Default::default()
        }
    }

    #[test]
    fn login_plan_keeps_one_and_marks_duplicates() {
        let all = vec![
            meta("email", "a-login", "登录密码", "a@163.com", "2026-01-01"),
            meta("email", "a-old", "密码", "a@163.com", "2026-09-01"),
            meta("email", "a-auth", "授权码", "a@163.com", "2026-09-02"),
            meta("google", "backup", "密码", "a@163.com", "2026-09-03"),
            meta("email", "b-login", "登录密码", "b@163.com", "2026-09-04"),
        ];
        let p = login_plan(&all, "a@163.com");
        assert_eq!(p.target, "secret://email/a-login");
        let dups: Vec<String> = p.duplicates.iter().map(|m| m.r#ref()).collect();
        assert_eq!(dups, vec!["secret://email/a-old"]);
    }

    #[test]
    fn login_plan_new_slot_when_none() {
        let p = login_plan(&[], "x@gmail.com");
        assert_eq!(p.target, "secret://email/x-gmail-com-login");
        assert!(p.existing.is_none() && p.duplicates.is_empty());
    }
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).status();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).status();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .status();
}

const PAGE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="referrer" content="no-referrer">
<title>kyvault · 录入凭据</title>
<style>
  :root { --bg:#f6f5f2; --card:#fff; --line:#e3e0da; --text:#2f2d2a; --dim:#7a766f; --accent:#5b7a6e; --ok:#4f7a52; --bad:#a5524a; --warn:#a07a2e; }
  * { box-sizing:border-box; margin:0; padding:0; }
  body { font:14px/1.6 -apple-system,BlinkMacSystemFont,"PingFang SC",sans-serif; background:var(--bg); color:var(--text); padding:32px 16px; }
  main { max-width:640px; margin:0 auto; }
  h1 { font-size:20px; margin-bottom:4px; }
  .sub { color:var(--dim); margin-bottom:20px; }
  .tabs { display:flex; gap:8px; margin-bottom:16px; }
  .tabs button { flex:1; padding:10px; border:1px solid var(--line); background:var(--card); border-radius:8px; cursor:pointer; font-size:14px; color:var(--dim); }
  .tabs button.on { border-color:var(--accent); color:var(--accent); font-weight:600; }
  .card { background:var(--card); border:1px solid var(--line); border-radius:12px; padding:20px; margin-bottom:16px; }
  label { display:block; font-weight:600; margin:14px 0 6px; }
  label:first-child { margin-top:0; }
  input, select { width:100%; padding:10px 12px; border:1px solid var(--line); border-radius:8px; font-size:14px; background:#fff; color:var(--text); }
  input:focus, select:focus { outline:none; border-color:var(--accent); }
  .mono { font-family:ui-monospace,SFMono-Regular,Menlo,monospace; }
  .row { display:flex; gap:8px; }
  .row input { flex:1; }
  .ghost { padding:0 12px; border:1px solid var(--line); background:#fff; border-radius:8px; cursor:pointer; white-space:nowrap; }
  .hint { color:var(--dim); font-size:12px; margin-top:6px; }
  .preview { margin-top:8px; font-size:13px; padding:8px 10px; border-radius:6px; background:#f3f1ec; }
  .warn { color:var(--warn); }
  .opt { display:flex; gap:10px; align-items:flex-start; padding:10px; border:1px solid var(--line); border-radius:8px; margin-bottom:8px; cursor:pointer; }
  .opt input[type=radio] { width:auto; margin-top:4px; }
  .opt > div { flex:1; min-width:0; overflow-wrap:anywhere; }
  .opt .meta { color:var(--dim); font-size:12px; }
  details.others { margin:4px 0 8px; color:var(--dim); font-size:12px; }
  details.others summary { cursor:pointer; padding:4px 0; }
  details.others ul { list-style:none; padding:6px 0 0 4px; }
  details.others li { overflow-wrap:anywhere; padding:2px 0; }
  body { overflow-x:hidden; }
  .tabs button { min-width:0; }
  .primary { width:100%; padding:12px; border:none; border-radius:8px; background:var(--accent); color:#fff; font-size:15px; font-weight:600; cursor:pointer; margin-top:16px; }
  .primary:disabled { opacity:.5; cursor:wait; }
  .result { margin-top:14px; padding:12px; border-radius:8px; display:none; white-space:pre-wrap; }
  .result.ok { display:block; background:#eef4ee; color:var(--ok); }
  .result.bad { display:block; background:#f7ecea; color:var(--bad); }
  .done { background:none; border:1px solid var(--line); color:var(--dim); }
</style>
</head>
<body>
<main>
  <h1>录入凭据</h1>
  <p class="sub">先验证，通过了才加密写入。明文只在本机，页面不会回显已存的密码。</p>

  <div class="tabs">
    <button id="tab-mail" class="on" onclick="setMode('mail')">邮箱授权码 / 应用密码</button>
    <button id="tab-login" onclick="setMode('login')">邮箱登录密码</button>
    <button id="tab-apikey" onclick="setMode('apikey')">API Key</button>
  </div>

  <div class="card">
    <div id="who-mail">
      <label for="email">邮箱地址</label>
      <input id="email" class="mono" placeholder="name@gmail.com" autocomplete="off" spellcheck="false">
    </div>
    <div id="who-apikey" style="display:none">
      <label for="provider">平台</label>
      <select id="provider"></select>
      <label for="account">账号备注（可选）</label>
      <input id="account" placeholder="如 个人 / 公司项目" autocomplete="off">
    </div>
    <p id="profile" class="hint"></p>

    <label>存到哪一条</label>
    <div id="targets"><p class="hint">先填上面的信息</p></div>

    <label for="value" id="value-label">密码</label>
    <div class="row">
      <input id="value" type="password" class="mono" autocomplete="new-password" spellcheck="false" placeholder="直接粘贴，空格会自动去掉">
      <button class="ghost" onclick="toggle()">显示</button>
    </div>
    <div id="confirm-row" style="display:none">
      <label for="confirm">再输一次</label>
      <input id="confirm" type="password" class="mono" autocomplete="new-password" spellcheck="false" placeholder="手动再输一遍，防止输错">
    </div>
    <p id="caps" class="hint warn" style="display:none">⚠ 大写锁定已开启，密码区分大小写</p>
    <div id="preview" class="preview" style="display:none"></div>

    <button id="save" class="primary" onclick="save()">验证并保存</button>
    <div id="result" class="result"></div>
  </div>

  <button class="primary done" onclick="done()">完成，关闭录入页</button>
</main>
<script>
const base = location.pathname.replace(/\/$/, '');
let mode = 'mail', lookupData = null, timer = null;

async function post(path, body) {
  const r = await fetch(base + path, { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify(body) });
  return r.json();
}
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const normalize = s => mode === 'login'
  ? s.replace(/[\u200B-\u200D\u2060\uFEFF]/g, '').trim()
  : s.replace(/[\s\u200B-\u200D\u2060\uFEFF]/g, '');

function setMode(m) {
  mode = m;
  document.getElementById('tab-mail').classList.toggle('on', m === 'mail');
  document.getElementById('tab-login').classList.toggle('on', m === 'login');
  document.getElementById('tab-apikey').classList.toggle('on', m === 'apikey');
  document.getElementById('who-mail').style.display = m === 'apikey' ? 'none' : '';
  document.getElementById('who-apikey').style.display = m === 'apikey' ? '' : 'none';
  document.getElementById('confirm-row').style.display = m === 'login' ? '' : 'none';
  document.getElementById('confirm').value = '';
  document.getElementById('value-label').innerText = { mail: '授权码 / 应用专用密码', login: '新登录密码', apikey: 'API Key' }[m];
  document.getElementById('value').placeholder = m === 'login' ? '首尾空白会去掉，中间的空格保留' : '直接粘贴，空格会自动去掉';
  document.getElementById('save').innerText = m === 'login' ? '核对并保存' : '验证并保存';
  lookupData = null; renderTargets(); preview(); lookup();
}

function scheduleLookup() { clearTimeout(timer); timer = setTimeout(lookup, 350); }

async function lookup() {
  const prof = document.getElementById('profile');
  const body = { mode };
  if (mode !== 'apikey') {
    body.email = document.getElementById('email').value.trim();
    if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(body.email)) { prof.innerText = ''; lookupData = null; renderTargets(); return; }
  } else {
    body.provider = document.getElementById('provider').value;
  }
  const d = await post('/api/lookup', body);
  if (!d.ok) { prof.innerHTML = `<span class="warn">${esc(d.error)}</span>`; lookupData = null; renderTargets(); return; }
  lookupData = d;
  prof.innerText = mode === 'mail'
    ? `验证方式：IMAP 登录 ${d.profile.imap}。${d.profile.hint}`
    : mode === 'login' ? d.profile.hint
    : `验证方式：调用 ${d.profile.name} 接口；环境变量名 ${d.profile.env_key}`;
  renderTargets(); preview();
}

function renderTargets() {
  const el = document.getElementById('targets');
  if (!lookupData) { el.innerHTML = '<p class="hint">先填上面的信息</p>'; return; }
  const othersHtml = list => list.length ? `
    <details class="others"><summary>同账号还有 ${list.length} 条其他凭据（平台或类型不同，这里不能覆盖）</summary>
      <ul>${list.map(c => `<li class="mono">${esc(c.ref)}</li>`).join('')}</ul></details>` : '';
  if (mode === 'login') {
    const L = lookupData.login, e = L.existing;
    let h = `<div class="opt"><div><div class="mono">${esc(L.target)}　<b>${e ? '覆盖' : '新建'}</b></div>
      <div class="meta">${e ? `登录密码 · 末四位 …${esc(e.last4)}${e.updated_at ? ' · 更新于 ' + esc(e.updated_at.slice(0, 10)) : ''}` : '库里还没有这个邮箱的登录密码'}</div></div></div>`;
    if (L.duplicates.length) h += `<p class="hint warn">还有 ${L.duplicates.length} 条重复的旧登录密码，保存时一并删除：${L.duplicates.map(d => esc(d.ref)).join('、')}</p>`;
    el.innerHTML = h + othersHtml(lookupData.candidates);
    return;
  }
  const sel = lookupData.default_ref || '__new__';
  const clip = (s, n) => { s = String(s || ''); return s.length > n ? s.slice(0, n) + '…' : s; };
  const mine = lookupData.candidates.filter(c => c.selectable);
  const others = lookupData.candidates.filter(c => !c.selectable);
  let html = mine.length ? '' : '<p class="hint">库里没有这个账号的旧条目。</p>';
  html += mine.map(c => `
    <label class="opt">
      <input type="radio" name="target" value="${esc(c.ref)}" ${c.ref === sel ? 'checked' : ''}>
      <div><div class="mono">${esc(c.ref)}　<b>覆盖</b></div>
      <div class="meta">${esc(c.kind || '-')} · ${c.last4 ? '末四位 …' + esc(c.last4) : '末四位未知'}${c.updated_at ? ' · 更新于 ' + esc(c.updated_at.slice(0, 10)) : ''}</div>
      <div class="meta">${esc(clip(c.account || '无账号备注', 60))}</div></div>
    </label>`).join('');
  html += `
    <label class="opt">
      <input type="radio" name="target" value="__new__" ${sel === '__new__' ? 'checked' : ''}>
      <div><div>新建一条</div>
      <input id="new-ref" class="mono" value="${esc(lookupData.new_ref)}" style="margin-top:6px" onclick="document.querySelector('input[value=__new__]').checked=true"></div>
    </label>`;
  el.innerHTML = html + othersHtml(others);
}

function preview() {
  const raw = document.getElementById('value').value, el = document.getElementById('preview');
  if (!raw) { el.style.display = 'none'; return; }
  const v = normalize(raw), notes = [];
  if (v.length !== raw.length) notes.push(`已去掉 ${raw.length - v.length} 个空白/不可见字符`);
  if ((mode === 'login' ? /[^\x20-\x7E]/ : /[^\x21-\x7E]/).test(v)) notes.push('<span class="warn">含非 ASCII 字符（中文标点或全角字母？）</span>');
  const exp = mode === 'mail' && lookupData && lookupData.profile.expected_len;
  if (exp && v.length !== exp) notes.push(`<span class="warn">长度 ${v.length}，这类密码通常 ${exp} 位</span>`);
  if (mode === 'login') {
    if (/\s/.test(v)) notes.push('<span class="warn">中间含空格，会原样保存</span>');
    const c = document.getElementById('confirm').value;
    if (c) notes.push(normalize(c) === v ? '✓ 两次输入一致' : '<span class="warn">✗ 两次输入不一致</span>');
  }
  const shown = document.getElementById('value').type === 'text' ? esc(v) : '•'.repeat(Math.max(0, v.length - 4)) + esc(v.slice(-4));
  el.innerHTML = `将保存：<span class="mono">${shown}</span>（${v.length} 位，大小写原样保留）${notes.length ? '<br>' + notes.join('<br>') : ''}`;
  el.style.display = 'block';
}

function toggle() {
  const el = document.getElementById('value');
  el.type = el.type === 'password' ? 'text' : 'password';
  event.target.innerText = el.type === 'password' ? '显示' : '隐藏';
  preview();
}

async function save() {
  const res = document.getElementById('result'), btn = document.getElementById('save');
  const value = document.getElementById('value').value;
  const picked = document.querySelector('input[name=target]:checked');
  if (!lookupData) { show(res, false, mode === 'apikey' ? '先选平台' : '先填正确的邮箱地址'); return; }
  if (!normalize(value)) { show(res, false, '没填内容'); return; }
  if (mode !== 'login' && !picked) { show(res, false, '选一条要存到的位置'); return; }
  const confirmValue = document.getElementById('confirm').value;
  if (mode === 'login' && normalize(confirmValue) !== normalize(value)) { show(res, false, '两次输入不一致'); return; }
  const L = mode === 'login' ? lookupData.login : null;
  const target_ref = L ? L.target : picked.value === '__new__' ? document.getElementById('new-ref').value.trim() : picked.value;
  const isOverwrite = L ? !!(L.existing || L.duplicates.length) : picked.value !== '__new__';
  const extra = L && L.duplicates.length ? `\n并删除重复的旧登录密码：${L.duplicates.map(d => d.ref).join('、')}` : '';
  if (isOverwrite && !confirm(`保存后会覆盖 ${target_ref} 的旧值${extra}\n继续？`)) return;
  const label = btn.innerText;
  btn.disabled = true; btn.innerText = mode === 'login' ? '正在保存…' : '正在验证…（最多约 30 秒）';
  try {
    const d = await post('/api/save', {
      mode, target_ref, value, confirm: confirmValue,
      email: document.getElementById('email').value.trim(),
      provider: document.getElementById('provider').value,
      account: document.getElementById('account').value.trim(),
    });
    if (d.ok) {
      show(res, true, `✓ ${d.message}\n已${d.old_last4 ? '覆盖' : '新建'} ${d.ref}\n末四位：${d.old_last4 ? '…' + d.old_last4 + ' → ' : ''}…${d.new_last4}（${d.len} 位）${d.deleted && d.deleted.length ? '\n已删除重复的旧条目：' + d.deleted.join('、') : ''}`);
      document.getElementById('value').value = ''; document.getElementById('confirm').value = ''; preview(); lookup();
    } else {
      const why = { invalid: '验证失败，密码不对或已失效', unknown: '无法判断（网络问题），为安全起见没有保存', unsupported: '不支持验证', rate_limited: '被限流，没能确认有效，没有保存', permission_limited: '权限受限，没能确认有效，没有保存' }[d.status] || '没有保存';
      show(res, false, `✗ ${why}\n${d.message}${d.len ? `\n（你输入的是 ${d.len} 位，末四位 …${d.last4}）` : ''}`);
    }
  } catch (e) { show(res, false, '请求失败：' + e); }
  btn.disabled = false; btn.innerText = label;
}

function show(el, ok, msg) { el.className = 'result ' + (ok ? 'ok' : 'bad'); el.innerText = msg; }

async function done() {
  await post('/api/done', {});
  document.body.innerHTML = '<main><h1>已关闭</h1><p class="sub">录入服务已退出，可以关掉这个标签页。</p></main>';
}

(async () => {
  const d = await (await fetch(base + '/api/providers')).json();
  document.getElementById('provider').innerHTML = d.providers.map(p => `<option value="${esc(p.id)}">${esc(p.logo)} ${esc(p.name)}</option>`).join('');
  const q = new URLSearchParams(location.search);
  document.getElementById('email').addEventListener('input', scheduleLookup);
  document.getElementById('provider').addEventListener('change', lookup);
  document.getElementById('value').addEventListener('input', preview);
  document.getElementById('confirm').addEventListener('input', preview);
  const caps = e => { if (e.getModifierState) document.getElementById('caps').style.display = e.getModifierState('CapsLock') ? '' : 'none'; };
  for (const id of ['value', 'confirm']) { document.getElementById(id).addEventListener('keydown', caps); document.getElementById(id).addEventListener('keyup', caps); }
  if (q.get('provider')) { document.getElementById('provider').value = q.get('provider'); setMode('apikey'); }
  else { if (q.get('email')) document.getElementById('email').value = q.get('email'); setMode(q.get('mode') === 'login' ? 'login' : 'mail'); }
})();
</script>
</body>
</html>
"#;
