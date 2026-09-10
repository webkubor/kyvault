//! AES-256-GCM 加解密 —— 必须与 Python 版和 CortexOS Go 端字节级兼容。
//!
//! 这里有**两套 wire format**，别混用，混了就是解不开已有的 141 条密钥：
//!
//! | | 本地 file 后端 | Cloudflare D1 后端 |
//! |---|---|---|
//! | 密钥派生 | `SHA256(base64_decode(master.key))` | `base64_decode(site_config)` **直接用，不派生** |
//! | 密文布局 | `base64(nonce ‖ ciphertext)` 一个字符串 | ciphertext / nonce **分两列**各自 base64 |
//!
//! 两边 AAD 都是空。Python 侧 file 后端传 `b""`、D1 后端传 `None`，
//! 在 AES-GCM 里等价，所以 Rust 这边统一不带 AAD。
//!
//! D1 那张 secret_vault 表是和 CortexOS 的 Go 实现（pkg/infra/secretvault）
//! 共用的，所以这两套格式都不是内部约定，改任何一处都会让另外两个实现读不出来。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const NONCE_LEN: usize = 12;

/// 本地 file 后端的密钥派生：master.key 里存的是 base64(32 随机字节)，
/// 再过一次 SHA-256 才是 AES key。多这一道是 Python 版的既有行为，
/// 不能省——省了就解不开 ~/.keyring/secrets.json。
pub fn derive_file_key(master_b64: &str) -> Result<[u8; 32]> {
    let raw = B64
        .decode(master_b64.trim())
        .context("master key 不是合法 base64")?;
    Ok(Sha256::digest(&raw).into())
}

/// D1 后端的密钥：site_config 里存的就是 base64 的 32 字节 AES key 本身，
/// **不做二次派生**（Go 端 vault.go 也是这样）。
pub fn d1_key(master_b64: &str) -> Result<[u8; 32]> {
    let raw = B64
        .decode(master_b64.trim())
        .context("D1 master key 不是合法 base64")?;
    if raw.len() != 32 {
        return Err(anyhow!("D1 master key 长度是 {} 字节，应为 32", raw.len()));
    }
    let mut k = [0u8; 32];
    k.copy_from_slice(&raw);
    Ok(k)
}

fn random_nonce() -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut n);
    n
}

fn cipher(key: &[u8; 32]) -> Aes256Gcm {
    Aes256Gcm::new(key.into())
}

/// file 后端格式：base64(nonce ‖ ciphertext)
pub fn encrypt_joined(plaintext: &str, key: &[u8; 32]) -> Result<String> {
    let nonce = random_nonce();
    let ct = cipher(key)
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|_| anyhow!("加密失败"))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(B64.encode(out))
}

pub fn decrypt_joined(ciphertext_b64: &str, key: &[u8; 32]) -> Result<String> {
    let data = B64
        .decode(ciphertext_b64.trim())
        .context("密文不是合法 base64")?;
    if data.len() <= NONCE_LEN {
        return Err(anyhow!(
            "密文过短：{} 字节，至少要有 12 字节 nonce",
            data.len()
        ));
    }
    let (nonce, ct) = data.split_at(NONCE_LEN);
    let pt = cipher(key)
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| anyhow!("解密失败：master key 不对，或密文已损坏"))?;
    String::from_utf8(pt).context("解出来的不是合法 UTF-8")
}

/// D1 后端格式：返回 (ciphertext_b64, nonce_b64) 两列分开存
pub fn encrypt_split(plaintext: &str, key: &[u8; 32]) -> Result<(String, String)> {
    let nonce = random_nonce();
    let ct = cipher(key)
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|_| anyhow!("加密失败"))?;
    Ok((B64.encode(ct), B64.encode(nonce)))
}

pub fn decrypt_split(ciphertext_b64: &str, nonce_b64: &str, key: &[u8; 32]) -> Result<String> {
    let ct = B64
        .decode(ciphertext_b64.trim())
        .context("ciphertext 不是合法 base64")?;
    let nonce = B64
        .decode(nonce_b64.trim())
        .context("nonce 不是合法 base64")?;
    if nonce.len() != NONCE_LEN {
        return Err(anyhow!("nonce 长度是 {} 字节，应为 12", nonce.len()));
    }
    let pt = cipher(key)
        .decrypt(Nonce::from_slice(&nonce), ct.as_ref())
        .map_err(|_| anyhow!("解密失败：master key 不对，或密文已损坏"))?;
    String::from_utf8(pt).context("解出来的不是合法 UTF-8")
}

/// 生成新的 master key（base64(32 随机字节)），格式与 Python 版 init 一致。
pub fn new_master_key_b64() -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    B64.encode(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joined_roundtrip() {
        let key = derive_file_key(&new_master_key_b64()).unwrap();
        let ct = encrypt_joined("hello 密钥", &key).unwrap();
        assert_eq!(decrypt_joined(&ct, &key).unwrap(), "hello 密钥");
    }

    #[test]
    fn split_roundtrip() {
        let key = d1_key(&new_master_key_b64()).unwrap();
        let (ct, nonce) = encrypt_split("hello 密钥", &key).unwrap();
        assert_eq!(decrypt_split(&ct, &nonce, &key).unwrap(), "hello 密钥");
    }

    /// 两套派生方式必须不同，否则说明哪一边写错了 —— 写错的后果是
    /// 用 file 的 key 去解 D1 的密文，报「解密失败」但看不出根因。
    #[test]
    fn two_derivations_differ() {
        let m = new_master_key_b64();
        assert_ne!(derive_file_key(&m).unwrap(), d1_key(&m).unwrap());
    }

    #[test]
    fn nonce_is_fresh_每次都不同() {
        let key = derive_file_key(&new_master_key_b64()).unwrap();
        let a = encrypt_joined("x", &key).unwrap();
        let b = encrypt_joined("x", &key).unwrap();
        assert_ne!(
            a, b,
            "同一明文两次加密不该得到相同密文（nonce 复用会毁掉 GCM）"
        );
    }

    #[test]
    fn wrong_key_fails_loudly() {
        let key = derive_file_key(&new_master_key_b64()).unwrap();
        let other = derive_file_key(&new_master_key_b64()).unwrap();
        let ct = encrypt_joined("secret", &key).unwrap();
        assert!(decrypt_joined(&ct, &other).is_err(), "换 key 必须解不开");
    }

    #[test]
    fn d1_key_rejects_wrong_length() {
        assert!(
            d1_key(&B64.encode([0u8; 16])).is_err(),
            "16 字节的 key 应被拒绝"
        );
    }

    #[test]
    fn short_ciphertext_rejected() {
        let key = derive_file_key(&new_master_key_b64()).unwrap();
        assert!(decrypt_joined(&B64.encode([0u8; 8]), &key).is_err());
    }
}
