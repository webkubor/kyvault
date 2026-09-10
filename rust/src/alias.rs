//! 别名 —— 人设好映射，AI 只用别名。存 ~/.keyring/aliases.json（明文，里面没有密钥）。
//!
//! 这不是可选的糖：Python 版的 get 和 run 都会先过一遍 resolve，
//! 所以老用户脚本里写的是 `kyvault get github_token` 而不是完整 ref。
//! Rust 版少了这一步，那些脚本会直接报「找不到」。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Result};

pub struct Aliases {
    path: PathBuf,
}

impl Aliases {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            path: root.into().join("aliases.json"),
        }
    }

    pub fn default_location() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("找不到 home 目录"))?;
        Ok(Self::new(home.join(".keyring")))
    }

    /// 读不出来当空表 —— 别名坏了不该让 get/run 整个不可用，
    /// 那样连完整 ref 都没法读了。
    pub fn load(&self) -> BTreeMap<String, String> {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, m: &BTreeMap<String, String>) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&self.path, serde_json::to_string_pretty(m)?)?;
        Ok(())
    }

    pub fn set(&self, name: &str, r: &str) -> Result<()> {
        let mut m = self.load();
        m.insert(name.to_string(), r.to_string());
        self.save(&m)
    }

    pub fn get(&self, name: &str) -> Option<String> {
        self.load().get(name).cloned()
    }

    pub fn delete(&self, name: &str) -> Result<bool> {
        let mut m = self.load();
        let hit = m.remove(name).is_some();
        if hit {
            self.save(&m)?;
        }
        Ok(hit)
    }

    /// secret:// 开头的原样返回；否则查别名；查不到也原样返回
    /// —— 与 Python 版同行为，让「找不到」这个错误由后端统一报，
    /// 报的是完整 ref，比在这里报「别名不存在」更好定位。
    pub fn resolve(&self, ref_or_alias: &str) -> String {
        if ref_or_alias.starts_with("secret://") {
            return ref_or_alias.to_string();
        }
        self.get(ref_or_alias)
            .unwrap_or_else(|| ref_or_alias.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (tempfile::TempDir, Aliases) {
        let d = tempfile::tempdir().unwrap();
        let a = Aliases::new(d.path());
        (d, a)
    }

    #[test]
    fn set_get_delete() {
        let (_d, a) = tmp();
        assert!(a.get("gh").is_none());
        a.set("gh", "secret://github/pat").unwrap();
        assert_eq!(a.get("gh").unwrap(), "secret://github/pat");
        assert!(a.delete("gh").unwrap());
        assert!(
            !a.delete("gh").unwrap(),
            "删第二次应返回 false，不能静默当成功"
        );
    }

    #[test]
    fn resolve_passthrough_and_lookup() {
        let (_d, a) = tmp();
        a.set("gh", "secret://github/pat").unwrap();
        assert_eq!(
            a.resolve("secret://x/y"),
            "secret://x/y",
            "完整 ref 原样返回"
        );
        assert_eq!(a.resolve("gh"), "secret://github/pat", "别名要被解析");
        assert_eq!(
            a.resolve("unknown"),
            "unknown",
            "查不到原样返回，由后端报错"
        );
    }

    #[test]
    fn broken_json_degrades_to_empty() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("aliases.json"), "{ 坏掉的 json").unwrap();
        let a = Aliases::new(d.path());
        assert!(a.load().is_empty(), "别名文件坏了不该让整个工具不可用");
        assert_eq!(a.resolve("secret://x/y"), "secret://x/y");
    }
}
