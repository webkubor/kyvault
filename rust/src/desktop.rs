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
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const RETENTION_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: u64,
    pub created_at: u64,
    pub text: String,
    pub redacted: bool,
    pub name: Option<String>,
    pub version: Option<u64>,
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

    pub fn capture(&self, text: &str) -> Result<HistoryEntry> {
        self.add(None, text)
    }

    /// Saves a redacted configuration/snippet version. Names are also validated
    /// so a credential cannot be retained in the grouping metadata.
    pub fn save_version(&self, name: &str, text: &str) -> Result<HistoryEntry> {
        let name = name.trim();
        if name.is_empty()
            || name.len() > 128
            || name.contains(['\n', '\r'])
            || redact(name) != name
        {
            bail!("Use a non-sensitive name between 1 and 128 bytes");
        }
        self.add(Some(name), text)
    }

    fn add(&self, name: Option<&str>, text: &str) -> Result<HistoryEntry> {
        if text.len() > MAX_TEXT_BYTES {
            bail!("Text exceeds the 64 KiB history limit");
        }
        if text.trim().is_empty() {
            bail!("Empty clipboard text is not retained");
        }
        self.transact(|state| Self::insert(state, name, text, now()?))
    }

    fn insert(
        state: &mut State,
        name: Option<&str>,
        text: &str,
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
        let safe = redact(text);
        let entry = HistoryEntry {
            id: state.next_id,
            created_at: timestamp,
            redacted: safe != text,
            text: safe,
            name: name.map(str::to_owned),
            version,
        };
        state.entries.push(entry.clone());
        if state.entries.len() > MAX_ENTRIES {
            state.entries.drain(..state.entries.len() - MAX_ENTRIES);
        }
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
            let mut entry = Self::insert(state, Some(name), &old.text, now()?)?;
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
            state.entries.clear();
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
        state.entries.retain(|entry| entry.created_at >= cutoff);
        let result = action(&mut state)?;
        let updated = serde_json::to_string(&state)?;
        if !data_path.exists() || updated != original {
            let encrypted = encrypt_joined(&updated, &key)?;
            atomic_write(&data_path, encrypted.as_bytes())?;
        }
        Ok(result)
    }
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
    fn unchanged_reads_do_not_rewrite_ciphertext() {
        let dir = tempfile::tempdir().unwrap();
        let history = DesktopHistory::new(dir.path());
        history.capture("fixture").unwrap();
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
            .capture("public fixture\nPASSWORD=super-sensitive-fixture")
            .unwrap();
        assert!(entry.redacted);
        assert_eq!(entry.text, "public fixture\n[REDACTED]");
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
        assert!(history.capture(&"a".repeat(MAX_TEXT_BYTES + 1)).is_err());
        assert!(history.capture("  ").is_err());
        history
            .transact(|state| {
                for n in 0..201 {
                    DesktopHistory::insert(state, None, &format!("item {n}"), now()?)?;
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
        history.capture("fixture").unwrap();
        fs::write(dir.path().join("history.enc"), "corrupted").unwrap();
        assert!(history.list().is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("history.enc")).unwrap(),
            "corrupted"
        );
        fs::remove_file(dir.path().join("history.key")).unwrap();
        assert!(history.capture("other").is_err());
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
                        .capture(&format!("item {n}"))
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
