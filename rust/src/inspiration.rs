//! 灵感库 —— 随手记的想法，**明文**存放，不碰密钥库。
//!
//! 为什么不加密：灵感不是凭据，加密只会让 grep / 同步 / 换机迁移变麻烦，
//! 也不该依赖 master.key 是否就绪。所以这里完全独立于 `Store`：
//! 不读 master.key、不写 secrets.json、不进 audit.log。
//!
//! 存储：`KY_INSPIRATION_DIR` 优先，否则 `~/.config/kyvault/inspiration`；
//! 每条一个 `<id>.json`，人能直接打开看，也方便单独同步。
//! **不要往这里记密钥** —— 它就是明文。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

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
}

impl Idea {
    /// 关键词匹配标题/正文/标签，不区分大小写。
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || self.title.to_lowercase().contains(&q)
            || self.body.to_lowercase().contains(&q)
            || self.tags.iter().any(|t| t.to_lowercase().contains(&q))
    }
}

pub struct InspirationStore {
    dir: PathBuf,
}

pub fn default_dir() -> Result<PathBuf> {
    if let Ok(d) = std::env::var("KY_INSPIRATION_DIR") {
        let d = d.trim();
        if !d.is_empty() {
            return Ok(PathBuf::from(crate::store::shellexpand_tilde(d)));
        }
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow!("找不到 home 目录"))?;
    Ok(home.join(".config").join("kyvault").join("inspiration"))
}

/// 逗号分隔的标签 → 去空白、去重、保序。
pub fn parse_tags(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
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
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn new_id() -> String {
    let mut bytes = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("idea-{hex}")
}

impl InspirationStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn default_location() -> Result<Self> {
        Ok(Self::new(default_dir()?))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err(anyhow!("非法灵感 ID：{id}"));
        }
        Ok(self.dir.join(format!("{id}.json")))
    }

    // CLI 和桌面可同时写入；锁涵盖读取、修改与替换，避免并发更新互相覆盖。
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

    // 调用者持有库锁；先写完并同步临时文件，再替换完整记录。
    fn write(&self, idea: &Idea) -> Result<()> {
        let path = self.path(&idea.id)?;
        let tmp = path.with_extension("json.tmp");
        let result = (|| -> Result<()> {
            let mut file = File::create(&tmp)?;
            file.write_all((serde_json::to_string_pretty(idea)? + "\n").as_bytes())?;
            file.sync_all()?;
            fs::rename(&tmp, &path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    /// 按更新时间倒序（最新在前）。目录不存在 = 空库。
    pub fn list(&self) -> Result<Vec<Idea>> {
        let rd = match fs::read_dir(&self.dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for entry in rd {
            let p = entry?.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let raw = match fs::read_to_string(&p) {
                Ok(raw) => raw,
                // 列表枚举后，另一进程可能刚好删除这条；仍显示其余灵感。
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e).with_context(|| format!("读取灵感失败：{}", p.display())),
            };
            let idea: Idea = serde_json::from_str(&raw)
                .with_context(|| format!("灵感文件格式错误：{}", p.display()))?;
            out.push(idea);
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
        Ok(out)
    }

    pub fn get(&self, id: &str) -> Result<Idea> {
        let p = self.path(id)?;
        let raw =
            fs::read_to_string(&p).with_context(|| format!("读取灵感失败：{}", p.display()))?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn add(&self, title: &str, body: &str, tags: Vec<String>) -> Result<Idea> {
        let title = title.trim();
        if title.is_empty() {
            return Err(anyhow!("灵感标题不能为空"));
        }
        let _lock = self.lock()?;
        let now = now_utc();
        let mut id = new_id();
        while self.path(&id)?.exists() {
            id = new_id();
        }
        let idea = Idea {
            id,
            title: title.to_string(),
            body: body.to_string(),
            tags,
            created_at: now.clone(),
            updated_at: now,
        };
        self.write(&idea)?;
        Ok(idea)
    }

    pub fn update(
        &self,
        id: &str,
        title: Option<String>,
        body: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Result<Idea> {
        let _lock = self.lock()?;
        let mut idea = self.get(id)?;
        if let Some(t) = title {
            let t = t.trim();
            if t.is_empty() {
                return Err(anyhow!("灵感标题不能为空"));
            }
            idea.title = t.to_string();
        }
        if let Some(b) = body {
            idea.body = b;
        }
        if let Some(t) = tags {
            idea.tags = t;
        }
        idea.updated_at = now_utc();
        self.write(&idea)?;
        Ok(idea)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let p = self.path(id)?;
        let _lock = self.lock()?;
        fs::remove_file(&p).with_context(|| format!("删除灵感失败：{}", p.display()))
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
        let raw = fs::read_to_string(s.dir().join(format!("{}.json", a.id))).unwrap();
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
