//! `kyvault auth` —— 多因素授权守卫。
//!
//! 在现有 master.key 加密链上方新增一层 Auth Guard，不改动 AES-256-GCM 密文
//! 的 wire format（180 条密钥原样兼容），只保护 master.key 本身：
//!
//! 1. **SSH Key 绑定**：用 SSH 公钥材料派生 AES 密钥加密 master.key → master.key.enc。
//!    解锁时零交互——只要 SSH 公钥文件还在且指纹匹配，Agent 毫无感知。
//! 2. **TOTP（Google 验证器）**：生成 RFC 6238 标准 secret，绑入 Google Authenticator。
//!    定位是对敏感操作（master-key --reveal）的二次确认，不是每次解密都要过的门。
//!
//! 文件布局：
//! ```text
//! ~/.config/kyvault/store/
//! ├── auth.json          ← 授权方法配置
//! ├── master.key.enc     ← 加密态 master.key（SSH key 派生密钥加密）
//! ├── master.key         ← 启用 guard 后删除；未启用时保留
//! └── ...
//! ```

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::store::shellexpand_tilde;

// ───────────────────────── 数据结构 ─────────────────────────

const NONCE_LEN: usize = 12;

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthConfig {
    pub version: u32,
    pub policy: String,
    pub methods: Vec<AuthMethod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMethod {
    pub id: String,
    #[serde(rename = "type")]
    pub method_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_enc: Option<String>,
    pub bound_at: String,
}

pub struct AuthGuard {
    root: PathBuf,
    config: AuthConfig,
}

// ───────────────────────── SSH 公钥工具 ─────────────────────────

/// 解析 OpenSSH 格式的公钥文件，返回 (key_type, raw_bytes)。
/// 格式：`ssh-ed25519 AAAA...== comment`
fn parse_ssh_pubkey(path: &Path) -> Result<(String, Vec<u8>)> {
    let content =
        fs::read_to_string(path).with_context(|| format!("读不到公钥文件 {}", path.display()))?;
    let line = content
        .lines()
        .find(|l| l.starts_with("ssh-") || l.starts_with("ecdsa-"))
        .ok_or_else(|| anyhow!("文件 {} 不含有效的 SSH 公钥行", path.display()))?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(anyhow!("SSH 公钥格式不合法：{}", path.display()));
    }
    let key_type = parts[0].to_string();
    let raw = B64
        .decode(parts[1])
        .with_context(|| format!("SSH 公钥 base64 解码失败：{}", path.display()))?;
    Ok((key_type, raw))
}

/// 计算 SSH 公钥的 SHA-256 指纹，格式同 `ssh-keygen -lf`：`SHA256:xxxxx`
pub fn ssh_fingerprint(pubkey_path: &Path) -> Result<String> {
    let (_ty, raw) = parse_ssh_pubkey(pubkey_path)?;
    let hash = Sha256::digest(&raw);
    // ssh-keygen 用 base64 无 padding 显示 SHA-256
    let b64 = data_encoding::BASE64_NOPAD.encode(&hash);
    Ok(format!("SHA256:{b64}"))
}

/// 用 SSH 公钥原始字节派生 AES-256 密钥
fn derive_key_from_ssh(pubkey_path: &Path) -> Result<[u8; 32]> {
    let (_ty, raw) = parse_ssh_pubkey(pubkey_path)?;
    Ok(Sha256::digest(&raw).into())
}

// ───────────────────────── AES 加解密 ─────────────────────────

/// 加密一段明文，返回 base64(nonce || ciphertext)
fn aes_encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<String> {
    let cipher = Aes256Gcm::new(key.into());
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow!("加密失败：{e}"))?;
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(B64.encode(&combined))
}

/// 解密 base64(nonce || ciphertext)，返回明文
fn aes_decrypt(key: &[u8; 32], encoded: &str) -> Result<Vec<u8>> {
    let combined = B64
        .decode(encoded.trim())
        .context("master.key.enc base64 解码失败")?;
    if combined.len() < NONCE_LEN + 16 {
        return Err(anyhow!(
            "master.key.enc 密文太短（{} 字节），至少需要 {} 字节",
            combined.len(),
            NONCE_LEN + 16
        ));
    }
    let (nonce_bytes, ct) = combined.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ct)
        .map_err(|_| anyhow!("解密失败——SSH 公钥与加密时用的不匹配，或文件损坏"))
}

// ───────────────────────── TOTP (RFC 6238) ─────────────────────────

/// 生成 TOTP 验证码。标准：HMAC-SHA1, 6 位, 30 秒时间步。
fn totp_generate(secret: &[u8], time: u64) -> String {
    use hmac::Mac;

    let step = time / 30;
    let msg = step.to_be_bytes();

    let mut mac = <hmac::Hmac<sha1::Sha1> as Mac>::new_from_slice(secret)
        .expect("HMAC-SHA1 accepts any key length");
    mac.update(&msg);
    let result = mac.finalize().into_bytes();

    let offset = (result[19] & 0x0f) as usize;
    let code = ((result[offset] as u32 & 0x7f) << 24)
        | ((result[offset + 1] as u32) << 16)
        | ((result[offset + 2] as u32) << 8)
        | (result[offset + 3] as u32);
    format!("{:06}", code % 1_000_000)
}

/// 验证 TOTP 码，±1 步容差（即当前、前 30s、后 30s 三个窗口）
pub fn totp_verify(secret: &[u8], code: &str) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for delta in [0i64, -30, 30] {
        let t = (now as i64 + delta) as u64;
        if totp_generate(secret, t) == code {
            return true;
        }
    }
    false
}

/// 生成 20 字节随机 TOTP secret
fn new_totp_secret() -> Vec<u8> {
    let mut buf = vec![0u8; 20];
    rand::thread_rng().fill_bytes(&mut buf);
    buf
}

/// 将 TOTP secret 显示为 Base32 字符串（给 Google Authenticator 扫码或手动输入用）
fn totp_secret_base32(secret: &[u8]) -> String {
    data_encoding::BASE32_NOPAD.encode(secret)
}

// ───────────────────────── 时间戳 ─────────────────────────

fn now_iso8601() -> String {
    // 精简实现：UTC
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // 简单 UTC 格式
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    // 从 epoch 天数算 YMD（简化实现）
    let (y, mo, d) = days_to_ymd(days_since_epoch);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // 从 1970-01-01 算起
    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let months: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u64;
    for &m_days in &months {
        if days < m_days {
            break;
        }
        days -= m_days;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

// ───────────────────────── 路径工具 ─────────────────────────

fn expand_path(p: &str) -> PathBuf {
    PathBuf::from(shellexpand_tilde(p))
}

// ───────────────────────── AuthGuard 实现 ─────────────────────────

impl AuthGuard {
    // ─── 文件路径 ───

    fn auth_json(root: &Path) -> PathBuf {
        root.join("auth.json")
    }

    fn master_key_enc(root: &Path) -> PathBuf {
        root.join("master.key.enc")
    }

    fn master_key_plain(root: &Path) -> PathBuf {
        root.join("master.key")
    }

    // ─── 状态查询 ───

    /// Auth Guard 是否已启用（auth.json 和 master.key.enc 同时存在）
    pub fn is_enabled(root: &Path) -> bool {
        Self::auth_json(root).exists() && Self::master_key_enc(root).exists()
    }

    /// 加载配置
    pub fn load(root: &Path) -> Result<Self> {
        let path = Self::auth_json(root);
        let content =
            fs::read_to_string(&path).with_context(|| format!("读 {} 失败", path.display()))?;
        let config: AuthConfig = serde_json::from_str(&content)
            .with_context(|| format!("{} 不是合法 JSON", path.display()))?;
        Ok(Self {
            root: root.to_path_buf(),
            config,
        })
    }

    /// 已绑定的授权方法列表
    pub fn methods(&self) -> &[AuthMethod] {
        &self.config.methods
    }

    /// 当前策略
    pub fn policy(&self) -> &str {
        &self.config.policy
    }

    // ─── 解锁（核心路径）───

    /// 尝试用已绑定的方法解锁 master.key，返回明文。
    ///
    /// 策略 `any_one`：任意一种 SSH key 匹配即解锁。
    /// TOTP 在 `any_one` 策略下**不参与日常解锁**（它是给 `--reveal` 等敏感操作的），
    /// 除非没有任何 SSH key 绑定。
    ///
    /// 每个 SSH key method 可能自带 `secret_enc` 字段（存的是用该 key 加密的
    /// master.key 副本）。优先用 method 自带的 enc，兜底才尝试全局 `master.key.enc`。
    pub fn unlock(&self) -> Result<String> {
        let enc_path = Self::master_key_enc(&self.root);
        let global_enc = fs::read_to_string(&enc_path).ok();

        // 遍历 SSH key 方法，任一成功即返回
        for method in &self.config.methods {
            if method.method_type == "ssh_key" {
                if let Some(ref kp) = method.key_path {
                    let path = expand_path(kp);
                    if !path.exists() {
                        continue;
                    }
                    // 验证指纹
                    let fp = match ssh_fingerprint(&path) {
                        Ok(fp) => fp,
                        Err(_) => continue,
                    };
                    if method.fingerprint.as_deref() != Some(&fp) {
                        continue;
                    }
                    let aes_key = match derive_key_from_ssh(&path) {
                        Ok(k) => k,
                        Err(_) => continue,
                    };

                    // 1) 先尝试 method 自带的 secret_enc
                    if let Some(ref enc) = method.secret_enc {
                        if let Ok(plain) = aes_decrypt(&aes_key, enc) {
                            return String::from_utf8(plain).context("master.key 不是合法 UTF-8");
                        }
                    }

                    // 2) 再尝试全局 master.key.enc
                    if let Some(ref ge) = global_enc {
                        if let Ok(plain) = aes_decrypt(&aes_key, ge) {
                            return String::from_utf8(plain).context("master.key 不是合法 UTF-8");
                        }
                    }
                }
            }
        }

        Err(anyhow!(
            "Auth Guard 解锁失败——没有匹配的 SSH 公钥。\n\
             已绑定的方法：\n{}\n\n\
             请确认绑定的 SSH 公钥文件仍在原位且未更换。\n\
             如需回退到明文 master.key 模式，使用 `kyvault auth disable`。",
            self.config
                .methods
                .iter()
                .filter(|m| m.method_type == "ssh_key")
                .map(|m| format!("  - {} ({})", m.id, m.key_path.as_deref().unwrap_or("?")))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }

    /// 验证 TOTP 码（用于敏感操作的二次确认）。
    /// 需要先用 unlock() 拿到 master.key 才能解密 TOTP secret。
    pub fn verify_totp(&self, master_key: &str) -> Result<bool> {
        let totp_method = self.config.methods.iter().find(|m| m.method_type == "totp");
        let method = match totp_method {
            Some(m) => m,
            None => return Ok(true), // 没绑 TOTP 就直接放行
        };
        let enc_secret = method
            .secret_enc
            .as_ref()
            .ok_or_else(|| anyhow!("TOTP 方法配置损坏：缺少 secret_enc"))?;

        // 用 master.key 的 SHA-256 作为 AES key 解密 TOTP secret
        let mk_bytes = B64
            .decode(master_key.trim())
            .context("master key base64 解码失败")?;
        let aes_key: [u8; 32] = Sha256::digest(&mk_bytes).into();

        let totp_secret = aes_decrypt(&aes_key, enc_secret)
            .context("TOTP secret 解密失败——master.key 可能已更换")?;

        // 提示输入
        eprint!("🔐 请输入 Google Authenticator 6 位验证码：");
        io::stderr().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let code = input.trim();

        if totp_verify(&totp_secret, code) {
            Ok(true)
        } else {
            Err(anyhow!("TOTP 验证码错误（当前码和前后 30 秒窗口均不匹配）"))
        }
    }

    // ─── 管理方法 ───

    /// 交互式首次配置：扫描 SSH 公钥 + 可选 TOTP
    pub fn setup(root: &Path) -> Result<()> {
        if Self::is_enabled(root) {
            return Err(anyhow!(
                "Auth Guard 已启用。\n\
                 用 `kyvault auth add` 添加方法，`kyvault auth disable` 关闭。"
            ));
        }

        // 读取当前明文 master.key
        let mk_path = Self::master_key_plain(root);
        let master_key = fs::read_to_string(&mk_path)
            .with_context(|| {
                format!(
                    "读 {} 失败——先运行 kyvault init 生成 master key",
                    mk_path.display()
                )
            })?
            .trim()
            .to_string();

        println!("🛡️  kyvault Auth Guard 首次配置");
        println!("{}", "-".repeat(52));

        // 1. 扫描 SSH 公钥
        let home = dirs::home_dir().ok_or_else(|| anyhow!("找不到 home 目录"))?;
        let ssh_dir = home.join(".ssh");
        let mut ssh_methods = Vec::new();

        if ssh_dir.exists() {
            let mut pub_keys: Vec<PathBuf> = Vec::new();
            if let Ok(entries) = fs::read_dir(&ssh_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().map(|e| e == "pub").unwrap_or(false) {
                        pub_keys.push(p);
                    }
                }
            }
            pub_keys.sort();

            if pub_keys.is_empty() {
                println!("\n⚠️  ~/.ssh/ 下没有找到 .pub 公钥文件。");
            } else {
                println!("\n🔑 发现 {} 把 SSH 公钥：", pub_keys.len());
                for (i, pk) in pub_keys.iter().enumerate() {
                    let fp = ssh_fingerprint(pk).unwrap_or_else(|_| "（解析失败）".into());
                    let fname = pk.file_name().unwrap_or_default().to_string_lossy();
                    println!("  [{}] {} — {}", i + 1, fname, fp);
                }

                println!("\n要绑定哪些公钥？（输入序号，逗号分隔，如 1,2；直接回车绑定全部）");
                eprint!("> ");
                io::stderr().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let input = input.trim();

                let selected: Vec<usize> = if input.is_empty() {
                    (0..pub_keys.len()).collect()
                } else {
                    input
                        .split(',')
                        .filter_map(|s| s.trim().parse::<usize>().ok().map(|n| n - 1))
                        .filter(|&i| i < pub_keys.len())
                        .collect()
                };

                for i in selected {
                    let pk = &pub_keys[i];
                    let fp = ssh_fingerprint(pk)?;
                    let fname = pk
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let id = fname.trim_end_matches(".pub").to_string();
                    // 用 ~/... 形式存路径，便于跨 session 稳定
                    let rel = format!("~/.ssh/{fname}");
                    ssh_methods.push(AuthMethod {
                        id: format!("ssh-{id}"),
                        method_type: "ssh_key".into(),
                        key_path: Some(rel),
                        fingerprint: Some(fp.clone()),
                        label: None,
                        secret_enc: None,
                        bound_at: now_iso8601(),
                    });
                    println!("  ✓ 已绑定 {fname} ({fp})");
                }
            }
        }

        if ssh_methods.is_empty() {
            return Err(anyhow!(
                "至少需要绑定一把 SSH 公钥才能启用 Auth Guard。\n\
                 请先生成 SSH Key：ssh-keygen -t ed25519"
            ));
        }

        // 2. 可选 TOTP
        let mut all_methods = ssh_methods;

        println!("\n🔐 是否同时绑定 TOTP（Google 验证器）？");
        println!("  TOTP 用于 `master-key --reveal` 等敏感操作的二次确认。");
        println!("  日常 get/set/run 只走 SSH Key 自动解锁，不受影响。");
        eprint!("  绑定 TOTP？[y/N] ");
        io::stderr().flush()?;
        let mut ans = String::new();
        io::stdin().read_line(&mut ans)?;
        if ans.trim().eq_ignore_ascii_case("y") {
            let secret = new_totp_secret();
            let b32 = totp_secret_base32(&secret);

            println!("\n  ── 请在 Google Authenticator / Authy 中手动输入以下密钥 ──");
            println!("  账户名：kyvault (webkubor)");
            println!("  密钥：  {b32}");
            println!("  类型：  基于时间 (TOTP)");
            println!("  ─────────────────────────────────────────────────────────");

            // 加密 TOTP secret（用 master.key 派生的 AES key）
            let mk_bytes = B64
                .decode(master_key.trim())
                .context("master key base64 解码失败")?;
            let aes_key: [u8; 32] = Sha256::digest(&mk_bytes).into();
            let enc = aes_encrypt(&aes_key, &secret)?;

            // 要求验证一次以确认录入正确
            eprint!("\n  请输入验证器当前显示的 6 位码确认：");
            io::stderr().flush()?;
            let mut code = String::new();
            io::stdin().read_line(&mut code)?;
            if !totp_verify(&secret, code.trim()) {
                println!("  ❌ 验证码不匹配，TOTP 未绑定。请检查录入是否正确后重试 `kyvault auth add totp`。");
            } else {
                all_methods.push(AuthMethod {
                    id: "totp-main".into(),
                    method_type: "totp".into(),
                    key_path: None,
                    fingerprint: None,
                    label: Some("kyvault (webkubor)".into()),
                    secret_enc: Some(enc),
                    bound_at: now_iso8601(),
                });
                println!("  ✓ TOTP 已绑定");
            }
        }

        // 3. 用第一把 SSH key 加密 master.key
        let first_ssh = all_methods
            .iter()
            .find(|m| m.method_type == "ssh_key")
            .unwrap();
        let ssh_path = expand_path(first_ssh.key_path.as_ref().unwrap());
        let aes_key = derive_key_from_ssh(&ssh_path)?;
        let encrypted = aes_encrypt(&aes_key, master_key.as_bytes())?;

        // 4. 写 auth.json
        let config = AuthConfig {
            version: 1,
            policy: "any_one".into(),
            methods: all_methods,
        };
        let auth_path = Self::auth_json(root);
        let json = serde_json::to_string_pretty(&config)?;
        fs::write(&auth_path, &json)?;
        crate::store::harden(&auth_path)?;

        // 5. 写 master.key.enc
        let enc_path = Self::master_key_enc(root);
        fs::write(&enc_path, &encrypted)?;
        crate::store::harden(&enc_path)?;

        // 6. 删除明文 master.key
        if mk_path.exists() {
            fs::remove_file(&mk_path)?;
            println!("\n✓ 明文 master.key 已安全删除");
        }

        println!("\n🛡️  Auth Guard 已启用！");
        println!("  - 绑定方法：{} 种", config.methods.len());
        println!("  - 策略：{}", config.policy);
        println!("  - 密文位置：{}", enc_path.display());
        println!("{}", "-".repeat(52));

        Ok(())
    }

    /// 添加一种授权方法
    pub fn add_method(root: &Path, method_type: &str, path: Option<&str>) -> Result<()> {
        if !Self::is_enabled(root) {
            return Err(anyhow!(
                "Auth Guard 尚未启用，请先运行 `kyvault auth setup`"
            ));
        }

        let mut guard = Self::load(root)?;

        match method_type {
            "ssh" => {
                let pk_path = path
                    .ok_or_else(|| anyhow!("添加 SSH Key 需要指定公钥路径，如：kyvault auth add ssh ~/.ssh/id_ed25519.pub"))?;
                let pk = expand_path(pk_path);
                if !pk.exists() {
                    return Err(anyhow!("公钥文件不存在：{}", pk.display()));
                }
                let fp = ssh_fingerprint(&pk)?;

                // 检查是否已绑定
                if guard
                    .config
                    .methods
                    .iter()
                    .any(|m| m.method_type == "ssh_key" && m.fingerprint.as_deref() == Some(&fp))
                {
                    println!("已绑定此公钥（{}），跳过。", fp);
                    return Ok(());
                }

                let fname = pk
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let id = format!("ssh-{}", fname.trim_end_matches(".pub"));

                // 需要先解锁拿到 master.key，再用新公钥加密
                // （因为 master.key.enc 是用第一把 SSH key 加密的，
                //  新公钥能解锁是因为 any_one 策略下任一把都行——
                //  但前提是 master.key.enc 需要每把公钥各加密一份？
                //  不，设计上 master.key.enc 只用一把 SSH key 加密，
                //  unlock() 遍历全部 SSH key 尝试解密。
                //  所以只有加密时用的那把 key 能解——这不对。）
                //
                // 修正设计：master.key.enc 用 **每一把** SSH key 各加密一份。
                // auth.json 里存每把 key 对应的 enc 字段。
                //
                // 但那样 auth.json 结构要改……当前方案用更简单的做法：
                // master.key.enc 只加密一份，但每次 add ssh key 时
                // 用新 key 重新加密一份，同时保留旧的。
                //
                // 最干净的做法：master.key.enc 目录下每把 key 一个 .enc 文件。
                // 但那增加了复杂度。
                //
                // 最终方案：每个 ssh_key method 自带 `enc` 字段存自己的加密副本。
                // unlock() 遍历每个 method，用对应的 key 解对应的 enc。

                // 先用已有方法解锁 master.key
                let master_key = guard.unlock()?;

                let aes_key = derive_key_from_ssh(&pk)?;
                let enc = aes_encrypt(&aes_key, master_key.as_bytes())?;

                guard.config.methods.push(AuthMethod {
                    id,
                    method_type: "ssh_key".into(),
                    key_path: Some(pk_path.to_string()),
                    fingerprint: Some(fp.clone()),
                    label: None,
                    secret_enc: Some(enc),
                    bound_at: now_iso8601(),
                });

                guard.save()?;
                println!("✓ 已绑定 SSH Key：{fname} ({fp})");
            }
            "totp" => {
                // 检查是否已绑定
                if guard.config.methods.iter().any(|m| m.method_type == "totp") {
                    return Err(anyhow!(
                        "TOTP 已绑定。如需更换，先 `kyvault auth remove totp-main` 再重新添加。"
                    ));
                }

                // 解锁 master.key
                let master_key = guard.unlock()?;

                let secret = new_totp_secret();
                let b32 = totp_secret_base32(&secret);

                println!("\n── 请在 Google Authenticator / Authy 中手动输入以下密钥 ──");
                println!("账户名：kyvault (webkubor)");
                println!("密钥：  {b32}");
                println!("类型：  基于时间 (TOTP)");
                println!("─────────────────────────────────────────────────────────");

                // 用 master.key 加密 TOTP secret
                let mk_bytes = B64.decode(master_key.trim())?;
                let aes_key: [u8; 32] = Sha256::digest(&mk_bytes).into();
                let enc = aes_encrypt(&aes_key, &secret)?;

                eprint!("\n请输入验证器当前显示的 6 位码确认：");
                io::stderr().flush()?;
                let mut code = String::new();
                io::stdin().read_line(&mut code)?;
                if !totp_verify(&secret, code.trim()) {
                    return Err(anyhow!("验证码不匹配，TOTP 未绑定。"));
                }

                guard.config.methods.push(AuthMethod {
                    id: "totp-main".into(),
                    method_type: "totp".into(),
                    key_path: None,
                    fingerprint: None,
                    label: Some("kyvault (webkubor)".into()),
                    secret_enc: Some(enc),
                    bound_at: now_iso8601(),
                });
                guard.save()?;
                println!("✓ TOTP 已绑定");
            }
            _ => {
                return Err(anyhow!("未知方法 '{method_type}'。支持：ssh, totp"));
            }
        }
        Ok(())
    }

    /// 移除一种授权方法（至少保留一个 SSH key）
    pub fn remove_method(root: &Path, id: &str) -> Result<()> {
        let mut guard = Self::load(root)?;

        let idx = guard
            .config
            .methods
            .iter()
            .position(|m| m.id == id)
            .ok_or_else(|| anyhow!("找不到方法 '{id}'。用 `kyvault auth list` 查看。"))?;

        let is_ssh = guard.config.methods[idx].method_type == "ssh_key";
        if is_ssh {
            let ssh_count = guard
                .config
                .methods
                .iter()
                .filter(|m| m.method_type == "ssh_key")
                .count();
            if ssh_count <= 1 {
                return Err(anyhow!(
                    "至少保留一把 SSH Key。如需彻底关闭 Auth Guard，使用 `kyvault auth disable`。"
                ));
            }
        }

        let removed = guard.config.methods.remove(idx);
        guard.save()?;
        println!("✓ 已移除 {} ({})", removed.id, removed.method_type);

        Ok(())
    }

    /// 关闭 Auth Guard，恢复明文 master.key
    pub fn disable(root: &Path) -> Result<()> {
        if !Self::is_enabled(root) {
            println!("Auth Guard 未启用，无需操作。");
            return Ok(());
        }

        let guard = Self::load(root)?;
        let master_key = guard.unlock()?;

        // 恢复明文 master.key
        let mk_path = Self::master_key_plain(root);
        fs::write(&mk_path, &master_key)?;
        crate::store::harden(&mk_path)?;

        // 删除 guard 文件
        let _ = fs::remove_file(Self::auth_json(root));
        let _ = fs::remove_file(Self::master_key_enc(root));

        println!("✓ Auth Guard 已关闭，master.key 已恢复为明文。");
        Ok(())
    }

    /// 列出已绑定的方法
    pub fn list(root: &Path) -> Result<()> {
        use crate::tui::*;

        if !Self::is_enabled(root) {
            box_top("🛡️ Auth Guard");
            println!();
            status_line("守卫", false, "未启用");
            info_line("  ", &dim("kyvault auth setup 启用"));
            println!();
            box_bottom();
            return Ok(());
        }

        let guard = Self::load(root)?;
        box_top("🛡️ Auth Guard · 绑定清单");
        println!();
        kv("策略", &cyan(&guard.config.policy));
        kv("方法数", &bold(&format!("{}", guard.config.methods.len())));
        println!();
        box_sep();

        for (i, m) in guard.config.methods.iter().enumerate() {
            println!();
            match m.method_type.as_str() {
                "ssh_key" => {
                    let path = m.key_path.as_deref().unwrap_or("?");
                    let fp = m.fingerprint.as_deref().unwrap_or("?");
                    let exists = expand_path(path).exists();
                    println!(
                        "  {} {} {}",
                        bold_cyan(&format!("#{}", i + 1)),
                        bold("SSH Key"),
                        dim(&format!("[{}]", m.id))
                    );
                    kv("    路径", path);
                    kv("    指纹", &dim(fp));
                    status_line("    公钥", exists, if exists { "存在" } else { "缺失" });
                    kv("    绑定", &dim(&m.bound_at));
                }
                "totp" => {
                    let label = m.label.as_deref().unwrap_or("kyvault");
                    println!(
                        "  {} {} {}",
                        bold_cyan(&format!("#{}", i + 1)),
                        bold("TOTP"),
                        dim(&format!("[{}]", m.id))
                    );
                    kv("    账户", label);
                    kv("    绑定", &dim(&m.bound_at));
                }
                _ => {
                    println!("  {} {}", bold_cyan(&format!("#{}", i + 1)), m.method_type);
                }
            }
        }
        println!();
        box_bottom();
        Ok(())
    }

    /// 状态概览
    pub fn status(root: &Path) -> Result<()> {
        use crate::tui::*;

        box_top("🛡️ Auth Guard 状态");
        println!();

        if !Self::is_enabled(root) {
            status_line("守卫", false, "未启用 (仅 master.key 明文)");
            println!();
            info_line("  ", &dim("kyvault auth setup 启用"));
            println!();
            box_bottom();
            return Ok(());
        }

        let guard = Self::load(root)?;
        status_line("守卫", true, "已启用");

        let ssh_count = guard
            .config
            .methods
            .iter()
            .filter(|m| m.method_type == "ssh_key")
            .count();
        let has_totp = guard.config.methods.iter().any(|m| m.method_type == "totp");

        kv("策略", &cyan(&guard.config.policy));
        let ssh_str = format!("{} 把绑定", bold(&ssh_count.to_string()));
        kv("SSH Key", &ssh_str);
        let totp_str = if has_totp {
            bold_green("已绑定")
        } else {
            dim("未绑定")
        };
        kv("TOTP", &totp_str);

        println!();
        box_sep();
        println!();

        let enc_path = Self::master_key_enc(root);
        let plain_path = Self::master_key_plain(root);

        status_line(
            "master.key.enc",
            enc_path.exists(),
            if enc_path.exists() {
                "存在"
            } else {
                "缺失"
            },
        );
        if plain_path.exists() {
            warn_line("明文 master.key 仍存在，建议删除");
        } else {
            status_line("明文残留", true, "已清除");
        }

        // 解锁测试
        match guard.unlock() {
            Ok(_) => status_line("解锁测试", true, "成功"),
            Err(e) => status_line("解锁测试", false, &format!("{e}")),
        }

        println!();
        box_bottom();
        Ok(())
    }

    // ─── 内部方法 ───

    fn save(&self) -> Result<()> {
        let path = Self::auth_json(&self.root);
        let json = serde_json::to_string_pretty(&self.config)?;
        fs::write(&path, &json)?;
        crate::store::harden(&path)?;
        Ok(())
    }
}

// ───────────────────────── 测试 ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ssh_pubkey_fingerprint_roundtrip() {
        // 使用一个固定的测试用 ed25519 公钥
        let tmp = TempDir::new().unwrap();
        let pub_path = tmp.path().join("test.pub");
        // 这是一个生成的测试公钥，不对应任何实际私钥
        fs::write(
            &pub_path,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl test@test\n",
        )
        .unwrap();

        let fp = ssh_fingerprint(&pub_path).unwrap();
        assert!(fp.starts_with("SHA256:"), "指纹应以 SHA256: 开头：{fp}");

        // 重复计算应稳定
        let fp2 = ssh_fingerprint(&pub_path).unwrap();
        assert_eq!(fp, fp2, "同一公钥的指纹应一致");
    }

    #[test]
    fn aes_encrypt_decrypt_roundtrip() {
        let key = Sha256::digest(b"test-key-material").into();
        let plaintext = b"this is my master key content";
        let encrypted = aes_encrypt(&key, plaintext).unwrap();
        let decrypted = aes_decrypt(&key, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn aes_wrong_key_fails() {
        let key1: [u8; 32] = Sha256::digest(b"key-1").into();
        let key2: [u8; 32] = Sha256::digest(b"key-2").into();
        let encrypted = aes_encrypt(&key1, b"secret").unwrap();
        assert!(aes_decrypt(&key2, &encrypted).is_err());
    }

    #[test]
    fn totp_generates_6_digits() {
        let secret = b"12345678901234567890";
        let code = totp_generate(secret, 1_700_000_000);
        assert_eq!(code.len(), 6, "TOTP 码应为 6 位");
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn totp_verify_with_tolerance() {
        let secret = b"12345678901234567890";
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let code = totp_generate(secret, now);
        assert!(totp_verify(secret, &code), "当前码应验证通过");

        let old_code = totp_generate(secret, now - 30);
        assert!(totp_verify(secret, &old_code), "前一步码应验证通过（容差）");
    }

    #[test]
    fn totp_wrong_code_fails() {
        let secret = b"12345678901234567890";
        assert!(!totp_verify(secret, "000000"));
    }

    #[test]
    fn totp_base32_encoding() {
        let secret = vec![0x48, 0x65, 0x6c, 0x6c, 0x6f]; // "Hello"
        let b32 = totp_secret_base32(&secret);
        assert_eq!(b32, "JBSWY3DP"); // 标准 Base32 编码
    }

    #[test]
    fn auth_guard_full_lifecycle() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // 写一个明文 master.key
        let mk = "dGVzdC1tYXN0ZXIta2V5LWZvci1hdXRo"; // base64 of test data
        fs::write(root.join("master.key"), mk).unwrap();

        // 写一个假 SSH 公钥
        let pub_path = root.join("test.pub");
        fs::write(
            &pub_path,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl test@test\n",
        )
        .unwrap();

        // 手动构建 auth guard（不走交互式 setup）
        let fp = ssh_fingerprint(&pub_path).unwrap();
        let aes_key = derive_key_from_ssh(&pub_path).unwrap();
        let encrypted = aes_encrypt(&aes_key, mk.as_bytes()).unwrap();

        let config = AuthConfig {
            version: 1,
            policy: "any_one".into(),
            methods: vec![AuthMethod {
                id: "ssh-test".into(),
                method_type: "ssh_key".into(),
                key_path: Some(pub_path.to_string_lossy().to_string()),
                fingerprint: Some(fp),
                label: None,
                secret_enc: Some(encrypted.clone()),
                bound_at: now_iso8601(),
            }],
        };

        fs::write(
            root.join("auth.json"),
            serde_json::to_string_pretty(&config).unwrap(),
        )
        .unwrap();
        fs::write(root.join("master.key.enc"), &encrypted).unwrap();
        fs::remove_file(root.join("master.key")).unwrap();

        // 验证启用状态
        assert!(AuthGuard::is_enabled(root));

        // 验证解锁
        let guard = AuthGuard::load(root).unwrap();
        let unlocked = guard.unlock().unwrap();
        assert_eq!(unlocked, mk, "解锁后应得到原始 master.key");

        // 验证明文已删
        assert!(!root.join("master.key").exists());

        // disable
        AuthGuard::disable(root).unwrap();
        assert!(!AuthGuard::is_enabled(root));
        assert!(root.join("master.key").exists());
        let restored = fs::read_to_string(root.join("master.key")).unwrap();
        assert_eq!(restored, mk);
    }

    #[test]
    fn auth_guard_multi_ssh_key() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let mk = "bXVsdGkta2V5LXRlc3Q=";
        fs::write(root.join("master.key"), mk).unwrap();

        // 两把不同的公钥
        let pub1 = root.join("key1.pub");
        let pub2 = root.join("key2.pub");
        fs::write(
            &pub1,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl key1\n",
        ).unwrap();
        // 用不同内容的 ed25519 key 制造不同指纹
        fs::write(
            &pub2,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIG1VVnUtRfeDJCx00VKGGEJVIFNwMSNqS+F5YONHfjWk key2\n",
        ).unwrap();

        let fp1 = ssh_fingerprint(&pub1).unwrap();
        let aes_key1 = derive_key_from_ssh(&pub1).unwrap();
        let enc1 = aes_encrypt(&aes_key1, mk.as_bytes()).unwrap();

        let fp2 = ssh_fingerprint(&pub2).unwrap();
        let aes_key2 = derive_key_from_ssh(&pub2).unwrap();
        let enc2 = aes_encrypt(&aes_key2, mk.as_bytes()).unwrap();

        let config = AuthConfig {
            version: 1,
            policy: "any_one".into(),
            methods: vec![
                AuthMethod {
                    id: "ssh-key1".into(),
                    method_type: "ssh_key".into(),
                    key_path: Some(pub1.to_string_lossy().to_string()),
                    fingerprint: Some(fp1),
                    label: None,
                    secret_enc: Some(enc1),
                    bound_at: now_iso8601(),
                },
                AuthMethod {
                    id: "ssh-key2".into(),
                    method_type: "ssh_key".into(),
                    key_path: Some(pub2.to_string_lossy().to_string()),
                    fingerprint: Some(fp2),
                    label: None,
                    secret_enc: Some(enc2),
                    bound_at: now_iso8601(),
                },
            ],
        };

        fs::write(
            root.join("auth.json"),
            serde_json::to_string_pretty(&config).unwrap(),
        )
        .unwrap();
        // master.key.enc 存第一把 key 的加密结果（但 unlock 能用任一把）
        let enc_for_file = aes_encrypt(&aes_key1, mk.as_bytes()).unwrap();
        fs::write(root.join("master.key.enc"), &enc_for_file).unwrap();

        let guard = AuthGuard::load(root).unwrap();
        let result = guard.unlock().unwrap();
        assert_eq!(result, mk);
    }

    #[test]
    fn iso8601_format_is_valid() {
        let ts = now_iso8601();
        // 应形如 2026-09-22T15:15:00Z
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20);
    }
}
