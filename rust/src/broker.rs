//! OpenSSH forced-command broker. SSH authenticates a per-device private key;
//! the trusted authorized_keys entry pins device ID, source ranges and command.
//! Never install this on an account whose unrestricted shell the Agent can use.
//! Device policy and root key stay on the broker; no master key crosses SSH.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::IpAddr;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::model::{now_utc, parse_ref};
use crate::store::{harden, Store};

pub const POLICY_FILE: &str = "broker-devices.json";
pub const AUDIT_FILE: &str = "broker-audit.jsonl";
const REQUEST_LIMIT: u64 = 16 * 1024;
const RESPONSE_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub id: String,
    pub principal: String,
    pub public_key: String,
    pub fingerprint: String,
    /// Local OS administrator's approval label, not a verified SSH signer.
    pub approved_by: String,
    pub approved_at: String,
    pub allow_ips: Vec<String>,
    pub refs: Vec<String>,
    pub expires_unix: u64,
    pub revoked: bool,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    #[serde(default)]
    devices: BTreeMap<String, Device>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    /// "inject" fetches only explicitly allowed refs; "probe" returns no values.
    pub operation: String,
    #[serde(default)]
    pub refs: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub request_id: String,
    pub device: String,
    pub values: BTreeMap<String, String>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn validate_leaf_ref(value: &str) -> Result<()> {
    if value.len() > 1024 || value.chars().any(char::is_control) || !value.is_ascii() {
        bail!("凭据 ref 必须是无控制字符的 ASCII URI");
    }
    let (platform, name) = parse_ref(value)?;
    if platform == "server"
        && !name
            .split_once('/')
            .map(|(host, field)| !host.is_empty() && !field.is_empty())
            .unwrap_or(false)
    {
        bail!("服务器授权必须到 host/field，不能授予整台服务器聚合对象");
    }
    if platform == "cli"
        && !name
            .split_once('/')
            .map(|(cli, profile)| !cli.is_empty() && !profile.is_empty())
            .unwrap_or(false)
    {
        bail!("CLI 授权必须到 cli/profile");
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.:@".contains(&c))
}

fn load(store: &Store) -> Result<Policy> {
    match fs::read(store.root().join(POLICY_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("broker policy 损坏，拒绝使用"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Policy::default()),
        Err(e) => Err(e.into()),
    }
}

fn save(store: &Store, policy: &Policy) -> Result<()> {
    let tmp = store.root().join("broker-devices.json.tmp");
    // Serialize before writing: malformed updates must not truncate existing policy.
    let bytes = serde_json::to_vec_pretty(policy)?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    harden(&tmp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(tmp, store.root().join(POLICY_FILE))?;
    // Persist the directory entry too: a durable revocation must survive a crash.
    #[cfg(unix)]
    fs::File::open(store.root())?.sync_all()?;
    Ok(())
}

/// Public key is parsed by OpenSSH, never by hashing a string as encryption key.
pub fn validate_public_key(path: &Path) -> Result<(String, String)> {
    let text = fs::read_to_string(path).context("读取设备公钥失败")?;
    validate_public_key_text(&text)
}

fn validate_public_key_text(text: &str) -> Result<(String, String)> {
    let lines: Vec<_> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() != 1 {
        bail!("每个设备只接受一行 SSH 公钥");
    }
    let parts: Vec<_> = lines[0].split_whitespace().collect();
    if parts.len() < 2
        || !matches!(
            parts[0],
            "ssh-ed25519"
                | "ssh-rsa"
                | "ecdsa-sha2-nistp256"
                | "ecdsa-sha2-nistp384"
                | "ecdsa-sha2-nistp521"
        )
    {
        bail!("只接受 OpenSSH 公钥，不接受 authorized_keys options、证书或私钥");
    }
    let key = format!("{} {}", parts[0], parts[1]);
    let mut child = Command::new("ssh-keygen")
        .args(["-l", "-E", "sha256", "-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("校验公钥需要 OpenSSH ssh-keygen")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("无法写公钥"))?
        .write_all(key.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("ssh-keygen 拒绝此设备公钥");
    }
    let line = String::from_utf8(out.stdout)?;
    let fields: Vec<_> = line.split_whitespace().collect();
    let fp = fields
        .get(1)
        .filter(|fp| fp.starts_with("SHA256:"))
        .ok_or_else(|| anyhow!("无法读取公钥指纹"))?;
    let bits = fields
        .first()
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(0);
    if parts[0] == "ssh-rsa" && bits < 3072 {
        bail!("RSA 设备公钥至少需要 3072 位");
    }
    Ok((key, fp.to_string()))
}

fn network(input: &str) -> Result<(IpAddr, u8)> {
    let (ip, prefix) = match input.split_once('/') {
        Some((ip, n)) => (ip.parse::<IpAddr>()?, Some(n.parse::<u8>()?)),
        None => (input.parse::<IpAddr>()?, None),
    };
    let bits = if ip.is_ipv4() { 32 } else { 128 };
    let prefix = prefix.unwrap_or(bits);
    if prefix > bits {
        bail!("无效 IP 网段 {input}");
    }
    Ok((ip, prefix))
}

pub fn ip_allowed(actual: IpAddr, ranges: &[String]) -> Result<bool> {
    let mut allowed = false;
    for value in ranges {
        let (base, prefix) = network(value)?;
        allowed |= match (actual, base) {
            (IpAddr::V4(a), IpAddr::V4(b)) => {
                let mask = if prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - prefix)
                };
                (u32::from(a) & mask) == (u32::from(b) & mask)
            }
            (IpAddr::V6(a), IpAddr::V6(b)) => {
                let mask = if prefix == 0 {
                    0
                } else {
                    u128::MAX << (128 - prefix)
                };
                (u128::from(a) & mask) == (u128::from(b) & mask)
            }
            _ => false,
        };
    }
    Ok(allowed)
}

pub fn register(store: &Store, mut device: Device, ttl_hours: u64) -> Result<Device> {
    if !valid_id(&device.id) || !valid_id(&device.principal) {
        bail!("设备/身份 ID 仅可含字母、数字和 -_.:@");
    }
    let (public_key, fingerprint) = validate_public_key_text(&device.public_key)?;
    device.public_key = public_key;
    device.fingerprint = fingerprint;
    if device.approved_by.trim().is_empty() {
        bail!("需要登记本地审批者标签");
    }
    if device.allow_ips.is_empty()
        || device.refs.is_empty()
        || device.allow_ips.len() > 64
        || device.refs.len() > 64
    {
        bail!("设备必须有 1 到 64 条 IP 白名单和精确凭据授权");
    }
    for ip in &device.allow_ips {
        network(ip)?;
    }
    if !(1..=8760).contains(&ttl_hours) {
        bail!("设备授权期限须在 1 到 8760 小时内");
    }
    let known: BTreeSet<_> = store
        .list_secrets()?
        .into_iter()
        .map(|m| m.r#ref())
        .collect();
    for r in &device.refs {
        validate_leaf_ref(r)?;
        if !known.contains(r) {
            bail!("授权条目 {r} 不存在（只接受逐条 ref，不接受通配或整库）");
        }
    }
    device.refs.sort();
    device.refs.dedup();
    device.approved_at = now_utc();
    device.expires_unix = now_secs()
        .checked_add(ttl_hours * 3600)
        .ok_or_else(|| anyhow!("有效期溢出"))?;
    device.revoked = false;
    let _guard = store.lock()?;
    let mut policy = load(store)?;
    if policy.devices.contains_key(&device.id) {
        bail!("设备 ID 已存在；请撤销后以新 ID 登记");
    }
    if policy
        .devices
        .values()
        .any(|d| d.fingerprint == device.fingerprint && !d.revoked)
    {
        bail!("此 SSH key 已被另一台有效设备使用；每台设备使用独立私钥");
    }
    policy.devices.insert(device.id.clone(), device.clone());
    admin_event(store, &device, "register")?;
    save(store, &policy)?;
    Ok(device)
}

/// 从 CortexOS 舰队台账「领养」一台已登记的机器。
///
/// 设备身份的唯一真源是 cs 的 `registry/fleet-hosts.json`（id / public_ip /
/// agent_id），kyvault 不另存一份 —— 两份设备表必然漂移，而漂移的那一份
/// 往往是安全策略。这里只补 kyvault 独有的东西：公钥指纹与逐条 ref 授权。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FleetHost {
    pub id: String,
    #[serde(default)]
    pub public_ip: String,
    #[serde(default)]
    pub ssh_alias: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub ssh_key_ref: String,
}

/// 解析舰队台账。只认 `servers` 数组；结构不对就报错，不猜。
pub fn load_fleet(path: &Path) -> Result<Vec<FleetHost>> {
    let text = fs::read_to_string(path).with_context(|| {
        format!(
            "读不到舰队台账 {}（cs 的 registry/fleet-hosts.json）",
            path.display()
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&text).context("舰队台账不是合法 JSON")?;
    let servers = value
        .get("servers")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("舰队台账缺少 servers 数组"))?;
    let mut hosts = Vec::new();
    for item in servers {
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").trim();
        if id.is_empty() {
            bail!("舰队台账有主机缺 id");
        }
        hosts.push(FleetHost {
            id: id.to_string(),
            public_ip: item
                .get("public_ip")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            ssh_alias: item
                .get("ssh_alias")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            agent_id: item
                .get("agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            ssh_key_ref: item
                .get("ssh_key_ref")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
        });
    }
    Ok(hosts)
}

/// 同一公网 IP 上的其他主机。IP 共享时它不能当设备身份，只能当网络门禁。
pub fn hosts_sharing_ip(fleet: &[FleetHost], host: &FleetHost) -> Vec<String> {
    if host.public_ip.is_empty() {
        return Vec::new();
    }
    fleet
        .iter()
        .filter(|h| h.id != host.id && h.public_ip == host.public_ip)
        .map(|h| h.id.clone())
        .collect()
}

/// 领养：身份取自台账，授权由本次参数决定。返回（设备, 同 IP 的其他主机）。
pub fn adopt(
    store: &Store,
    fleet: &[FleetHost],
    host_id: &str,
    public_key: &str,
    refs: Vec<String>,
    ttl_hours: u64,
    approved_by: &str,
) -> Result<(Device, Vec<String>)> {
    let host = fleet
        .iter()
        .find(|h| h.id == host_id)
        .ok_or_else(|| anyhow!("舰队台账里没有 {host_id}（先 cs server list 确认 id）"))?;
    if host.public_ip.is_empty() {
        bail!("{host_id} 在台账里没有 public_ip，无法建立网络门禁；请先补台账再领养");
    }
    let (key, fingerprint) = validate_public_key_text(public_key)?;
    let principal = if host.agent_id.is_empty() {
        format!("host:{host_id}")
    } else {
        format!("agent:{}", host.agent_id)
    };
    let device = Device {
        id: host.id.clone(),
        principal,
        public_key: key,
        fingerprint,
        approved_by: approved_by.to_string(),
        approved_at: String::new(),
        // /32：台账写的是单机公网地址，不做网段放大
        allow_ips: vec![format!("{}/32", host.public_ip)],
        refs,
        expires_unix: 0,
        revoked: false,
    };
    let shared = hosts_sharing_ip(fleet, host);
    // 重复批准 = 续期/改授权，不是错误。只有显式 revoke 才是「停用」。
    let already = list(store)?.into_iter().any(|d| d.id == host.id);
    if already {
        forget(store, &host.id)?;
    }
    let registered = register(store, device, ttl_hours)?;
    Ok((registered, shared))
}

/// 从策略里彻底移除一台设备（用于「重新批准」时覆盖旧条目）。
/// 与 revoke 的区别：revoke 保留记录作为停用证据，forget 只为让位给新批准。
fn forget(store: &Store, id: &str) -> Result<()> {
    let _guard = store.lock()?;
    let mut policy = load(store)?;
    if policy.devices.remove(id).is_none() {
        return Ok(());
    }
    event(
        store,
        &serde_json::json!({"ts": now_utc(), "action": "reapprove-replaces", "device": id,
            "authentication": "local-os-admin"}),
    )?;
    save(store, &policy)
}

/// 服务器自助接入用的票据：一段可复制的文本，只含身份与公钥。
///
/// 为什么要有它：服务器通常在 NAT 后面，Mac 连不进去；但人可以把一小段文本
/// 复制到 Mac 上。票据让「申请」和「批准」不必同时在线，也不必开入站端口。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    pub v: u32,
    /// 建议的主机 ID，通常取 hostname
    pub host: String,
    /// 一行 OpenSSH 公钥
    pub key: String,
}

const TICKET_PREFIX: &str = "kyticket-";

pub fn ticket_new(host: &str, public_key: &str) -> Result<String> {
    if !valid_id(host) {
        bail!("hostname 含非法字符，请用 --id 指定一个（字母数字和 -_.:@）");
    }
    let (key, _fp) = validate_public_key_text(public_key)?;
    let payload = serde_json::to_vec(&Ticket {
        v: 1,
        host: host.to_string(),
        key,
    })?;
    use base64::Engine;
    Ok(format!(
        "{TICKET_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload)
    ))
}

pub fn ticket_parse(text: &str) -> Result<Ticket> {
    use base64::Engine;
    let body = text
        .trim()
        .strip_prefix(TICKET_PREFIX)
        .ok_or_else(|| anyhow!("不是 kyvault 票据（应以 {TICKET_PREFIX} 开头）——确认复制完整"))?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(body.trim())
        .map_err(|_| anyhow!("票据内容损坏，请重新复制"))?;
    let ticket: Ticket = serde_json::from_slice(&raw).map_err(|_| anyhow!("票据结构无法识别"))?;
    if ticket.v != 1 {
        bail!("票据版本 {} 不受支持（本机 kyvault 可能过旧）", ticket.v);
    }
    let (key, _fp) = validate_public_key_text(&ticket.key)?;
    if key != ticket.key {
        bail!("票据里的公钥不规范");
    }
    Ok(ticket)
}

/// 把 forced-command 行装进 authorized_keys。
///
/// 只追加、不改既有行；按设备 ID 去重（同设备重复 admit 不会堆出第二行）；
/// 写前备份。返回是否新增。
pub fn install_authorized_key(path: &Path, line: &str, device: &str) -> Result<bool> {
    let marker = format!("kyvault:{device}");
    let existing = fs::read_to_string(path).unwrap_or_default();
    if existing.lines().any(|l| l.trim_end().ends_with(&marker)) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }
    if path.exists() {
        fs::copy(path, path.with_extension("bak"))?;
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(line.trim_end());
    content.push('\n');
    let tmp = path.with_extension("tmp-install");
    fs::write(&tmp, content)?;
    harden(&tmp)?;
    fs::rename(&tmp, path)?;
    Ok(true)
}

pub fn list(store: &Store) -> Result<Vec<Device>> {
    Ok(load(store)?.devices.into_values().collect())
}

/// Metadata-only observability snapshot; no values or master key are read.
pub fn status(store: &Store) -> Result<serde_json::Value> {
    let devices = list(store)?;
    let now = now_secs();
    let mut recent = std::collections::VecDeque::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut last_seen = BTreeMap::<String, String>::new();
    if let Ok(file) = fs::File::open(store.root().join(AUDIT_FILE)) {
        use std::io::BufRead;
        for line in std::io::BufReader::new(file).lines() {
            let line = line?;
            let item: serde_json::Value = serde_json::from_str(&line).context("服务端审计损坏")?;
            let action = item["action"].as_str().unwrap_or("unknown").to_string();
            *counts.entry(action).or_default() += 1;
            if let (Some(id), Some(ts)) = (item["device"].as_str(), item["ts"].as_str()) {
                if item["authentication"] == "openssh-forced-command" && item["decision"] == "allow"
                {
                    last_seen.insert(id.to_string(), ts.to_string());
                }
            }
            recent.push_back(item);
            if recent.len() > 20 {
                recent.pop_front();
            }
        }
    } else if store.root().join(AUDIT_FILE).exists() {
        bail!("服务端审计不可读");
    }
    let devices: Vec<_> = devices.into_iter().map(|d| serde_json::json!({
        "id":d.id, "principal_label":d.principal, "fingerprint":d.fingerprint,
        "source_ranges":d.allow_ips, "grant_refs":d.refs, "expires_unix":d.expires_unix,
        "state":if d.revoked {"revoked"} else if now >= d.expires_unix {"expired"} else {"active"},
        "approved_by_label":d.approved_by, "last_authorized_at":last_seen.get(&d.id),
    })).collect();
    Ok(
        serde_json::json!({"generated_at":now_utc(), "devices":devices,
        "event_counts":counts,"recent_events":recent,
        "trust":"SSH identity is verified only when installed as a trusted forced command; OS administrator can edit policy and audit"}),
    )
}

pub fn revoke(store: &Store, id: &str) -> Result<()> {
    let _guard = store.lock()?;
    let mut policy = load(store)?;
    let device = policy
        .devices
        .get_mut(id)
        .ok_or_else(|| anyhow!("未登记设备 {id}"))?;
    device.revoked = true;
    admin_event(store, device, "revoke")?;
    save(store, &policy)
}

fn shell_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

/// Returns a reviewable line; never edits the user's actual authorized_keys.
pub fn authorized_key(store: &Store, id: &str, executable: &Path) -> Result<String> {
    let d = load(store)?
        .devices
        .remove(id)
        .ok_or_else(|| anyhow!("未登记设备"))?;
    if d.revoked || now_secs() >= d.expires_unix {
        bail!("设备已撤销或授权过期");
    }
    if !valid_id(&d.id) || d.allow_ips.is_empty() || d.allow_ips.len() > 64 {
        bail!("设备策略结构错误");
    }
    for range in &d.allow_ips {
        network(range)?;
    }
    let (key, fp) = validate_public_key_text(&d.public_key)?;
    if key != d.public_key || fp != d.fingerprint {
        bail!("设备公钥与指纹不匹配");
    }
    if !executable.is_absolute() || !store.root().is_absolute() {
        bail!("broker 程序与 store 必须使用绝对路径");
    }
    let command = format!(
        "{} broker serve --store {} --device {}",
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&store.root().to_string_lossy()),
        shell_quote(&d.id)
    );
    let command = command.replace('\\', "\\\\").replace('"', "\\\"");
    if command.contains(['\r', '\n']) {
        bail!("路径不能包含换行");
    }
    // 'restrict' disables PTY, agent/TCP/X11 forwarding and ~/.ssh/rc execution.
    Ok(format!(
        "restrict,from=\"{}\",command=\"{}\" {} kyvault:{}",
        d.allow_ips.join(","),
        command,
        d.public_key,
        d.id
    ))
}

fn source_ip(connection: &str) -> Result<IpAddr> {
    let values: Vec<_> = connection.split_whitespace().collect();
    if values.len() != 4 {
        bail!("只接受 sshd 创建的 SSH_CONNECTION");
    }
    values[1].parse::<u16>()?;
    values[3].parse::<u16>()?;
    values[2].parse::<IpAddr>()?;
    Ok(values[0].parse()?)
}

pub fn authorize(device: &Device, request: &Request, ip: IpAddr, now: u64) -> Result<()> {
    if device.revoked {
        bail!("设备已撤销");
    }
    if now >= device.expires_unix {
        bail!("设备授权过期");
    }
    if !ip_allowed(ip, &device.allow_ips)? {
        bail!("来源 IP 不在白名单");
    }
    if request.version != 1 {
        bail!("不支持的请求版本");
    }
    if request.refs.len() > 64 {
        bail!("单次最多请求 64 条");
    }
    match request.operation.as_str() {
        "probe" if request.refs.is_empty() => Ok(()),
        "inject" if !request.refs.is_empty() => {
            for r in &request.refs {
                validate_leaf_ref(r)?;
                if !device.refs.contains(r) {
                    bail!("凭据未获授权");
                }
            }
            Ok(())
        }
        _ => Err(anyhow!("只支持 probe 或逐 ref 的 inject")),
    }
}

/// Required broker audit: not affected by KY_AUDIT=0 and fails closed on write error.
fn event(store: &Store, value: &serde_json::Value) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).append(true).read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(store.root().join(AUDIT_FILE))
        .context("无法写服务端审计，拒绝释放凭据")?;
    file.lock_exclusive()?;
    harden(&store.root().join(AUDIT_FILE))?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    FileExt::unlock(&file)?;
    Ok(())
}

fn admin_event(store: &Store, device: &Device, action: &str) -> Result<()> {
    event(
        store,
        &serde_json::json!({"ts": now_utc(), "action":action, "device":device.id,
        "principal":device.principal, "actor_label":device.approved_by,
        "authentication":"local-os-admin", "refs":device.refs}),
    )
}

/// Core request processing is separate from stdio for denial/secret-leak regression tests.
pub fn process(store: &Store, id: &str, connection: &str, request: Request) -> Result<Response> {
    let ip = source_ip(connection)?;
    let _guard = store.lock()?;
    let policy = load(store)?;
    let device = policy
        .devices
        .get(id)
        .ok_or_else(|| anyhow!("设备未登记"))?;
    let request_id = format!("{:032x}", rand::random::<u128>());
    let decision = authorize(device, &request, ip, now_secs());
    // Never audit raw client input: it might contain a secret disguised as a ref.
    let authorized_refs: Vec<_> = request
        .refs
        .iter()
        .filter(|r| device.refs.contains(r))
        .cloned()
        .collect();
    event(
        store,
        &serde_json::json!({"ts":now_utc(), "request_id":request_id,
        "device":device.id,"principal":device.principal,"fingerprint":device.fingerprint,
        "authentication":"openssh-forced-command","source_ip":ip.to_string(),
        "action":if request.operation == "probe" {"probe"} else {"inject"},
        "decision":if decision.is_ok() {"allow"} else {"deny"},"refs":authorized_refs,
        "requested_count":request.refs.len()}),
    )?;
    decision?;
    let mut values = BTreeMap::new();
    for r in &request.refs {
        let value = match store.get_secret(r) {
            Ok(Some(v)) => v,
            _ => {
                event(
                    store,
                    &serde_json::json!({"ts":now_utc(),"request_id":request_id,
                    "device":id,"action":"prepare","result":"error"}),
                )?;
                bail!("获准凭据当前不可读取");
            }
        };
        values.insert(r.clone(), value);
        if values.values().map(|v: &String| v.len()).sum::<usize>() as u64 > RESPONSE_LIMIT / 8 {
            bail!("获准凭据集合过大，拒绝交付");
        }
    }
    if now_secs() >= device.expires_unix {
        bail!("准备响应时授权已过期");
    }
    event(
        store,
        &serde_json::json!({"ts":now_utc(),"request_id":request_id,"device":id,
        "principal":device.principal,"action":"prepared","result":"ok","refs":authorized_refs}),
    )?;
    Ok(Response {
        version: 1,
        request_id,
        device: id.to_string(),
        values,
    })
}

/// Must be called only as the fixed forced command for this registered key.
pub fn serve(store: &Store, device: &str) -> Result<()> {
    let result = serve_inner(store, device);
    if result.is_err() {
        let _guard = store.lock()?;
        // The device ID comes from the trusted forced command, not client input.
        event(
            store,
            &serde_json::json!({"ts":now_utc(),
            "device":if valid_id(device) {device} else {"invalid-fixed-command"},
            "action":"request-rejected", "decision":"deny"}),
        )?;
    }
    result
}

fn serve_inner(store: &Store, device: &str) -> Result<()> {
    let connection = std::env::var("SSH_CONNECTION")
        .context("broker serve 仅允许通过 SSH forced command 访问")?;
    if std::env::var("SSH_ORIGINAL_COMMAND")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        bail!("代理只接受标准输入 JSON，拒绝客户端远程命令");
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut raw = Vec::new();
        let result = std::io::stdin()
            .take(REQUEST_LIMIT + 1)
            .read_to_end(&mut raw)
            .map(|_| raw);
        let _ = sender.send(result);
    });
    let raw = receiver
        .recv_timeout(std::time::Duration::from_secs(15))
        .map_err(|_| anyhow!("代理请求未在 15 秒内结束"))??;
    if raw.len() as u64 > REQUEST_LIMIT {
        bail!("代理请求过大");
    }
    let request: Request = serde_json::from_slice(&raw).map_err(|_| anyhow!("代理请求格式错误"))?;
    let response = process(store, device, &connection, request)?;
    let bytes = serde_json::to_vec(&response)?;
    if bytes.len() as u64 > RESPONSE_LIMIT {
        bail!("代理响应过大，拒绝输出");
    }
    std::io::stdout().lock().write_all(&bytes)?;
    Ok(())
}

/// SSH handles private-key proof, encryption and host identity. Strict host checking
/// prevents accepting an unverified host key; existing ControlMaster sessions are disabled.
pub fn fetch(
    host: &str,
    identity: Option<&Path>,
    expected_device: &str,
    ssh_config: Option<&Path>,
    request: &Request,
) -> Result<Response> {
    if !valid_id(expected_device) {
        bail!("必须提供有效的预期设备 ID");
    }
    for reference in &request.refs {
        validate_leaf_ref(reference)?;
    }
    if host.is_empty()
        || host.starts_with('-')
        || host.chars().any(char::is_whitespace)
        || host.contains(['\0', '\n', '\r'])
    {
        bail!("请使用已在 SSH config 配置的 broker 主机别名或 user@host");
    }
    let mut cmd = Command::new("ssh");
    if let Some(config) = ssh_config {
        cmd.arg("-F").arg(config);
    }
    cmd.args([
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "PasswordAuthentication=no",
        "-o",
        "KbdInteractiveAuthentication=no",
        "-o",
        "PreferredAuthentications=publickey",
        "-o",
        "ForwardAgent=no",
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-o",
        "PermitLocalCommand=no",
        "-o",
        "RequestTTY=no",
        "-o",
        "RemoteCommand=none",
        "-o",
        "ConnectTimeout=15",
    ]);
    if let Some(path) = identity {
        cmd.arg("-i").arg(path).args(["-o", "IdentitiesOnly=yes"]);
    }
    // No client command follows host: sshd runs the trusted fixed authorized_keys command.
    let mut child = cmd
        .arg(host)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("无法启动 OpenSSH")?;
    let input = serde_json::to_vec(request)?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("SSH stdin 不可用"))?
        .write_all(&input)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("SSH stdout 不可用"))?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let result = stdout
            .take(RESPONSE_LIMIT + 1)
            .read_to_end(&mut out)
            .map(|_| out);
        let _ = sender.send(result);
    });
    let out = match receiver.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(Ok(out)) => out,
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            bail!("SSH broker 响应失败或超过 30 秒");
        }
    };
    let _ = reader.join();
    if out.len() as u64 > RESPONSE_LIMIT {
        let _ = child.kill();
        let _ = child.wait();
        bail!("代理响应超过限制");
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    // stdout may close before the remote process exits; bound this final wait too.
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("SSH broker 未正常退出");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    if !status.success() {
        bail!(
            "SSH broker 请求失败（退出码 {:?}）；检查主机 pin、独立设备 key 和 forced command 配置",
            status.code()
        );
    }
    let response: Response =
        serde_json::from_slice(&out).map_err(|_| anyhow!("broker 响应格式错误"))?;
    if response.values.values().any(|v| v.contains('\0')) {
        bail!("代理凭据包含无法注入环境的空字节");
    }
    let expected: BTreeSet<_> = request.refs.iter().collect();
    let received: BTreeSet<_> = response.values.keys().collect();
    if response.version != 1 || response.device != expected_device || received != expected {
        bail!("broker 返回的版本、设备或凭据集合不匹配");
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn device() -> Device {
        Device {
            id: "prod-1".into(),
            principal: "agent:deploy".into(),
            public_key:
                "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOv3Q1J3Wompk6Ke5fKU8bQrjYAlBlG/kySLh8vh8J3O"
                    .into(),
            fingerprint: String::new(),
            approved_by: "owner-mac".into(),
            approved_at: now_utc(),
            allow_ips: vec!["192.0.2.0/24".into(), "2001:db8::/32".into()],
            refs: vec!["secret://example/token".into()],
            expires_unix: now_secs() + 3600,
            revoked: false,
        }
    }
    fn request() -> Request {
        Request {
            version: 1,
            operation: "inject".into(),
            refs: vec!["secret://example/token".into()],
        }
    }
    #[test]
    fn cidr_checks_both_families_and_invalid_policy() {
        let ips = device().allow_ips;
        assert!(ip_allowed("192.0.2.9".parse().unwrap(), &ips).unwrap());
        assert!(!ip_allowed("198.51.100.9".parse().unwrap(), &ips).unwrap());
        assert!(ip_allowed("2001:db8::7".parse().unwrap(), &ips).unwrap());
        assert!(!ip_allowed("::ffff:192.0.2.9".parse().unwrap(), &ips).unwrap());
        assert!(ip_allowed("127.0.0.1".parse().unwrap(), &["0.0.0.0/33".into()]).is_err());
    }
    #[test]
    fn exact_ref_source_expiry_revocation_are_all_required() {
        let mut d = device();
        let mut r = request();
        let ip = "192.0.2.9".parse().unwrap();
        assert!(authorize(&d, &r, ip, now_secs()).is_ok());
        assert!(authorize(&d, &r, "198.51.100.9".parse().unwrap(), now_secs()).is_err());
        assert!(authorize(&d, &r, ip, d.expires_unix).is_err());
        r.refs.push("secret://example/other".into());
        assert!(authorize(&d, &r, ip, now_secs()).is_err());
        r = request();
        d.revoked = true;
        assert!(authorize(&d, &r, ip, now_secs()).is_err());
    }
    #[test]
    fn process_releases_only_granted_values_and_audits_denial() {
        let tmp = tempfile::tempdir().unwrap();
        let st = Store::new(tmp.path());
        st.init_master_key().unwrap();
        st.set_secret("secret://example/token", "dummy-private-value")
            .unwrap();
        register(&st, device(), 1).unwrap();
        let response = process(&st, "prod-1", "192.0.2.9 1234 192.0.2.1 22", request()).unwrap();
        assert_eq!(response.values.len(), 1);
        assert_eq!(
            response.values["secret://example/token"],
            "dummy-private-value"
        );
        let mut r = request();
        r.refs.push("secret://example/other".into());
        assert!(process(&st, "prod-1", "192.0.2.9 1234 192.0.2.1 22", r).is_err());
        let audit = fs::read_to_string(st.root().join(AUDIT_FILE)).unwrap();
        assert!(!audit.contains("dummy-private-value"));
        assert!(audit.contains("deny"));
        revoke(&st, "prod-1").unwrap();
        assert!(process(&st, "prod-1", "192.0.2.9 1234 192.0.2.1 22", request()).is_err());
    }
    #[test]
    fn malformed_policy_never_becomes_an_empty_allow_list() {
        let tmp = tempfile::tempdir().unwrap();
        let st = Store::new(tmp.path());
        fs::write(st.root().join(POLICY_FILE), "{").unwrap();
        assert!(list(&st).is_err());
    }
    #[test]
    fn mandatory_audit_failure_prevents_release() {
        let tmp = tempfile::tempdir().unwrap();
        let st = Store::new(tmp.path());
        st.init_master_key().unwrap();
        st.set_secret("secret://example/token", "dummy-private-value")
            .unwrap();
        register(&st, device(), 1).unwrap();
        fs::remove_file(st.root().join(AUDIT_FILE)).unwrap();
        fs::create_dir(st.root().join(AUDIT_FILE)).unwrap();
        assert!(process(&st, "prod-1", "192.0.2.9 1234 192.0.2.1 22", request()).is_err());
    }

    #[test]
    fn fleet_adoption_reuses_ledger_identity_and_flags_shared_ip() {
        let tmp = tempfile::tempdir().unwrap();
        let st = Store::new(tmp.path());
        st.init_master_key().unwrap();
        st.set_secret("secret://example/token", "dummy-private-value")
            .unwrap();
        let host = |id: &str, ip: &str, agent: &str| FleetHost {
            id: id.into(),
            public_ip: ip.into(),
            ssh_alias: id.into(),
            agent_id: agent.into(),
            ssh_key_ref: String::new(),
        };
        let fleet = vec![
            host("gigi", "43.133.71.10", "gigi"),
            // 同机第二个 agent：台账里就是同一个公网 IP
            host("xiaomei", "43.133.71.10", "xiaomei"),
            host("no-ip", "", ""),
        ];
        let key = device().public_key;
        let (adopted, shared) = adopt(
            &st,
            &fleet,
            "gigi",
            &key,
            vec!["secret://example/token".into()],
            1,
            "owner-mac",
        )
        .unwrap();
        assert_eq!(adopted.id, "gigi");
        assert_eq!(adopted.principal, "agent:gigi");
        assert_eq!(adopted.allow_ips, vec!["43.133.71.10/32"]);
        // IP 相同不等于同一设备：只提示，不合并身份
        assert_eq!(shared, vec!["xiaomei".to_string()]);
        // 台账缺 IP 时拒绝领养，而不是退化成「任意来源」
        assert!(adopt(&st, &fleet, "no-ip", &key, vec![], 1, "x").is_err());
        assert!(adopt(&st, &fleet, "nope", &key, vec![], 1, "x").is_err());
    }

    #[test]
    fn ticket_roundtrip_rejects_tampering_and_bad_version() {
        let key = device().public_key;
        let ticket = ticket_new("nanzhu", &key).unwrap();
        let parsed = ticket_parse(&ticket).unwrap();
        assert_eq!(parsed.host, "nanzhu");
        assert_eq!(parsed.key, key);
        // 非法 hostname 不生成票据
        assert!(ticket_new("../etc", &key).is_err());
        // 非票据文本、截断、错版本都要拒绝
        assert!(ticket_parse("ssh-ed25519 AAAA").is_err());
        assert!(ticket_parse(&ticket[..ticket.len() - 6]).is_err());
        use base64::Engine;
        let v2 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(format!(r#"{{"v":2,"host":"x","key":"{key}"}}"#));
        assert!(ticket_parse(&format!("kyticket-{v2}"))
            .unwrap_err()
            .to_string()
            .contains("版本"));
    }

    #[test]
    fn installing_authorized_key_is_idempotent_and_keeps_existing_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".ssh/authorized_keys");
        let line = "restrict,from=\"203.0.113.10\",command=\"x\" ssh-ed25519 AAAA kyvault:nanzhu";
        assert!(install_authorized_key(&path, line, "nanzhu").unwrap());
        assert!(!install_authorized_key(&path, line, "nanzhu").unwrap());
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches("kyvault:nanzhu").count(), 1);
        // 既有他人的行不被覆盖
        let other = "ssh-ed25519 BBBB someone@else\n";
        let path2 = tmp.path().join("other_keys");
        fs::write(&path2, other).unwrap();
        assert!(install_authorized_key(&path2, line, "nanzhu").unwrap());
        let content = fs::read_to_string(&path2).unwrap();
        assert!(content.starts_with(other));
        assert!(content.contains("kyvault:nanzhu"));
    }
}
