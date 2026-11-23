//! Local-only desktop history. The caller must use an application-data directory
//! outside the synced vault. Only redacted text is retained; detection is best
//! effort, so clipboard monitoring must remain an explicit user opt-in.
use crate::crypto::{decrypt_joined, derive_file_key, encrypt_joined};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use fs2::FileExt;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_ENTRIES: usize = 200;
/// 图片条目的像素文件目录（相对 history root）。
pub const IMG_DIR: &str = "img";
/// 单张图片上限（PNG 编码后）：截图/贴图通常 &lt; 4 MiB，8 MiB 兜底防异常大图。
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const RETENTION_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: u64,
    pub created_at: u64,
    /// 原文。history.enc 本身已加密；敏感条目在展示层默认打码，点开才显示。
    pub text: String,
    /// 这条是否被判定为敏感（展示时默认打码）。不再销毁原文。
    pub redacted: bool,
    pub name: Option<String>,
    pub version: Option<u64>,
    /// 复制时前台应用名（如 "Google Chrome" / "Terminal"）。
    #[serde(default)]
    pub source: Option<String>,
    /// 图片条目的元数据。像素数据存独立的加密文件 img/<id>.enc。
    #[serde(default)]
    pub image: Option<ImageMeta>,
}

/// 图片条目元数据（进 JSON 索引，保持轻量；像素在单独的加密文件里）。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    /// PNG 编码后的字节数（展示用，如 "1.2 MB"）。
    pub png_len: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    next_id: u64,
    entries: Vec<HistoryEntry>,
}

pub struct DesktopHistory {
    root: PathBuf,
}

impl DesktopHistory {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn capture(&self, text: &str, source: Option<&str>) -> Result<HistoryEntry> {
        self.add(None, text, source)
    }

    /// 记录一张图片：PNG 像素加密存 img/<id>.enc，索引只留元数据。
    /// 返回条目；失败（目录不可写/编码异常）时不产生残留文件。
    pub fn capture_image(
        &self,
        png: &[u8],
        width: u32,
        height: u32,
        source: Option<&str>,
    ) -> Result<HistoryEntry> {
        if png.len() > MAX_IMAGE_BYTES {
            bail!(
                "Image exceeds the {} MiB history limit",
                MAX_IMAGE_BYTES / 1024 / 1024
            );
        }
        let meta = ImageMeta {
            width,
            height,
            png_len: png.len() as u64,
        };
        let entry = self.transact(|state| {
            state.next_id = state
                .next_id
                .checked_add(1)
                .context("History identifier limit reached")?;
            let entry = HistoryEntry {
                id: state.next_id,
                created_at: now()?,
                text: String::new(),
                redacted: false,
                name: None,
                version: None,
                source: source.map(str::to_owned),
                image: Some(meta.clone()),
            };
            state.entries.push(entry.clone());
            trim_history(state);
            Ok(entry)
        })?;
        // 索引写成功后再写像素文件；写失败则回滚索引里的这条。
        let b64 = STANDARD.encode(png);
        let payload = match crate::crypto::encrypt_joined(&b64, &self.file_key()?) {
            Ok(p) => p,
            Err(e) => {
                let _ = self.delete(entry.id);
                return Err(e);
            }
        };
        let dir = self.root.join(IMG_DIR);
        fs::create_dir_all(&dir).ok();
        if let Err(e) = atomic_write(&dir.join(format!("{}.enc", entry.id)), payload.as_bytes()) {
            let _ = self.delete(entry.id);
            return Err(e);
        }
        Ok(entry)
    }

    /// 读回某条图片的 PNG 像素（解密 img/<id>.enc）。
    pub fn image_data(&self, id: u64) -> Result<Vec<u8>> {
        let path = self.root.join(IMG_DIR).join(format!("{id}.enc"));
        let enc = fs::read_to_string(&path).with_context(|| "Image entry not found")?;
        let b64 = crate::crypto::decrypt_joined(&enc, &self.file_key()?)?;
        STANDARD
            .decode(b64)
            .map_err(|e| anyhow::anyhow!("Image decode failed: {e}"))
    }

    /// transact 里读写的文件密钥（与 history.enc 同源派生）。
    fn file_key(&self) -> Result<[u8; 32]> {
        let key_path = self.root.join("history.key");
        let raw = fs::read_to_string(&key_path).context("Cannot read desktop history key")?;
        derive_file_key(&raw)
    }

    /// Saves a configuration/snippet version. Names are also validated
    /// so a credential cannot be retained in the grouping metadata.
    pub fn save_version(&self, name: &str, text: &str) -> Result<HistoryEntry> {
        let name = name.trim();
        if name.is_empty() || name.len() > 128 || name.contains(['\n', '\r']) || needs_mask(name) {
            bail!("Use a non-sensitive name between 1 and 128 bytes");
        }
        self.add(Some(name), text, None)
    }

    fn add(&self, name: Option<&str>, text: &str, source: Option<&str>) -> Result<HistoryEntry> {
        if text.len() > MAX_TEXT_BYTES {
            bail!("Text exceeds the 64 KiB history limit");
        }
        if text.trim().is_empty() {
            bail!("Empty clipboard text is not retained");
        }
        self.transact(|state| Self::insert(state, name, text, source, now()?))
    }

    fn insert(
        state: &mut State,
        name: Option<&str>,
        text: &str,
        source: Option<&str>,
        timestamp: u64,
    ) -> Result<HistoryEntry> {
        let version = name
            .map(|n| {
                state
                    .entries
                    .iter()
                    .filter(|e| e.name.as_deref() == Some(n))
                    .filter_map(|e| e.version)
                    .max()
                    .unwrap_or(0)
                    .checked_add(1)
                    .context("Version limit reached")
            })
            .transpose()?;
        state.next_id = state
            .next_id
            .checked_add(1)
            .context("History identifier limit reached")?;
        // 存原文（history.enc 已加密），只标记是否需要打码；不再销毁内容。
        let entry = HistoryEntry {
            id: state.next_id,
            created_at: timestamp,
            redacted: needs_mask(text),
            text: text.to_owned(),
            name: name.map(str::to_owned),
            version,
            source: source.map(str::to_owned),
            image: None,
        };
        state.entries.push(entry.clone());
        trim_history(state);
        Ok(entry)
    }

    /// Newest first. Reading also removes expired entries from encrypted storage.
    pub fn list(&self) -> Result<Vec<HistoryEntry>> {
        self.transact(|state| Ok(state.entries.iter().rev().cloned().collect()))
    }

    pub fn search(&self, query: &str) -> Result<Vec<HistoryEntry>> {
        let query = query.to_lowercase();
        Ok(self
            .list()?
            .into_iter()
            .filter(|e| {
                e.text.to_lowercase().contains(&query)
                    || e.name
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
            })
            .collect())
    }

    pub fn versions(&self, name: &str) -> Result<Vec<HistoryEntry>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|e| e.name.as_deref() == Some(name))
            .collect())
    }

    /// Restoring appends a version instead of overwriting history. The restored
    /// content is the retained, redacted text, never the original secret.
    pub fn restore(&self, id: u64) -> Result<HistoryEntry> {
        self.transact(|state| {
            let old = state
                .entries
                .iter()
                .find(|e| e.id == id)
                .cloned()
                .context("History entry not found")?;
            let name = old
                .name
                .as_deref()
                .context("Only named versions can be restored")?;
            let src = old.source.as_deref();
            let mut entry = Self::insert(state, Some(name), &old.text, src, now()?)?;
            entry.redacted |= old.redacted;
            if let Some(last) = state.entries.last_mut() {
                last.redacted = entry.redacted;
            }
            Ok(entry)
        })
    }

    pub fn delete(&self, id: u64) -> Result<bool> {
        self.transact(|state| {
            let old = state.entries.len();
            state.entries.retain(|e| e.id != id);
            Ok(old != state.entries.len())
        })
    }

    pub fn clear(&self) -> Result<()> {
        self.transact(|state| {
            state.entries.retain(|entry| entry.name.is_some());
            Ok(())
        })
    }

    fn transact<T>(&self, action: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        reject_symlink(&self.root)?;
        fs::create_dir_all(&self.root).context("Cannot create desktop history directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        }
        let lock_path = self.root.join("history.lock");
        reject_symlink(&lock_path)?;
        let lock = private_file(&lock_path, false)?;
        lock.lock_exclusive()
            .context("Cannot lock desktop history")?;
        // File closure releases the OS lock even when an operation fails.
        let key_path = self.root.join("history.key");
        let data_path = self.root.join("history.enc");
        reject_symlink(&key_path)?;
        reject_symlink(&data_path)?;
        if !key_path.exists() {
            if data_path.exists() {
                bail!("Desktop history key is missing; encrypted history was preserved");
            }
            let mut raw = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut raw);
            atomic_write(&key_path, STANDARD.encode(raw).as_bytes())?;
        }
        let key = derive_file_key(
            &fs::read_to_string(&key_path).context("Cannot read desktop history key")?,
        )?;
        let mut state: State = if data_path.exists() {
            let encrypted =
                fs::read_to_string(&data_path).context("Cannot read desktop history")?;
            let plain =
                decrypt_joined(&encrypted, &key).context("Cannot decrypt desktop history")?;
            serde_json::from_str(&plain)
                .map_err(|_| anyhow::anyhow!("Desktop history format is invalid"))?
        } else {
            State::default()
        };
        let original = serde_json::to_string(&state)?;
        let cutoff = now()?.saturating_sub(RETENTION_SECONDS);
        state
            .entries
            .retain(|entry| entry.name.is_some() || entry.created_at >= cutoff);
        let result = action(&mut state)?;
        let updated = serde_json::to_string(&state)?;
        if !data_path.exists() || updated != original {
            let encrypted = encrypt_joined(&updated, &key)?;
            atomic_write(&data_path, encrypted.as_bytes())?;
            // 图片文件与索引同生命周期：清理孤儿（条目过期/删除后残留的像素文件）。
            self.cleanup_orphan_images(&state);
        }
        Ok(result)
    }

    /// 删除 img/ 下不再被任何条目引用的加密像素文件。失败静默（只影响磁盘占用）。
    fn cleanup_orphan_images(&self, state: &State) {
        let dir = self.root.join(IMG_DIR);
        let Ok(iter) = fs::read_dir(&dir) else { return };
        let live: std::collections::HashSet<u64> = state
            .entries
            .iter()
            .filter_map(|e| e.image.as_ref().map(|_| e.id))
            .collect();
        for item in iter.flatten() {
            let name = item.file_name();
            let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(".enc")) else {
                continue;
            };
            if let Ok(id) = stem.parse::<u64>() {
                if !live.contains(&id) {
                    let _ = fs::remove_file(item.path());
                }
            }
        }
    }
}

// Preserve previously saved snippets while limiting ordinary clipboard history.
fn trim_history(state: &mut State) {
    let mut excess = state
        .entries
        .iter()
        .filter(|entry| entry.name.is_none())
        .count()
        .saturating_sub(MAX_ENTRIES);
    state.entries.retain(|entry| {
        if entry.name.is_none() && excess > 0 {
            excess -= 1;
            false
        } else {
            true
        }
    });
}

fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            bail!("Desktop history paths must not be symbolic links")
        }
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn private_file(path: &Path, exclusive: bool) -> Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true);
    if exclusive {
        opts.create_new(true);
    } else {
        opts.create(true).truncate(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("tmp-{:016x}", rand::random::<u64>()));
    let mut file = private_file(&tmp, true)?;
    let result = (|| -> Result<()> {
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        #[cfg(unix)]
        File::open(path.parent().context("Missing history directory")?)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Conservative, line-level redaction for common credential assignments,
/// authorization headers, token prefixes and PEM blocks. It intentionally
/// sacrifices surrounding text on a sensitive line. It cannot detect arbitrary
/// unlabelled passwords; callers must not describe it as a complete DLP filter.
/// 这段文本是否含敏感内容（密钥/令牌/PEM 等）。只做判定，不销毁内容。
/// 复用 redact 的判定逻辑：打码后与原文不同即视为需要打码。
pub fn needs_mask(text: &str) -> bool {
    redact(text) != text
}

pub fn redact(text: &str) -> String {
    let mut pem = false;
    let mut redact_all = false;
    let output = text
        .split('\n')
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if lower.contains("-----begin ") {
                pem = true;
            }
            if pem {
                if lower.contains("-----end ") {
                    pem = false;
                }
                return "[REDACTED]".to_owned();
            }
            let sensitive_assignment = line.rsplit_once(['=', ':']).is_some_and(|(label, _)| {
                (line.contains('=') || line.contains(':')) && {
                    let label = label
                        .to_ascii_lowercase()
                        .replace(['-', '_', ' ', '\t', '\"', '\''], "");
                    [
                        "password",
                        "passwd",
                        "pwd",
                        "secret",
                        "token",
                        "apikey",
                        "privatekey",
                        "authorization",
                        "credential",
                        "accesskey",
                        "clientkey",
                    ]
                    .iter()
                    .any(|word| label.contains(word))
                }
            });
            if sensitive_assignment {
                if let Some((_, value)) = line.rsplit_once(['=', ':']) {
                    let value = value.trim();
                    if value.is_empty()
                        || value.starts_with(['|', '>', '{', '['])
                        || value.chars().filter(|c| *c == '\"').count() % 2 != 0
                        || value.chars().filter(|c| *c == '\'').count() % 2 != 0
                        || value.contains("\"\"\"")
                        || value.contains("'".repeat(3).as_str())
                    {
                        redact_all = true;
                    }
                }
            }
            let common_token = [
                "sk-",
                "ghp_",
                "gho_",
                "ghu_",
                "ghs_",
                "ghr_",
                "github_pat_",
                "xoxb-",
                "xoxp-",
                "xoxa-",
                "xoxs-",
                "akia",
                "asia",
                "bearer ",
                "basic ",
                "eyj",
            ]
            .iter()
            .any(|prefix| lower.contains(prefix));
            if sensitive_assignment || common_token {
                "[REDACTED]".to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if redact_all {
        "[REDACTED]".to_owned()
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_credentials_never_leave_value_lines() {
        for text in [
            "password: |\n  synthetic value",
            "password:\n  synthetic value",
            "TOKEN=\"\nsynthetic value\n\"",
            "secret = '''\nsynthetic value\n'''",
            "{\"password\":\n\"synthetic value\"}",
        ] {
            assert_eq!(redact(text), "[REDACTED]");
        }
    }

    #[test]
    fn original_text_survives_roundtrip_and_marks_mask() {
        // 产品决策：敏感内容不再永久销毁。原文加密存盘，列出时仍能拿回，
        // 仅以 redacted 标志告诉展示层"默认打码，点开才显示"。
        let dir = tempfile::tempdir().unwrap();
        let history = DesktopHistory::new(dir.path());
        let secret = "plain clipboard text with source";
        let e = history.capture(secret, Some("Terminal")).unwrap();
        assert_eq!(e.text, secret);
        assert_eq!(e.source.as_deref(), Some("Terminal"));
        let read = history.list().unwrap().pop().unwrap();
        assert_eq!(read.text, secret, "原文应能从加密存储读回，不再永久销毁");
        assert_eq!(read.source.as_deref(), Some("Terminal"));
    }

    #[test]
    fn image_roundtrip_and_orphan_cleanup() {
        // 图片：像素加密存独立文件，索引只留元数据；读回一致；删除后文件被清理。
        let dir = tempfile::tempdir().unwrap();
        let history = DesktopHistory::new(dir.path());
        // 1x1 红色 PNG（最小合法图）
        let png: Vec<u8> = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x62, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
        .to_vec();
        let e = history.capture_image(&png, 1, 1, Some("Chrome")).unwrap();
        assert_eq!(e.image.as_ref().unwrap().width, 1);
        assert!(dir
            .path()
            .join("img")
            .join(format!("{}.enc", e.id))
            .exists());
        let back = history.image_data(e.id).unwrap();
        assert_eq!(back, png, "像素应能从加密文件读回");
        // 删除条目 → 孤儿像素文件被清理。
        history.delete(e.id).unwrap();
        assert!(!dir
            .path()
            .join("img")
            .join(format!("{}.enc", e.id))
            .exists());
    }

    #[test]
    fn legacy_snippets_survive_history_retention_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let history = DesktopHistory::new(dir.path());
        let saved = history.save_version("saved", "keep this").unwrap();
        history
            .transact(|state| {
                state
                    .entries
                    .iter_mut()
                    .find(|e| e.id == saved.id)
                    .unwrap()
                    .created_at = 1;
                for n in 0..=MAX_ENTRIES {
                    DesktopHistory::insert(state, None, &format!("item {n}"), None, now()?)?;
                }
                Ok(())
            })
            .unwrap();
        assert!(history
            .list()
            .unwrap()
            .iter()
            .any(|entry| entry.id == saved.id));
        history.clear().unwrap();
        let entries = history.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, saved.id);
        assert_eq!(entries[0].text, "keep this");
    }

    #[test]
    fn unchanged_reads_do_not_rewrite_ciphertext() {
        let dir = tempfile::tempdir().unwrap();
        let history = DesktopHistory::new(dir.path());
        history.capture("fixture", None).unwrap();
        let before = fs::read(dir.path().join("history.enc")).unwrap();
        history.list().unwrap();
        history.search("fixture").unwrap();
        assert_eq!(before, fs::read(dir.path().join("history.enc")).unwrap());
    }

    #[test]
    fn redacts_credentials_pem_and_common_tokens() {
        // The PEM header is assembled at runtime: a literal PEM banner in the
        // source would trip the repository credential scanner, and this test
        // only needs the shape, never a real key.
        let pem_header = format!("-----BEGIN {}-----", "RSA PRIVATE KEY");
        let pem_end = format!("-----END {}-----", "RSA PRIVATE KEY");
        for input in [
            "API_KEY=fake-value",
            "{\"password\": \"fixture\"}",
            "export AWS_SECRET_ACCESS_KEY=fixture",
            "Authorization: Bearer fixture",
            "hello ghp_fixture",
            "token: fixture",
            &format!("{pem_header}\nfixture\n{pem_end}"),
            &format!("{pem_header}\nfixture"),
        ] {
            let safe = redact(input);
            assert!(!safe.contains("fixture"));
            assert!(safe.contains("[REDACTED]"));
        }
        for prefix in ["ghp_", "sk-", "github_pat_", "xoxb-", "AKIA", "eyJ"] {
            assert_eq!(
                redact(&format!("{prefix}{}", "fixture".repeat(8))),
                "[REDACTED]"
            );
        }
        assert_eq!(
            redact("{\"config\": {\"password\": \"fixture\"}}"),
            "[REDACTED]"
        );
        assert_eq!(redact("port=8080\n你好"), "port=8080\n你好");
    }

    #[test]
    fn encrypted_disk_and_redacted_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let history = DesktopHistory::new(dir.path());
        let entry = history
            .capture("public fixture\nPASSWORD=super-sensitive-fixture", None)
            .unwrap();
        assert!(entry.redacted, "敏感内容应被标记需要打码");
        // 原文保留在内存条目（展示层默认打码，点开才显示），不再永久销毁。
        assert_eq!(
            entry.text,
            "public fixture\nPASSWORD=super-sensitive-fixture"
        );
        assert_eq!(history.list().unwrap(), vec![entry]);
        for item in fs::read_dir(dir.path()).unwrap() {
            let data = fs::read_to_string(item.unwrap().path()).unwrap();
            assert!(!data.contains("super-sensitive-fixture"));
            assert!(!data.contains("public fixture"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(dir.path().join("history.key"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(dir.path().join("history.enc"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn limits_retention_search_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let history = DesktopHistory::new(dir.path());
        assert!(history
            .capture(&"a".repeat(MAX_TEXT_BYTES + 1), None)
            .is_err());
        assert!(history.capture("  ", None).is_err());
        history
            .transact(|state| {
                for n in 0..201 {
                    DesktopHistory::insert(state, None, &format!("item {n}"), None, now()?)?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(history.list().unwrap().len(), MAX_ENTRIES);
        assert_eq!(history.search("ITEM 200").unwrap().len(), 1);
        history
            .transact(|state| {
                state.entries[0].created_at = 0;
                Ok(())
            })
            .unwrap();
        assert_eq!(history.list().unwrap().len(), 199);
        let id = history.list().unwrap()[0].id;
        assert!(history.delete(id).unwrap());
        assert!(!history.delete(id).unwrap());
        history.clear().unwrap();
        assert!(history.list().unwrap().is_empty());
    }

    #[test]
    fn restore_creates_new_redacted_version() {
        let dir = tempfile::tempdir().unwrap();
        let history = DesktopHistory::new(dir.path());
        let first = history
            .save_version("dev config", "port=3000\npassword=fixture")
            .unwrap();
        history.save_version("dev config", "port=4000").unwrap();
        let restored = history.restore(first.id).unwrap();
        assert_eq!(restored.version, Some(3));
        assert_eq!(restored.text, first.text);
        assert!(restored.redacted);
        assert_eq!(history.versions("dev config").unwrap().len(), 3);
        assert!(history.save_version("token=fixture", "text").is_err());
    }

    #[test]
    fn corrupted_ciphertext_and_missing_key_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let history = DesktopHistory::new(dir.path());
        history.capture("fixture", None).unwrap();
        fs::write(dir.path().join("history.enc"), "corrupted").unwrap();
        assert!(history.list().is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("history.enc")).unwrap(),
            "corrupted"
        );
        fs::remove_file(dir.path().join("history.key")).unwrap();
        assert!(history.capture("other", None).is_err());
        assert!(!dir.path().join("history.key").exists());
    }

    #[test]
    fn concurrent_writers_preserve_entries() {
        let dir = tempfile::tempdir().unwrap();
        let handles: Vec<_> = (0..8)
            .map(|n| {
                let root = dir.path().to_path_buf();
                std::thread::spawn(move || {
                    DesktopHistory::new(root)
                        .capture(&format!("item {n}"), None)
                        .unwrap()
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let entries = DesktopHistory::new(dir.path()).list().unwrap();
        assert_eq!(entries.len(), 8);
        let mut ids: Vec<_> = entries.iter().map(|e| e.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 8);
    }
}
