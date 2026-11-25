//! Plain Markdown inspiration notes, independent of the encrypted vault.
//! Each note is readable without Ky; an HTML comment holds IDs, tags and timestamps.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::now_utc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Idea {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub path: String,
}

impl Idea {
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || self.title.to_lowercase().contains(&q)
            || self.body.to_lowercase().contains(&q)
            || self.tags.iter().any(|t| t.to_lowercase().contains(&q))
            || self
                .source
                .as_ref()
                .is_some_and(|s| s.to_lowercase().contains(&q))
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Metadata {
    id: String,
    title: String,
    tags: Vec<String>,
    created_at: String,
    updated_at: String,
    source: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

struct Document {
    idea: Idea,
    prefix: String,
    extra: serde_json::Map<String, serde_json::Value>,
}

pub struct InspirationStore {
    dir: PathBuf,
}

pub fn default_dir() -> Result<PathBuf> {
    if let Ok(d) = std::env::var("KY_INSPIRATION_DIR") {
        if !d.trim().is_empty() {
            return Ok(PathBuf::from(crate::store::shellexpand_tilde(d.trim())));
        }
    }
    Ok(dirs::document_dir()
        .ok_or_else(|| anyhow!("找不到文稿目录"))?
        .join("ky灵感库"))
}

pub fn parse_tags(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for t in s
        .split([',', '，'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        if !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    out
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}
fn new_id() -> String {
    let mut bytes = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    format!(
        "idea-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}
fn heading(title: &str) -> String {
    title.replace(['\r', '\n'], " ")
}
fn safe_filename(title: &str) -> String {
    let title = heading(title);
    let s: String = title
        .chars()
        .take(48)
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim_matches([' ', '.']);
    if s.is_empty() {
        "灵感".into()
    } else {
        s.into()
    }
}

impl InspirationStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
    pub fn default_location() -> Result<Self> {
        let store = Self::new(default_dir()?);
        // Overrides are isolated, including test and portable libraries.
        if std::env::var("KY_INSPIRATION_DIR").map_or(true, |v| v.trim().is_empty()) {
            let marker = store.dir.join(".legacy-migrated");
            if !marker.exists() {
                if let Some(home) = dirs::home_dir() {
                    let old = home.join(".config/kyvault/inspiration");
                    if old.is_dir() {
                        store.migrate_from(&old)?;
                        let _lock = store.lock()?;
                        Self::atomic_write(&marker, b"Legacy JSON originals preserved.\n")?;
                    }
                }
            }
        }
        Ok(store)
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }
    fn lock(&self) -> Result<File> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("创建灵感库目录失败：{}", self.dir.display()))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.dir.join(".lock"))?;
        lock.lock_exclusive()?;
        Ok(lock)
    }
    fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
        let tmp = path.with_extension(format!("{}.tmp", new_id()));
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&tmp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
    fn read_document(&self, path: &Path) -> Result<Document> {
        let raw = fs::read_to_string(path)?;
        let mut text = raw.as_str();
        let mut meta = Metadata::default();
        if let Some(line) = text.lines().next() {
            if let Some(json) = line
                .strip_prefix("<!-- ky: ")
                .and_then(|s| s.strip_suffix(" -->"))
            {
                meta = serde_json::from_str(json).context("灵感元数据格式错误")?;
                text = text.split_once('\n').map_or("", |(_, rest)| rest);
            }
        }
        let mut frontmatter = "";
        if text.starts_with("---\n") || text.starts_with("---\r\n") {
            let mut end = text.find('\n').unwrap() + 1;
            for line in text[end..].split_inclusive('\n') {
                end += line.len();
                if line.trim_end_matches(['\r', '\n']) == "---" {
                    frontmatter = &text[..end];
                    text = &text[end..];
                    break;
                }
            }
        }
        let mut prefix = frontmatter.to_string();
        let mut body = text.to_string();
        let mut title = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            if let Some(t) = line.trim_end_matches(['\r', '\n']).strip_prefix("# ") {
                title = t.to_string();
                prefix = format!("{frontmatter}{}", &text[..offset]);
                let rest = &text[offset + line.len()..];
                body = rest
                    .strip_prefix("\r\n")
                    .or_else(|| rest.strip_prefix('\n'))
                    .unwrap_or(rest)
                    .to_string();
                if !meta.title.is_empty() && heading(&meta.title) == title {
                    title = meta.title.clone();
                }
                break;
            }
            offset += line.len();
        }
        let id = if meta.id.is_empty() {
            // shortcut: unmanaged notes keep their ID until renamed; first Ky edit embeds it.
            format!(
                "file-{:x}",
                Sha256::digest(
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_bytes()
                )
            )
        } else {
            meta.id
        };
        if !valid_id(&id) {
            return Err(anyhow!("非法灵感 ID：{id}"));
        }
        let modified = fs::metadata(path)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let (y, mo, d, h, mi, s) = crate::model::civil_from_unix(modified as i64);
        let fallback = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z");
        Ok(Document {
            idea: Idea {
                id,
                title,
                body,
                tags: meta.tags,
                created_at: if meta.created_at.is_empty() {
                    fallback.clone()
                } else {
                    meta.created_at
                },
                updated_at: if meta.updated_at.is_empty() {
                    fallback
                } else {
                    meta.updated_at
                },
                source: meta.source,
                path: fs::canonicalize(path)?.to_string_lossy().into_owned(),
            },
            prefix,
            extra: meta.extra,
        })
    }
    fn documents(&self) -> Result<Vec<Document>> {
        let mut docs = Vec::new();
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(docs),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || entry
                    .path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| !s.eq_ignore_ascii_case("md"))
                    .unwrap_or(true)
            {
                continue;
            }
            docs.push(self.read_document(&entry.path())?);
        }
        Ok(docs)
    }
    fn find(&self, id: &str) -> Result<Document> {
        if !valid_id(id) {
            return Err(anyhow!("非法灵感 ID：{id}"));
        }
        let mut matches = self.documents()?.into_iter().filter(|d| d.idea.id == id);
        let doc = matches.next().ok_or_else(|| anyhow!("找不到灵感：{id}"))?;
        if matches.next().is_some() {
            return Err(anyhow!("重复灵感 ID：{id}"));
        }
        Ok(doc)
    }
    fn write(&self, doc: &Document) -> Result<()> {
        let i = &doc.idea;
        let meta = Metadata {
            id: i.id.clone(),
            title: i.title.clone(),
            tags: i.tags.clone(),
            created_at: i.created_at.clone(),
            updated_at: i.updated_at.clone(),
            source: i.source.clone(),
            extra: doc.extra.clone(),
        };
        let json = serde_json::to_string(&meta)?
            .replace('<', "\\u003c")
            .replace('>', "\\u003e");
        Self::atomic_write(
            Path::new(&i.path),
            format!(
                "<!-- ky: {json} -->\n{}# {}\n\n{}",
                doc.prefix,
                heading(&i.title),
                i.body
            )
            .as_bytes(),
        )
    }
    fn insert(&self, mut idea: Idea) -> Result<Idea> {
        let basename = format!(
            "{} {}",
            safe_filename(idea.created_at.get(..10).unwrap_or("灵感")),
            safe_filename(&idea.title)
        );
        let mut path = self.dir.join(format!("{basename}.md"));
        let mut suffix = 2;
        while path.exists() {
            path = self.dir.join(format!("{basename} ({suffix}).md"));
            suffix += 1;
        }
        idea.path = fs::canonicalize(&self.dir)?
            .join(path.file_name().unwrap())
            .to_string_lossy()
            .into_owned();
        let doc = Document {
            idea,
            prefix: String::new(),
            extra: Default::default(),
        };
        self.write(&doc)?;
        Ok(doc.idea)
    }
    pub fn list(&self) -> Result<Vec<Idea>> {
        if !self.dir.exists() {
            return Ok(vec![]);
        }
        let _lock = self.lock()?;
        let mut ideas: Vec<_> = self.documents()?.into_iter().map(|d| d.idea).collect();
        ideas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
        Ok(ideas)
    }
    pub fn get(&self, id: &str) -> Result<Idea> {
        let _lock = self.lock()?;
        Ok(self.find(id)?.idea)
    }
    pub fn file_path(&self, id: &str) -> Result<PathBuf> {
        Ok(PathBuf::from(self.get(id)?.path))
    }
    pub fn add(&self, title: &str, body: &str, tags: Vec<String>) -> Result<Idea> {
        self.add_with_source(title, body, tags, None)
    }
    pub fn add_with_source(
        &self,
        title: &str,
        body: &str,
        tags: Vec<String>,
        source: Option<&str>,
    ) -> Result<Idea> {
        let title = title.trim();
        if title.is_empty() {
            return Err(anyhow!("灵感标题不能为空"));
        }
        let _lock = self.lock()?;
        let now = now_utc();
        self.insert(Idea {
            id: new_id(),
            title: title.into(),
            body: body.into(),
            tags,
            created_at: now.clone(),
            updated_at: now,
            source: source.map(str::to_owned),
            path: String::new(),
        })
    }
    pub fn update(
        &self,
        id: &str,
        title: Option<String>,
        body: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Result<Idea> {
        let _lock = self.lock()?;
        let mut doc = self.find(id)?;
        if let Some(title) = title {
            if title.trim().is_empty() {
                return Err(anyhow!("灵感标题不能为空"));
            }
            doc.idea.title = title.trim().into();
        }
        if let Some(body) = body {
            doc.idea.body = body;
        }
        if let Some(tags) = tags {
            doc.idea.tags = tags;
        }
        doc.idea.updated_at = now_utc();
        self.write(&doc)?;
        Ok(doc.idea)
    }
    pub fn delete(&self, id: &str) -> Result<()> {
        let _lock = self.lock()?;
        fs::remove_file(self.find(id)?.idea.path)?;
        Ok(())
    }
    /// Copies legacy JSON records; originals remain untouched. Repeated imports skip existing IDs.
    pub fn migrate_from(&self, old: impl AsRef<Path>) -> Result<usize> {
        let entries = match fs::read_dir(old) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        let _lock = self.lock()?;
        let mut ids: std::collections::HashSet<_> =
            self.documents()?.into_iter().map(|d| d.idea.id).collect();
        let mut count = 0;
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || entry.path().extension().and_then(|s| s.to_str()) != Some("json")
            {
                continue;
            }
            let idea: Idea = serde_json::from_str(&fs::read_to_string(entry.path())?)?;
            if !valid_id(&idea.id) {
                return Err(anyhow!("非法灵感 ID：{}", idea.id));
            }
            if ids.insert(idea.id.clone()) {
                self.insert(idea)?;
                count += 1;
            }
        }
        Ok(count)
    }
    pub fn capture_image(&self, png: &[u8], title: &str, source: Option<&str>) -> Result<Idea> {
        if title.trim().is_empty() {
            return Err(anyhow!("灵感标题不能为空"));
        }
        if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(anyhow!("图片必须是 PNG"));
        }
        let _lock = self.lock()?;
        let attachments = self.dir.join("附件");
        fs::create_dir_all(&attachments)?;
        if fs::canonicalize(&attachments)?.parent() != Some(fs::canonicalize(&self.dir)?.as_path())
        {
            return Err(anyhow!("附件目录不可指向库外"));
        }
        let name = format!("{}.png", new_id());
        let image_path = attachments.join(&name);
        Self::atomic_write(&image_path, png)?;
        let now = now_utc();
        let result = self.insert(Idea {
            id: new_id(),
            title: title.trim().into(),
            body: format!("![图片](附件/{name})\n"),
            tags: vec![],
            created_at: now.clone(),
            updated_at: now,
            source: source.map(str::to_owned),
            path: String::new(),
        });
        if result.is_err() {
            let _ = fs::remove_file(image_path);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_roundtrip_is_plaintext_and_isolated() {
        let tmp = tempfile::tempdir().unwrap();
        let s = InspirationStore::new(tmp.path().join("insp"));
        assert!(s.list().unwrap().is_empty());

        let a = s
            .add("  做个灵感库 ", "明文就行", parse_tags("ky, 产品,ky"))
            .unwrap();
        assert_eq!(a.title, "做个灵感库");
        assert_eq!(a.tags, vec!["ky", "产品"]);
        // 明文落盘，人能直接读
        let raw = fs::read_to_string(s.file_path(&a.id).unwrap()).unwrap();
        assert!(raw.contains("明文就行"));

        let b = s
            .update(&a.id, None, Some("改过".into()), Some(vec![]))
            .unwrap();
        assert_eq!(b.body, "改过");
        assert!(b.tags.is_empty());
        assert_eq!(s.get(&a.id).unwrap(), b);

        assert!(s.get(&a.id).unwrap().matches("灵感"));
        assert!(!s.get(&a.id).unwrap().matches("不存在"));

        s.delete(&a.id).unwrap();
        assert!(s.list().unwrap().is_empty());
        assert!(s.delete(&a.id).is_err());
        // 灵感库目录里只有灵感，不会出现密钥库文件
        assert!(!tmp.path().join("insp/master.key").exists());
    }

    #[test]
    fn concurrent_edits_preserve_other_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let store = InspirationStore::new(tmp.path());
        let idea = store.add("原标题", "原正文", vec![]).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            let id = &idea.id;
            let first = barrier.clone();
            let store = &store;
            scope.spawn(move || {
                first.wait();
                store.update(id, Some("新标题".into()), None, None).unwrap();
            });
            scope.spawn(move || {
                barrier.wait();
                store.update(id, None, Some("新正文".into()), None).unwrap();
            });
        });
        let saved = store.get(&idea.id).unwrap();
        assert_eq!(saved.title, "新标题");
        assert_eq!(saved.body, "新正文");
    }

    #[test]
    fn list_remains_available_during_deletes() {
        let tmp = tempfile::tempdir().unwrap();
        let store = InspirationStore::new(tmp.path());
        let retained = store.add("留下这条", "", vec![]).unwrap();
        let ids: Vec<_> = (0..32)
            .map(|_| store.add("稍后删除", "", vec![]).unwrap().id)
            .collect();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            let first = barrier.clone();
            let store = &store;
            scope.spawn(move || {
                first.wait();
                for id in ids {
                    store.delete(&id).unwrap();
                }
            });
            barrier.wait();
            for _ in 0..64 {
                let items = store.list().unwrap();
                assert!(items.iter().any(|i| i.id == retained.id));
            }
        });
        assert_eq!(store.list().unwrap(), vec![retained]);
    }

    #[test]
    fn rejects_bad_input() {
        let tmp = tempfile::tempdir().unwrap();
        let s = InspirationStore::new(tmp.path());
        assert!(s.add("   ", "", vec![]).is_err());
        assert!(s.get("../secrets").is_err());
        assert!(s.delete("../../etc/passwd").is_err());
    }
}
