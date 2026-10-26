//! 账户注册表 —— 把散落的密钥收成「一个账号」。
//!
//! 现状是五套平行的命令：`account`（用户名+密码）、`key`（平台多密钥）、
//! `cli`（CLI 多 profile）、`server`（资产台账）、`identity`（按机器人聚合）。
//! 它们各自都对，但回答不了最常见的那句问话：**「我到底有几个 GitHub 账号，
//! 每个账号下挂了几条东西，现在用的是哪个？」**
//!
//! 这个模块补的就是这一层实体：`accounts.json` 里一个 profile = 一个账号，
//! 带平台、登录名、组织、对应的 CLI、以及它名下所有 secret ref。
//!
//! ## 为什么明文存、为什么入仓
//!
//! 里面只有「账号名 / 登录名 / 组织 / ref 路径」这类**结构性元信息**，没有任何
//! 密钥材料 —— 和 `meta.json` 同一敏感级别，而 `meta.json` 本来就是入仓的。
//! 入仓的好处是团队看到的是同一份「谁负责哪个账号」，这正是用户问的「多账户
//! 维护」。密码与 token 仍然只在 `secrets.json` 的密文里。
//!
//! 文件权限照样收紧到 0600：不是因为它含密钥，而是里面写着别人的登录名。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::now_utc;
use crate::store::harden;

pub const FILE: &str = "accounts.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Profile {
    /// profile 名，命令行里用的那个（work / personal / client-a）
    pub name: String,
    /// 平台：github / gitlab / lark / aws …；留空表示「跨平台账号」
    #[serde(default)]
    pub platform: String,
    /// 登录名 / 邮箱 / 账号 ID
    #[serde(default)]
    pub username: String,
    /// 归属组织，和 SecretMeta.org 同一套取值
    #[serde(default)]
    pub org: String,
    /// 对应哪个 CLI（gh / glab / lark-cli），用于 `ky run --profile` 的场景提示
    #[serde(default)]
    pub cli: String,
    /// 属于这个账号的 secret ref，如 secret://github/pat
    #[serde(default)]
    pub refs: Vec<String>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FileShape {
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    profiles: BTreeMap<String, Profile>,
}

pub struct Registry {
    path: PathBuf,
    data: FileShape,
}

impl Registry {
    pub fn at(root: &Path) -> Result<Self> {
        let mut data = Self::read(root.join(FILE))?;
        // name 以 key 为准：手改过文件的人不该看到「键叫 work、内文叫 office」
        for (k, v) in data.profiles.iter_mut() {
            if v.name != *k {
                v.name = k.clone();
            }
        }
        Ok(Self {
            path: root.join(FILE),
            data,
        })
    }

    fn read(p: PathBuf) -> Result<FileShape> {
        if !p.exists() {
            return Ok(FileShape::default());
        }
        let text =
            std::fs::read_to_string(&p).with_context(|| format!("读 {} 失败", p.display()))?;
        // 读不出来就报错，不静默当空表 —— 空表被 save 一次就把账号全删了
        let value: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("{} 不是合法 JSON", p.display()))?;
        if !value.is_object() {
            return Err(anyhow!("accounts.json 顶层必须是对象，拒绝读取/覆盖"));
        }
        serde_json::from_value(value).context("accounts.json 字段结构无效")
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.data)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text).with_context(|| format!("写 {} 失败", tmp.display()))?;
        harden(&tmp)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn active(&self) -> Option<&str> {
        self.data.active.as_deref()
    }

    pub fn list(&self) -> Vec<&Profile> {
        self.data.profiles.values().collect()
    }

    pub fn get(&self, name: &str) -> Option<&Profile> {
        self.data.profiles.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.data.profiles.is_empty()
    }

    /// 新增或覆盖。覆盖是**合并**语义：没传的字段保留原值，避免
    /// `ky profile add work --org hym` 这种局部更新把 username 清空。
    pub fn upsert(&mut self, p: Profile) -> Result<&Profile> {
        if p.name.trim().is_empty() {
            return Err(anyhow!("profile 名不能为空"));
        }
        let name = p.name.clone();
        let entry = self
            .data
            .profiles
            .entry(name.clone())
            .or_insert_with(|| Profile {
                name: name.clone(),
                created_at: now_utc(),
                ..Default::default()
            });
        if !p.platform.is_empty() {
            entry.platform = p.platform;
        }
        if !p.username.is_empty() {
            entry.username = p.username;
        }
        if !p.org.is_empty() {
            entry.org = p.org;
        }
        if !p.cli.is_empty() {
            entry.cli = p.cli;
        }
        if !p.note.is_empty() {
            entry.note = p.note;
        }
        for r in p.refs {
            if !entry.refs.contains(&r) {
                entry.refs.push(r);
            }
        }
        if entry.created_at.is_empty() {
            entry.created_at = now_utc();
        }
        // 第一个 profile 自动成为当前账号：否则 `ky profile current` 永远空着，
        // 用户得再记一条命令
        if self.data.active.is_none() {
            self.data.active = Some(name.clone());
        }
        self.save()?;
        Ok(self.data.profiles.get(&name).expect("刚插入"))
    }

    pub fn set_active(&mut self, name: &str) -> Result<()> {
        if !self.data.profiles.contains_key(name) {
            return Err(anyhow!(
                "没有 profile {name}（先 ky profile add {name} ...）"
            ));
        }
        self.data.active = Some(name.to_string());
        self.save()
    }

    pub fn link(&mut self, name: &str, r: &str) -> Result<()> {
        if !r.starts_with("secret://") {
            return Err(anyhow!("ref 应以 secret:// 开头：{r}"));
        }
        let e = self
            .data
            .profiles
            .get_mut(name)
            .ok_or_else(|| anyhow!("没有 profile {name}"))?;
        if !e.refs.iter().any(|x| x == r) {
            e.refs.push(r.to_string());
        }
        self.save()
    }

    pub fn unlink(&mut self, name: &str, r: &str) -> Result<bool> {
        let e = self
            .data
            .profiles
            .get_mut(name)
            .ok_or_else(|| anyhow!("没有 profile {name}"))?;
        let before = e.refs.len();
        e.refs.retain(|x| x != r);
        let removed = e.refs.len() != before;
        self.save()?;
        Ok(removed)
    }

    pub fn remove(&mut self, name: &str) -> Result<bool> {
        let removed = self.data.profiles.remove(name).is_some();
        if removed {
            if self.data.active.as_deref() == Some(name) {
                self.data.active = None;
            }
            self.save()?;
        }
        Ok(removed)
    }
}

/// `ky run --profile` 用的环境变量名：`gh/work` → `GH_WORK`。
///
/// 规则只有一条：非字母数字一律变下划线，再整体大写。不做任何「聪明」的
/// 缩写 —— 猜错名字比让用户自己写 `--env` 更费时间。
pub fn env_name(ref_name: &str) -> String {
    let mut out = String::with_capacity(ref_name.len() + 2);
    for c in ref_name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_name_is_upper_snake() {
        assert_eq!(env_name("gh/work"), "GH_WORK");
        assert_eq!(env_name("pat"), "PAT");
        assert_eq!(env_name("a-b.c"), "A_B_C");
    }

    #[test]
    fn upsert_merges_and_first_becomes_active() {
        let dir = tempfile::tempdir().unwrap();
        let mut reg = Registry::at(dir.path()).unwrap();
        reg.upsert(Profile {
            name: "work".into(),
            platform: "github".into(),
            username: "alice".into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(reg.active(), Some("work"));

        // 局部更新不该清空已有字段
        reg.upsert(Profile {
            name: "work".into(),
            org: "hym".into(),
            ..Default::default()
        })
        .unwrap();
        let p = reg.get("work").unwrap();
        assert_eq!(p.username, "alice");
        assert_eq!(p.org, "hym");
    }

    #[test]
    fn link_and_unlink() {
        let dir = tempfile::tempdir().unwrap();
        let mut reg = Registry::at(dir.path()).unwrap();
        reg.upsert(Profile {
            name: "work".into(),
            ..Default::default()
        })
        .unwrap();
        reg.link("work", "secret://github/pat").unwrap();
        reg.link("work", "secret://github/pat").unwrap(); // 幂等
        assert_eq!(reg.get("work").unwrap().refs.len(), 1);
        assert!(reg.unlink("work", "secret://github/pat").unwrap());
        assert!(!reg.unlink("work", "secret://github/pat").unwrap());
    }

    #[test]
    fn second_profile_does_not_steal_active() {
        let dir = tempfile::tempdir().unwrap();
        let mut reg = Registry::at(dir.path()).unwrap();
        reg.upsert(Profile {
            name: "work".into(),
            ..Default::default()
        })
        .unwrap();
        reg.upsert(Profile {
            name: "personal".into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(reg.active(), Some("work"));
        reg.set_active("personal").unwrap();
        assert_eq!(reg.active(), Some("personal"));
        reg.remove("personal").unwrap();
        assert_eq!(reg.active(), None);
    }
}
