//! 邮箱授权码 / 应用专用密码：规范化 + IMAP 登录验证。
//!
//! 为什么验 IMAP 而不是 SMTP：代理节点普遍封 25/465/587（防垃圾邮件），
//! 本机走代理时 SMTP 握手直接被掐，会把一把好密码误判成「连不上」。
//! 993 通常放行，而且 163/QQ 的授权码、Gmail 应用专用密码对 IMAP 和 SMTP 是同一把。
//!
//! 输出纪律：密码只在 LOGIN 那一行出现在网络上，任何报错/返回都不带它。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use crate::providers::CheckResult;

const TIMEOUT: Duration = Duration::from_secs(12);

pub struct MailProfile {
    pub imap_host: String,
    pub imap_port: u16,
    /// 写进 kind 列：让 list 一眼看出这是授权码还是登录密码
    pub kind: &'static str,
    /// 该邮箱商这类密码的固定长度（已知时），只用来提示，不拦截
    pub expected_len: Option<usize>,
    pub hint: &'static str,
}

pub fn profile_for(email: &str) -> Result<MailProfile> {
    let email = email.trim();
    let (_, domain) = email
        .rsplit_once('@')
        .filter(|(l, d)| !l.is_empty() && d.contains('.'))
        .ok_or_else(|| anyhow!("邮箱格式不对：{email}"))?;
    let domain = domain.to_ascii_lowercase();
    let p = |host: &str, kind, expected_len, hint| MailProfile {
        imap_host: host.to_string(),
        imap_port: 993,
        kind,
        expected_len,
        hint,
    };
    Ok(match domain.as_str() {
        "gmail.com" | "googlemail.com" => p(
            "imap.gmail.com",
            "App Password",
            Some(16),
            "Google 应用专用密码：16 位字母，页面上显示的空格会自动去掉",
        ),
        "163.com" | "126.com" | "yeah.net" => p(
            &format!("imap.{domain}"),
            "授权码",
            Some(16),
            "网易客户端授权码（设置 → POP3/SMTP/IMAP），区分大小写；需先开启 IMAP 服务",
        ),
        "qq.com" | "foxmail.com" => p(
            "imap.qq.com",
            "授权码",
            Some(16),
            "QQ 邮箱授权码（设置 → 账户 → IMAP 服务），需先开启 IMAP",
        ),
        "icloud.com" | "me.com" | "mac.com" => p(
            "imap.mail.me.com",
            "App Password",
            Some(16),
            "Apple ID 的 App 专用密码（xxxx-xxxx-xxxx-xxxx），连字符保留",
        ),
        "outlook.com" | "hotmail.com" | "live.com" => {
            return Err(anyhow!(
                "Outlook 已关闭 IMAP 基本认证，应用密码无法用 LOGIN 验证，暂不支持"
            ))
        }
        _ => p(
            &format!("imap.{domain}"),
            "邮箱密码",
            None,
            "未知邮箱商：按 imap.<域名>:993 验证，连不上说明服务器地址不同",
        ),
    })
}

/// 去掉所有空白（含全角空格、NBSP）和零宽字符，大小写原样保留。
///
/// Google 页面把应用专用密码显示成 `abcd efgh ijkl mnop`，复制时空格会跟着来；
/// 从网页复制还常夹带 U+200B 之类看不见的字符 —— 这些都会让登录失败，
/// 而且失败时人眼看不出差别。授权码本身从不含空白，所以一律剥掉是安全的。
pub fn normalize(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_whitespace())
        .filter(|c| !matches!(*c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}'))
        .collect()
}

/// 规范化之后的格式提示（不拦截，真正的判据是登录结果）
pub fn format_warning(profile: &MailProfile, pw: &str) -> Option<String> {
    if !pw.is_ascii() {
        return Some("含非 ASCII 字符（可能混进了中文标点或全角字母）".into());
    }
    match profile.expected_len {
        Some(n) if pw.chars().count() != n => Some(format!(
            "长度 {} 位，这类密码通常是 {n} 位，检查是否复制完整",
            pw.chars().count()
        )),
        _ => None,
    }
}

pub fn verify(email: &str, password: &str) -> CheckResult {
    let profile = match profile_for(email) {
        Ok(p) => p,
        Err(e) => return result("unsupported", e.to_string()),
    };
    if password.is_empty() {
        return result("invalid", "密码为空".into());
    }
    if !password.is_ascii() || password.chars().any(|c| c.is_control()) {
        return result(
            "invalid",
            "密码含非 ASCII 或控制字符，邮箱服务器不会接受".into(),
        );
    }

    let mut errors = Vec::new();
    let direct = connect_direct(&profile.imap_host, profile.imap_port);
    let session = match direct {
        Ok(s) => Some(s),
        Err(e) => {
            errors.push(format!("直连：{e:#}"));
            match system_proxy() {
                Some(proxy) => {
                    match connect_via_proxy(&proxy, &profile.imap_host, profile.imap_port) {
                        Ok(s) => Some(s),
                        Err(e) => {
                            errors.push(format!("代理 {proxy}：{e:#}"));
                            None
                        }
                    }
                }
                None => None,
            }
        }
    };
    let Some(mut s) = session else {
        return result(
            "unknown",
            format!(
                "连不上 {}:{}，无法判断密码是否有效（{}）",
                profile.imap_host,
                profile.imap_port,
                errors.join("；")
            ),
        );
    };
    match login(&mut s, email, password) {
        Ok(LoginOutcome::Ok) => result("valid", format!("{} IMAP 登录成功", profile.imap_host)),
        Ok(LoginOutcome::Rejected(msg)) => {
            result("invalid", format!("{} 拒绝登录：{msg}", profile.imap_host))
        }
        Err(e) => result("unknown", format!("登录过程中断：{e:#}")),
    }
}

fn result(status: &'static str, message: String) -> CheckResult {
    CheckResult {
        valid: status == "valid",
        status,
        http_status: None,
        message,
        models: Vec::new(),
        balance: None,
    }
}

type TlsStream = BufReader<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>;

fn tls_config() -> Result<Arc<rustls::ClientConfig>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Arc::new(cfg))
}

/// TLS 握手 + 读到问候行才算连上：TUN 模式下 TCP 总能「连上」假 IP，
/// 真正被掐是在握手阶段，只看 connect 成功会错过回退代理的时机。
fn handshake(tcp: TcpStream, host: &str) -> Result<TlsStream> {
    tcp.set_read_timeout(Some(TIMEOUT))?;
    tcp.set_write_timeout(Some(TIMEOUT))?;
    let name = rustls::pki_types::ServerName::try_from(host.to_string())?;
    let conn = rustls::ClientConnection::new(tls_config()?, name)?;
    let mut s = BufReader::new(rustls::StreamOwned::new(conn, tcp));
    let mut greeting = String::new();
    s.read_line(&mut greeting)
        .context("TLS 握手或读取问候失败")?;
    if !greeting.starts_with("* OK") {
        return Err(anyhow!("服务器问候异常：{}", greeting.trim()));
    }
    Ok(s)
}

fn connect_direct(host: &str, port: u16) -> Result<TlsStream> {
    let addr = (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("解析不到 {host}"))?;
    let tcp = TcpStream::connect_timeout(&addr, TIMEOUT)?;
    handshake(tcp, host)
}

fn connect_via_proxy(proxy: &str, host: &str, port: u16) -> Result<TlsStream> {
    let addr = proxy
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("代理地址无效"))?;
    let mut tcp = TcpStream::connect_timeout(&addr, TIMEOUT)?;
    tcp.set_read_timeout(Some(TIMEOUT))?;
    write!(
        tcp,
        "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n"
    )?;
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if tcp.read(&mut byte)? == 0 || head.len() > 8192 {
            return Err(anyhow!("代理提前断开"));
        }
        head.push(byte[0]);
    }
    let status = String::from_utf8_lossy(&head);
    let first = status.lines().next().unwrap_or_default();
    if !first.contains(" 200") {
        return Err(anyhow!("代理拒绝隧道：{first}"));
    }
    handshake(tcp, host)
}

/// HTTPS_PROXY / ALL_PROXY（仅 http://），其次 macOS 系统代理（scutil）。
fn system_proxy() -> Option<String> {
    for key in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
        if let Ok(v) = std::env::var(key) {
            if let Some(rest) = v.trim().strip_prefix("http://") {
                let hostport = rest.trim_end_matches('/');
                if !hostport.is_empty() {
                    return Some(hostport.to_string());
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("scutil")
            .arg("--proxy")
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let field = |k: &str| {
            text.lines().find_map(|l| {
                let (a, b) = l.split_once(" : ")?;
                (a.trim() == k).then(|| b.trim().to_string())
            })
        };
        if field("HTTPSEnable").as_deref() == Some("1") {
            return Some(format!("{}:{}", field("HTTPSProxy")?, field("HTTPSPort")?));
        }
    }
    None
}

enum LoginOutcome {
    Ok,
    Rejected(String),
}

fn imap_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn login(s: &mut TlsStream, email: &str, password: &str) -> Result<LoginOutcome> {
    let cmd = format!(
        "k1 LOGIN {} {}\r\n",
        imap_quote(email),
        imap_quote(password)
    );
    s.get_mut().write_all(cmd.as_bytes())?;
    s.get_mut().flush()?;
    loop {
        let mut line = String::new();
        if s.read_line(&mut line)? == 0 {
            return Err(anyhow!("服务器在返回结果前断开"));
        }
        let Some(rest) = line.strip_prefix("k1 ") else {
            continue;
        };
        let rest = rest.trim();
        let outcome = if rest.starts_with("OK") {
            LoginOutcome::Ok
        } else {
            // 服务器原话里不会有密码，但仍只取前 160 字符，防止异常长回包刷屏
            LoginOutcome::Rejected(rest.chars().take(160).collect())
        };
        let _ = s.get_mut().write_all(b"k2 LOGOUT\r\n");
        return Ok(outcome);
    }
}

/// 从备注里抽出邮箱 token 精确比较 —— 子串匹配会让 `webkubor@163.com`
/// 命中 `xwebkubor@163.com`，覆盖错条目的代价远大于漏掉一条候选。
pub fn account_has_email(account: &str, email: &str) -> bool {
    account
        .split(|c: char| !(c.is_ascii_alphanumeric() || "._%+-@".contains(c)))
        .any(|tok| tok.eq_ignore_ascii_case(email.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_spaces_keeps_case() {
        assert_eq!(
            normalize(" AbCd efgh\u{3000}ijkl\u{200B} mnop\n"),
            "AbCdefghijklmnop"
        );
    }

    #[test]
    fn account_match_is_exact_token() {
        assert!(account_has_email("webkubor@163.com", "webkubor@163.com"));
        assert!(account_has_email(
            "主号 (WEBKUBOR@163.com)",
            "webkubor@163.com"
        ));
        assert!(!account_has_email("xwebkubor@163.com", "webkubor@163.com"));
        assert!(!account_has_email("163-primary", "webkubor@163.com"));
    }

    #[test]
    fn quote_escapes() {
        assert_eq!(imap_quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn gmail_profile() {
        let p = profile_for("x@gmail.com").unwrap();
        assert_eq!(p.imap_host, "imap.gmail.com");
        assert!(format_warning(&p, "abcdefghijklmnop").is_none());
        assert!(format_warning(&p, "abc").is_some());
        assert!(profile_for("x@outlook.com").is_err());
    }
}
