//! 本地 file 后端 —— 加密存 ~/.keyring/secrets.json，零网络。
//!
//! **读-改-写必须保留未知字段**，所以全程操作 serde_json::Value 而不是
//! 反序列化到强类型结构。secrets.json 是异构的：平台键（{accounts,keys}）、
//! `_servers`、`_clis`，还有更早的扁平格式 `"platform/name": {ciphertext,...}`。
//! 用强类型 struct 往返一次，就会把没建模的那些整段丢掉 —— 对密钥库来说
//! 那不是丢字段，是丢密钥。
//!
//! Store 带 root 而不是用全局常量，纯粹为了可测：测试能指到临时目录，
//! 不会去动真的 ~/.keyring。

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use fs2::FileExt;
use serde_json::{json, Map, Value};

use crate::crypto::{decrypt_joined, derive_file_key, encrypt_joined, new_master_key_b64};
use crate::model::{parse_ref, SecretMeta};

pub struct StoreLock {
    file: std::fs::File,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

pub struct Store {
    root: PathBuf,
}

/// 把文件权限收紧到「只有本人可读写」。
///
/// Unix 上就是 0600。Windows 没有 mode 这个概念，文件继承的是用户目录的 ACL
/// （`C:\Users\<name>` 默认只有本人和管理员能进），所以这里不做额外处理 ——
/// 但**不能因此就让 `from_mode` 出现在 Windows 的编译路径里**：
/// `std::os::unix` 在 Windows 上根本不存在，无条件 use 会直接编译失败。
/// 这正是此前 CI 里没有 Windows target 的隐性原因之一。
pub fn harden(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// 密钥库根目录：`KYVAULT_STORE_DIR` 优先，其次 `~/.config/kyvault/store`（若已就绪），回落 `~/.keyring`。
///
/// store 和 alias 都用它，避免两处各读一次环境变量后走岔（一个指到 git 仓、
/// 一个还在 ~/.keyring，表现是「密钥能读到但别名全丢」，很难往这上面想）。
pub fn default_store_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("KYVAULT_STORE_DIR") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Ok(PathBuf::from(shellexpand_tilde(dir)));
        }
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow!("找不到 home 目录"))?;
    // 优先使用 ~/.config/kyvault/store（GitLab 私有团队仓）。
    // 密文存在 + 根密钥可用（明文 master.key 或 Auth Guard 模式的 master.key.enc）
    // 才算就绪，避免指到尚未初始化的空目录。
    let preferred = home.join(".config").join("kyvault").join("store");
    let has_secrets = preferred.join("secrets.json").exists();
    let has_master = preferred.join("master.key").exists()
        || (preferred.join("master.key.enc").exists() && preferred.join("auth.json").exists());
    if has_secrets && has_master {
        return Ok(preferred);
    }
    Ok(home.join(".keyring"))
}

/// 只展开开头的 `~` —— 环境变量里写 `~/.config/kyvault/store` 很自然，
/// 但它由 shell 展开，直接塞进 env 时不会展开，落到这里就是个字面量目录名。
pub fn shellexpand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    p.to_string()
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 默认位置。`KYVAULT_STORE_DIR` 未设置时是 `~/.keyring`（历史默认，保持兼容）。
    ///
    /// 可配是为了让密钥库能放进一个 git 仓库 —— 把 store 指到 clone 下来的目录，
    /// `secrets.json`（密文）入仓、`master.key` 靠 .gitignore 留在本机，就得到了
    /// 「GitLab 私有托管」形态，而加密逻辑一行都不用改：GitLab 后端本质就是
    /// file 后端 + 一个 git 远端。
    ///
    /// 2026-09-22 起这是推荐形态。原因见 docs/proposals 那份 GitLab 后端方案，
    /// 以及 D1 后端那次事故：解密本该是纯本地计算，不该被任何远端服务的可用性绑架。
    pub fn default_location() -> Result<Self> {
        Ok(Self::new(default_store_dir()?))
    }

    fn secrets_file(&self) -> PathBuf {
        self.root.join("secrets.json")
    }

    fn master_key_file(&self) -> PathBuf {
        self.root.join("master.key")
    }

    // 下面三个只读访问器给 doctor 用 —— 自检要报「文件在不在、权限对不对」，
    // 就得知道路径。刻意只暴露读，不给外部改 root 的机会。
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn master_key_path(&self) -> PathBuf {
        self.master_key_file()
    }

    pub fn secrets_path(&self) -> PathBuf {
        self.secrets_file()
    }

    pub fn meta_file(&self) -> PathBuf {
        self.root.join("meta.json")
    }

    /// 获取 store 目录的跨进程排他锁（~/.lock 文件）
    pub fn lock(&self) -> Result<StoreLock> {
        fs::create_dir_all(&self.root)?;
        let lock_path = self.root.join(".lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .context("无法打开锁文件")?;
        file.lock_exclusive()
            .context("无法获取密钥库排他锁（可能被其他进程/Agent 占用）")?;
        Ok(StoreLock { file })
    }

    pub fn load_meta(&self) -> Result<serde_json::Map<String, Value>> {
        let f = self.meta_file();
        if !f.exists() {
            return Ok(serde_json::Map::new());
        }
        let content = fs::read_to_string(&f)?;
        let val: Value = serde_json::from_str(&content).context("meta.json 语法损坏")?;
        Ok(val.as_object().cloned().unwrap_or_default())
    }

    pub fn save_meta(&self, meta: &serde_json::Map<String, Value>) -> Result<()> {
        let f = self.meta_file();
        let content = serde_json::to_string_pretty(&Value::Object(meta.clone()))?;
        let tmp = self.root.join("meta.json.tmp");
        fs::write(&tmp, content)?;
        harden(&tmp)?;
        fs::rename(tmp, f)?;
        Ok(())
    }

    /// master key：环境变量优先，其次 Auth Guard 解锁，最后明文文件。
    /// 与 Python 版同序，否则 CI/容器里注入的 key 会被本地文件悄悄盖掉。
    pub fn master_key(&self) -> Result<String> {
        // 1. 环境变量注入（CI / 容器 / Agent 场景，完全绕过 guard）
        if let Ok(k) = std::env::var("KEYRING_MASTER_KEY") {
            if !k.trim().is_empty() {
                return Ok(k.trim().to_string());
            }
        }
        // 2. Auth Guard 启用时，走 SSH Key 自动解锁
        if crate::auth_guard::AuthGuard::is_enabled(&self.root) {
            let guard = crate::auth_guard::AuthGuard::load(&self.root)?;
            return guard.unlock();
        }
        // 3. 传统模式：直接读明文 master.key
        let f = self.master_key_file();
        if f.exists() {
            return Ok(fs::read_to_string(&f)?.trim().to_string());
        }
        Err(anyhow!("未初始化。运行 kyvault init 生成 master key。"))
    }

    /// 已存在就原样返回 —— 绝不覆盖：覆盖 master key 等于让全部存量密文永久解不开。
    pub fn init_master_key(&self) -> Result<String> {
        let f = self.master_key_file();
        if f.exists() {
            return Ok(fs::read_to_string(&f)?.trim().to_string());
        }
        fs::create_dir_all(&self.root)?;
        let key = new_master_key_b64();
        fs::write(&f, &key)?;
        harden(&f)?;
        Ok(key)
    }

    fn aes_key(&self) -> Result<[u8; 32]> {
        derive_file_key(&self.master_key()?)
    }

    /// 加载库内容。文件不存在 → 空对象（init 流程需要）；其余读不出 / JSON
    /// 损坏 / 含 git 冲突标记 → 报错返回。
    ///
    /// 旧的"读不出来就当空库"是有意的（不想让手改坏的 json 把整个工具锁死），
    /// 但代价是 rebase 冲突标记进入 secrets.json 时，下一次 `set` 会**拿空对象
    /// 覆盖整库**。Codex 在 2026-09 抓到这个 bug 后，把宽容只留给"文件不存在"
    /// 这一种合法空库场景；其他所有"读不出"都拒绝写入。
    pub fn load(&self) -> Result<Value> {
        let path = self.secrets_file();
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({})),
            Err(e) => {
                return Err(anyhow!(
                    "读 {} 失败：{}。先确认文件权限，未恢复前不要写入。",
                    path.display(),
                    e
                ));
            }
        };
        // 顺序：先试 JSON 解析；**只在解析失败**时才细化冲突标记。
        // 合法 JSON 里也可以包含 "=======" 这样的字符串（作为 base64 密文或
        // 名称 / 备注），如果先 contains 就会**误判合法数据**，比旧 bug 还糟。
        match serde_json::from_str(&content) {
            Ok(v) => Ok(v),
            Err(_) if Self::has_git_conflict_markers(&content) => Err(anyhow!(
                "{} 含 git 冲突标记（<<<<<<< / >>>>>>>）。\n\
                 先 git rebase --abort 或手动解冲突，再 kyvault list 验证库能正常读出。\n\
                 未解决冲突前 set / delete 都会被拒绝，原数据保持不变。",
                path.display()
            )),
            Err(e) => Err(anyhow!("{} 不是合法 JSON：{}", path.display(), e)),
        }
    }

    /// 检测 git conflict markers。**只在 JSON 解析失败后调用**，所以可以用
    /// 粗粒度 contains —— 合法 JSON 里碰巧包含这些字符串是合法的，误判风险
    /// 由外层 `Err(_) if ...` 兜住：JSON 合法就根本不会走到这里。
    ///
    /// `<<<<<<<` / `=======` / `>>>>>>>` 是合并冲突；`|||||||` 是 diff3 风格的
    /// 共同祖先段（merge.conflictStyle=diff3 时出现）。
    fn has_git_conflict_markers(content: &str) -> bool {
        content.contains("<<<<<<<") || content.contains(">>>>>>>") || content.contains("|||||||")
    }

    fn save(&self, data: &Value) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        let path = self.secrets_file();
        // 先写临时文件再 rename：写一半被打断也不会留下半个 json 把库废掉
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(data)?)
            .with_context(|| format!("写 {} 失败", tmp.display()))?;
        harden(&tmp)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn obj_mut<'a>(root: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
        root.as_object_mut()
            .expect("顶层必须是对象")
            .entry(key.to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("该键必须是对象")
    }

    // ── secret:// 寻址 ─────────────────────────────────────

    pub fn get_secret(&self, r: &str) -> Result<Option<String>> {
        let (platform, name) = parse_ref(r)?;
        let key = self.aes_key()?;
        let data = self.load()?;

        if platform == "server" {
            let mut it = name.splitn(2, '/');
            let host = it.next().unwrap_or_default();
            return match it.next() {
                Some(field) => self.decrypt_at(&data, &["_servers", host, field], &key),
                None => {
                    // 不带字段时返回整台机器的 json，与 Python 版一致
                    let Some(fields) = data
                        .pointer(&format!("/_servers/{host}"))
                        .and_then(|v| v.as_object())
                    else {
                        return Ok(None);
                    };
                    let mut out = Map::new();
                    for (k, v) in fields {
                        out.insert(
                            k.clone(),
                            Value::String(decrypt_joined(v.as_str().unwrap_or_default(), &key)?),
                        );
                    }
                    Ok(Some(serde_json::to_string(&out)?))
                }
            };
        }
        if platform == "cli" {
            if let Some((cli, profile)) = name.split_once('/') {
                return self.decrypt_at(&data, &["_clis", cli, profile], &key);
            }
        }
        if !platform.starts_with('_') {
            for bucket in ["keys", "accounts"] {
                if let Some(v) = self.decrypt_at(&data, &[&platform, bucket, &name], &key)? {
                    return Ok(Some(v));
                }
            }
        }
        // 旧扁平格式：{"platform/name": {"ciphertext": ...}}
        if let Some(ct) = data
            .get(format!("{platform}/{name}"))
            .and_then(|v| v.get("ciphertext"))
            .and_then(|v| v.as_str())
        {
            return Ok(Some(decrypt_joined(ct, &key)?));
        }
        Ok(None)
    }

    fn decrypt_at(&self, data: &Value, path: &[&str], key: &[u8; 32]) -> Result<Option<String>> {
        let mut cur = data;
        for p in path {
            match cur.get(*p) {
                Some(v) => cur = v,
                None => return Ok(None),
            }
        }
        match cur.as_str() {
            Some(ct) => Ok(Some(decrypt_joined(ct, key)?)),
            None => Ok(None),
        }
    }

    pub fn set_secret(&self, r: &str, value: &str) -> Result<()> {
        self.set_secret_with_meta(r, value, None, None)
    }

    pub fn set_secret_with_meta(
        &self,
        r: &str,
        value: &str,
        kind: Option<&str>,
        account: Option<&str>,
    ) -> Result<()> {
        let _lock = self.lock()?;
        let (platform, name) = parse_ref(r)?;
        let key = self.aes_key()?;
        let ct = encrypt_joined(value, &key)?;
        let mut data = self.load()?;

        if platform == "cli" {
            if let Some((cli, profile)) = name.split_once('/') {
                let clis = Self::obj_mut(&mut data, "_clis");
                clis.entry(cli.to_string())
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .unwrap()
                    .insert(profile.to_string(), Value::String(ct));
                self.save(&data)?;
                self.update_meta_for_set(&platform, &name, value, kind, account)?;
                return Ok(());
            }
        }
        if platform == "server" {
            if let Some((host, field)) = name.split_once('/') {
                let servers = Self::obj_mut(&mut data, "_servers");
                servers
                    .entry(host.to_string())
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .unwrap()
                    .insert(field.to_string(), Value::String(ct));
                self.save(&data)?;
                self.update_meta_for_set(&platform, &name, value, kind, account)?;
                return Ok(());
            }
        }
        let p = Self::obj_mut(&mut data, &platform);
        p.entry("accounts".to_string()).or_insert_with(|| json!({}));
        p.entry("keys".to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(name.clone(), Value::String(ct));
        self.save(&data)?;
        self.update_meta_for_set(&platform, &name, value, kind, account)?;
        Ok(())
    }

    fn update_meta_for_set(
        &self,
        platform: &str,
        name: &str,
        value: &str,
        kind: Option<&str>,
        account: Option<&str>,
    ) -> Result<()> {
        if self.meta_file().exists() {
            let mut meta = self.load_meta().unwrap_or_default();
            let key = format!("{platform}/{name}");
            let mut entry = meta
                .get(&key)
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            if let Some(k) = kind {
                if !k.is_empty() {
                    entry.insert("kind".into(), Value::String(k.to_string()));
                }
            } else if !entry.contains_key("kind") {
                entry.insert("kind".into(), Value::String("API Key".to_string()));
            }
            if let Some(a) = account {
                if !a.is_empty() {
                    entry.insert("account".into(), Value::String(a.to_string()));
                }
            }
            entry.insert("last4".into(), Value::String(crate::model::last4(value)));
            entry.insert("length".into(), Value::Number(value.chars().count().into()));
            entry.insert(
                "sha256".into(),
                Value::String(crate::model::sha256_hex(value)),
            );
            entry.insert("updated_at".into(), Value::String(crate::model::now_utc()));
            if !entry.contains_key("created_at") {
                entry.insert("created_at".into(), Value::String(crate::model::now_utc()));
            }
            meta.insert(key, Value::Object(entry));
            let _ = self.save_meta(&meta);
        }
        Ok(())
    }

    pub fn annotate(
        &self,
        r: &str,
        account: Option<&str>,
        kind: Option<&str>,
        org: Option<&str>,
        scopes: Option<&str>,
        visibility: Option<&str>,
    ) -> Result<bool> {
        let _lock = self.lock()?;
        let (platform, name) = parse_ref(r)?;
        let key = format!("{platform}/{name}");
        let mut meta = self.load_meta()?;
        let mut entry = meta
            .get(&key)
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        if let Some(a) = account {
            entry.insert("account".into(), Value::String(a.to_string()));
        }
        if let Some(k) = kind {
            entry.insert("kind".into(), Value::String(k.to_string()));
        }
        if let Some(o) = org {
            entry.insert("org".into(), Value::String(o.to_string()));
        }
        if let Some(s) = scopes {
            entry.insert("scopes".into(), Value::String(s.to_string()));
        }
        if let Some(v) = visibility {
            entry.insert("visibility".into(), Value::String(v.to_string()));
        }
        entry.insert("updated_at".into(), Value::String(crate::model::now_utc()));
        meta.insert(key, Value::Object(entry));
        self.save_meta(&meta)?;
        Ok(true)
    }

    pub fn delete_secret(&self, r: &str) -> Result<bool> {
        let _lock = self.lock()?;
        let (platform, name) = parse_ref(r)?;
        let mut data = self.load()?;
        let removed = {
            let root = data.as_object_mut().ok_or_else(|| anyhow!("库结构损坏"))?;
            let mut hit = false;
            if platform == "cli" {
                if let Some((cli, profile)) = name.split_once('/') {
                    if let Some(m) = root
                        .get_mut("_clis")
                        .and_then(|v| v.get_mut(cli))
                        .and_then(|v| v.as_object_mut())
                    {
                        hit = m.remove(profile).is_some();
                    }
                }
            } else if platform == "server" {
                let host = name.split('/').next().unwrap_or_default();
                if let Some(m) = root.get_mut("_servers").and_then(|v| v.as_object_mut()) {
                    hit = m.remove(host).is_some();
                }
            } else {
                if let Some(m) = root
                    .get_mut(&platform)
                    .and_then(|v| v.get_mut("keys"))
                    .and_then(|v| v.as_object_mut())
                {
                    hit = m.remove(&name).is_some();
                }
                if !hit {
                    hit = root.remove(&format!("{platform}/{name}")).is_some();
                }
            }
            hit
        };
        if removed {
            self.save(&data)?;
            if self.meta_file().exists() {
                if let Ok(mut meta) = self.load_meta() {
                    let key = format!("{platform}/{name}");
                    if meta.remove(&key).is_some() {
                        let _ = self.save_meta(&meta);
                    }
                }
            }
        }
        Ok(removed)
    }

    pub fn list_secrets(&self) -> Result<Vec<SecretMeta>> {
        let data = self.load()?;
        let mut out = Vec::new();
        let Some(root) = data.as_object() else {
            return Ok(out);
        };

        if let Some(servers) = root.get("_servers").and_then(|v| v.as_object()) {
            for (host, fields) in servers {
                for field in fields
                    .as_object()
                    .map(|m| m.keys().collect::<Vec<_>>())
                    .unwrap_or_default()
                {
                    out.push(SecretMeta {
                        platform: "server".into(),
                        name: format!("{host}/{field}"),
                        kind: "Server Field".into(),
                        ..Default::default()
                    });
                }
            }
        }
        if let Some(clis) = root.get("_clis").and_then(|v| v.as_object()) {
            for (cli, profiles) in clis {
                for profile in profiles
                    .as_object()
                    .map(|m| m.keys().collect::<Vec<_>>())
                    .unwrap_or_default()
                {
                    out.push(SecretMeta {
                        platform: "cli".into(),
                        name: format!("{cli}/{profile}"),
                        kind: "CLI Token".into(),
                        account: profile.clone(),
                        ..Default::default()
                    });
                }
            }
        }
        for (platform, v) in root {
            if platform.starts_with('_') {
                continue;
            }
            if let Some(keys) = v.get("keys").and_then(|k| k.as_object()) {
                for name in keys.keys() {
                    out.push(SecretMeta {
                        platform: platform.clone(),
                        name: name.clone(),
                        kind: "Key".into(),
                        ..Default::default()
                    });
                }
            }
            if let Some(accounts) = v.get("accounts").and_then(|k| k.as_object()) {
                for user in accounts.keys() {
                    out.push(SecretMeta {
                        platform: platform.clone(),
                        name: user.clone(),
                        kind: "Account".into(),
                        account: user.clone(),
                        ..Default::default()
                    });
                }
            }
            // 旧扁平格式
            if v.get("keys").is_none() && v.get("accounts").is_none() {
                if let Some(ct) = v.as_object() {
                    if ct.contains_key("ciphertext") {
                        if let Some((p, n)) = platform.split_once('/') {
                            out.push(SecretMeta {
                                platform: p.into(),
                                name: n.into(),
                                kind: ct.get("kind").and_then(|k| k.as_str()).unwrap_or("").into(),
                                account: ct
                                    .get("account")
                                    .and_then(|k| k.as_str())
                                    .unwrap_or("")
                                    .into(),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| (&a.platform, &a.name).cmp(&(&b.platform, &b.name)));
        if self.meta_file().exists() {
            if let Ok(meta_map) = self.load_meta() {
                for item in &mut out {
                    let key = format!("{}/{}", item.platform, item.name);
                    if let Some(m) = meta_map.get(&key).and_then(|v| v.as_object()) {
                        if let Some(kind) = m.get("kind").and_then(|v| v.as_str()) {
                            if !kind.is_empty() {
                                item.kind = kind.to_string();
                            }
                        }
                        if let Some(account) = m.get("account").and_then(|v| v.as_str()) {
                            if !account.is_empty() {
                                item.account = account.to_string();
                            }
                        }
                        if let Some(last4) = m.get("last4").and_then(|v| v.as_str()) {
                            item.last4 = last4.to_string();
                        }
                        if let Some(len) = m.get("length").and_then(|v| v.as_u64()) {
                            item.length = len;
                        }
                        if let Some(updated_at) = m.get("updated_at").and_then(|v| v.as_str()) {
                            item.updated_at = updated_at.to_string();
                        }
                        if let Some(org) = m.get("org").and_then(|v| v.as_str()) {
                            item.org = org.to_string();
                        }
                        if let Some(scopes) = m.get("scopes").and_then(|v| v.as_str()) {
                            item.scopes = scopes.to_string();
                        }
                        if let Some(visibility) = m.get("visibility").and_then(|v| v.as_str()) {
                            item.visibility = visibility.to_string();
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    // ── account / key / platform 命名空间 ─────────────────
    //
    // 这三组是**只有本地 file 后端**的能力（Python 版同样如此）：D1 那张
    // secret_vault 表是扁平的 id→密文，没有 accounts 这个桶。所以 CLI 层
    // 直接用 Store，不走后端分发 —— 假装支持再静默落到别处，比明确只支持本地糟。

    pub fn set_account(&self, platform: &str, user: &str, password: &str) -> Result<()> {
        let _lock = self.lock()?;
        let ct = encrypt_joined(password, &self.aes_key()?)?;
        let mut data = self.load()?;
        let p = Self::obj_mut(&mut data, platform);
        p.entry("keys".to_string()).or_insert_with(|| json!({}));
        p.entry("accounts".to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(user.to_string(), Value::String(ct));
        self.save(&data)
    }

    pub fn get_account(&self, platform: &str, user: &str) -> Result<Option<String>> {
        let key = self.aes_key()?;
        let data = self.load()?;
        self.decrypt_at(&data, &[platform, "accounts", user], &key)
    }

    // ── _servers / _clis 命名空间 ─────────────────────────
    //
    // 这两组同样只有 file 后端有（与 account/key 同理）。它们是**顶层特殊 key**
    // （`_servers` / `_clis`），不是某个 platform 下的桶 —— 结构必须和 Python 版
    // 逐字一致，否则老库读不出来：
    //   _servers[hostname] = { ip, root-password, cost?, provider? }   每个值单独加密
    //   _clis[cli_name][profile] = <密文>
    // 注意 `root-password` 带连字符，不是下划线（Python 版就这么写的，别顺手改）。

    pub fn set_server(
        &self,
        hostname: &str,
        ip: &str,
        root_password: &str,
        cost: &str,
        provider: &str,
    ) -> Result<()> {
        let _lock = self.lock()?;
        let key = self.aes_key()?;
        let mut entry = Map::new();
        entry.insert("ip".into(), Value::String(encrypt_joined(ip, &key)?));
        entry.insert(
            "root-password".into(),
            Value::String(encrypt_joined(root_password, &key)?),
        );
        // 空字段不写：写了空密文，get 时会解出空串，看着像"存过但丢了"
        if !cost.is_empty() {
            entry.insert("cost".into(), Value::String(encrypt_joined(cost, &key)?));
        }
        if !provider.is_empty() {
            entry.insert(
                "provider".into(),
                Value::String(encrypt_joined(provider, &key)?),
            );
        }
        let mut data = self.load()?;
        Self::obj_mut(&mut data, "_servers").insert(hostname.to_string(), Value::Object(entry));
        self.save(&data)
    }

    /// field 为 None 时返回全部字段（已解密）；指定 field 只返回那一项。
    pub fn get_server(
        &self,
        hostname: &str,
        field: Option<&str>,
    ) -> Result<Option<Vec<(String, String)>>> {
        let key = self.aes_key()?;
        let data = self.load()?;
        let Some(entry) = data.get("_servers").and_then(|v| v.get(hostname)) else {
            return Ok(None);
        };
        let Some(obj) = entry.as_object() else {
            return Ok(None);
        };
        let mut out = Vec::new();
        for (k, v) in obj {
            if let Some(f) = field {
                if k != f {
                    continue;
                }
            }
            if let Some(ct) = v.as_str() {
                out.push((k.clone(), decrypt_joined(ct, &key)?));
            }
        }
        if out.is_empty() {
            return Ok(None);
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Some(out))
    }

    pub fn list_servers(&self) -> Result<Vec<String>> {
        let mut v: Vec<String> = self
            .load()?
            .get("_servers")
            .and_then(|s| s.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        v.sort();
        Ok(v)
    }

    pub fn delete_server(&self, hostname: &str) -> Result<bool> {
        let _lock = self.lock()?;
        let mut data = self.load()?;
        let removed = Self::obj_mut(&mut data, "_servers")
            .remove(hostname)
            .is_some();
        if removed {
            self.save(&data)?;
        }
        Ok(removed)
    }

    pub fn set_cli_token(&self, cli_name: &str, profile: &str, token: &str) -> Result<()> {
        let _lock = self.lock()?;
        let ct = encrypt_joined(token, &self.aes_key()?)?;
        let mut data = self.load()?;
        Self::obj_mut(&mut data, "_clis")
            .entry(cli_name.to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(profile.to_string(), Value::String(ct));
        self.save(&data)
    }

    pub fn get_cli_token(&self, cli_name: &str, profile: &str) -> Result<Option<String>> {
        let key = self.aes_key()?;
        let data = self.load()?;
        self.decrypt_at(&data, &["_clis", cli_name, profile], &key)
    }

    /// cli_name 为 None 时列出所有 CLI 名；给了就列它的 profile。
    pub fn list_clis(&self, cli_name: Option<&str>) -> Result<Vec<String>> {
        let data = self.load()?;
        let Some(clis) = data.get("_clis").and_then(|v| v.as_object()) else {
            return Ok(vec![]);
        };
        let mut v: Vec<String> = match cli_name {
            None => clis.keys().cloned().collect(),
            Some(n) => clis
                .get(n)
                .and_then(|v| v.as_object())
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default(),
        };
        v.sort();
        Ok(v)
    }

    pub fn delete_cli_token(&self, cli_name: &str, profile: &str) -> Result<bool> {
        let _lock = self.lock()?;
        let mut data = self.load()?;
        let removed = Self::obj_mut(&mut data, "_clis")
            .get_mut(cli_name)
            .and_then(|v| v.as_object_mut())
            .and_then(|o| o.remove(profile))
            .is_some();
        if removed {
            self.save(&data)?;
        }
        Ok(removed)
    }

    pub fn set_key(&self, platform: &str, name: &str, value: &str) -> Result<()> {
        let _lock = self.lock()?;
        let ct = encrypt_joined(value, &self.aes_key()?)?;
        let mut data = self.load()?;
        let p = Self::obj_mut(&mut data, platform);
        p.entry("accounts".to_string()).or_insert_with(|| json!({}));
        p.entry("keys".to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(name.to_string(), Value::String(ct));
        self.save(&data)
    }

    pub fn get_key(&self, platform: &str, name: &str) -> Result<Option<String>> {
        let key = self.aes_key()?;
        let data = self.load()?;
        self.decrypt_at(&data, &[platform, "keys", name], &key)
    }

    /// bucket 传 "accounts" 或 "keys"
    pub fn list_bucket(&self, platform: &str, bucket: &str) -> Result<Vec<String>> {
        Ok(self
            .load()?
            .get(platform)
            .and_then(|v| v.get(bucket))
            .and_then(|v| v.as_object())
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default())
    }

    pub fn delete_from_bucket(&self, platform: &str, bucket: &str, name: &str) -> Result<bool> {
        let _lock = self.lock()?;
        let mut data = self.load()?;
        let hit = data
            .get_mut(platform)
            .and_then(|v| v.get_mut(bucket))
            .and_then(|v| v.as_object_mut())
            .map(|m| m.remove(name).is_some())
            .unwrap_or(false);
        if hit {
            self.save(&data)?;
        }
        Ok(hit)
    }

    /// 平台 → (accounts, keys)。跳过 _servers / _clis 这些系统保留键。
    #[allow(clippy::type_complexity)]
    pub fn platforms(
        &self,
    ) -> Result<std::collections::BTreeMap<String, (Vec<String>, Vec<String>)>> {
        let data = self.load()?;
        let mut out = std::collections::BTreeMap::new();
        if let Some(root) = data.as_object() {
            for (platform, v) in root {
                if platform.starts_with('_') {
                    continue;
                }
                let take = |b: &str| {
                    v.get(b)
                        .and_then(|x| x.as_object())
                        .map(|m| m.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default()
                };
                out.insert(platform.clone(), (take("accounts"), take("keys")));
            }
        }
        Ok(out)
    }

    pub fn exists(&self) -> bool {
        Path::new(&self.secrets_file()).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path());
        s.init_master_key().unwrap();
        (d, s)
    }

    #[test]
    fn secret_roundtrip_and_delete() {
        let (_d, s) = store();
        s.set_secret("secret://github/token", "ghp_x").unwrap();
        assert_eq!(
            s.get_secret("secret://github/token").unwrap().unwrap(),
            "ghp_x"
        );
        assert!(s.delete_secret("secret://github/token").unwrap());
        assert!(s.get_secret("secret://github/token").unwrap().is_none());
        assert!(
            !s.delete_secret("secret://github/token").unwrap(),
            "删不存在的要返回 false"
        );
    }

    /// cli/server 走各自的命名空间，别落进普通平台的 keys 里 ——
    /// CLAUDE.md 里 secret://cli/<cli>/<profile> 这种三段寻址依赖它
    #[test]
    fn cli_and_server_namespaces() {
        let (_d, s) = store();
        s.set_secret("secret://cli/gh/main", "gho_1").unwrap();
        s.set_secret("secret://server/vex/root-password", "pw")
            .unwrap();
        assert_eq!(
            s.get_secret("secret://cli/gh/main").unwrap().unwrap(),
            "gho_1"
        );
        assert_eq!(
            s.get_secret("secret://server/vex/root-password")
                .unwrap()
                .unwrap(),
            "pw"
        );
        // 不带字段时返回整台机器的 json
        let all = s.get_secret("secret://server/vex").unwrap().unwrap();
        assert!(all.contains("root-password"), "应返回整机 json：{all}");
    }

    /// 读-改-写必须保住没建模的字段。对密钥库来说丢字段就是丢密钥。
    #[test]
    fn unknown_fields_survive_write() {
        let (d, s) = store();
        let f = d.path().join("secrets.json");
        fs::write(
            &f,
            r#"{"legacy/flat":{"ciphertext":"x","kind":"K"},"future_thing":{"a":1}}"#,
        )
        .unwrap();
        s.set_secret("secret://github/token", "v").unwrap();
        let after: Value = serde_json::from_str(&fs::read_to_string(&f).unwrap()).unwrap();
        assert!(after.get("future_thing").is_some(), "未知字段被写掉了");
        assert!(after.get("legacy/flat").is_some(), "旧扁平格式被写掉了");
    }

    #[test]
    fn old_flat_format_readable() {
        let (d, s) = store();
        let key = s.aes_key().unwrap();
        let ct = encrypt_joined("old-value", &key).unwrap();
        fs::write(
            d.path().join("secrets.json"),
            serde_json::to_string(&json!({ "legacy/thing": { "ciphertext": ct } })).unwrap(),
        )
        .unwrap();
        assert_eq!(
            s.get_secret("secret://legacy/thing").unwrap().unwrap(),
            "old-value"
        );
    }

    #[test]
    fn account_and_key_buckets() {
        let (_d, s) = store();
        s.set_account("github", "webkubor", "pw1").unwrap();
        s.set_key("github", "pat", "ghp_2").unwrap();
        assert_eq!(s.get_account("github", "webkubor").unwrap().unwrap(), "pw1");
        assert_eq!(s.get_key("github", "pat").unwrap().unwrap(), "ghp_2");
        assert_eq!(
            s.list_bucket("github", "accounts").unwrap(),
            vec!["webkubor"]
        );
        assert_eq!(s.list_bucket("github", "keys").unwrap(), vec!["pat"]);
        let p = s.platforms().unwrap();
        assert_eq!(p["github"].0, vec!["webkubor"]);
        assert!(s.delete_from_bucket("github", "keys", "pat").unwrap());
        assert!(s.list_bucket("github", "keys").unwrap().is_empty());
    }

    #[test]
    fn master_key_never_overwritten() {
        let (_d, s) = store();
        let first = s.master_key().unwrap();
        let again = s.init_master_key().unwrap();
        assert_eq!(
            first, again,
            "init 第二次绝不能换 key —— 换了存量密文全部解不开"
        );
    }

    /// 坏 JSON 必须拒绝写入 —— 不能拿空对象覆盖整库。
    ///
    /// 这条原本是"宽容地当空库继续用"，代价是 rebase 冲突标记进入
    /// secrets.json 时，set 会**覆盖整库**（Codex 2026-09 review 抓到的 bug）。
    /// 现在把宽容只留给"文件不存在"这一种合法空库场景。
    #[test]
    fn broken_json_blocks_writes() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path());
        s.init_master_key().unwrap();
        fs::write(d.path().join("secrets.json"), "{不是 json").unwrap();
        // 读：报错
        let err = s.list_secrets().unwrap_err().to_string();
        assert!(err.contains("不是合法 JSON"), "应提示 JSON 损坏：{err}");
        // 写：拒绝，库文件不被破坏
        let before = fs::read_to_string(d.path().join("secrets.json")).unwrap();
        let set_err = s.set_secret("secret://a/b", "v").unwrap_err().to_string();
        assert!(
            set_err.contains("不是合法 JSON"),
            "set 应继承 load 的错误：{set_err}"
        );
        let after = fs::read_to_string(d.path().join("secrets.json")).unwrap();
        assert_eq!(before, after, "坏库被 set 覆盖了！");
    }

    /// git 冲突标记（rebase / merge 未解）必须拒绝写入。
    ///
    /// 这是 Codex 2026-09 review 的核心：<<<<<<< / >>>>>>> 进入 secrets.json 后，
    /// `set` 不能拿空对象覆盖，否则 rebase 一冲突整库就丢。
    ///
    /// 关键细节：冲突标记**必须在 JSON 解析失败的上下文里**才检测（load 里
    /// `Err(_) if has_git_conflict_markers(...)`）—— 见下一个测试的反例。
    #[test]
    fn git_conflict_markers_block_writes() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path());
        s.init_master_key().unwrap();
        // 真实 git 冲突：JSON 不合法 + 行首含 <<<<<<< 等标记
        let conflicted = r#"{
  "github/token": "ghp_one"
<<<<<<< HEAD
  "github/token": "ghp_other"
=======
  "github/token": "ghp_three"
>>>>>>> branch
}"#;
        fs::write(d.path().join("secrets.json"), conflicted).unwrap();

        let err = s.list_secrets().unwrap_err().to_string();
        assert!(err.contains("git 冲突标记"), "应提示冲突：{err}");

        let before = fs::read_to_string(d.path().join("secrets.json")).unwrap();
        let set_err = s
            .set_secret("secret://github/a", "v")
            .unwrap_err()
            .to_string();
        assert!(
            set_err.contains("git 冲突标记"),
            "set 应继承 load 的错误：{set_err}"
        );
        let after = fs::read_to_string(d.path().join("secrets.json")).unwrap();
        assert_eq!(before, after, "冲突标记被 set 覆盖了！");
    }

    /// 反例：合法 JSON 里包含 "=======" 等字符串不能被误判为冲突。
    ///
    /// Codex v2 review 抓到的回归：原 `has_git_conflict_markers` 用全文件 contains，
    /// 如果**先检测**冲突标记再尝试 JSON 解析，合法库含 "=======" 字符串就会被
    /// 锁死（即使 JSON 完全合法）。现在 load() 是先 JSON 解析，失败后才细化检测，
    /// 这里守住这条边界。
    #[test]
    fn legal_strings_with_marker_chars_remain_readable() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path());
        s.init_master_key().unwrap();
        // 合法 JSON，键名 / 值里都包含冲突标记字符
        let legal = r#"{
  "github": {
    "keys": {
      "note": "we use ======= for headers",
      "syntax": "<<<<<<< is not a real marker here"
    }
  }
}"#;
        fs::write(d.path().join("secrets.json"), legal).unwrap();
        // list_secrets 不报错，正常列出 keys 桶里的两条
        let metas = s.list_secrets().unwrap();
        assert_eq!(
            metas.len(),
            2,
            "合法 JSON 应列出两条（github/note + github/syntax），实际：{metas:?}"
        );
        // has_git_conflict_markers 单测本身的边界：
        // contains 在合法 JSON 上确实会返回 true，但 load() 不会到这一步
        assert!(
            Store::has_git_conflict_markers(legal),
            "contains 本身确实返回 true —— 但 load() 不会到这一步"
        );
    }

    /// init 场景：文件不存在时 load 应返回空对象，set 应正常写入。
    ///
    /// 这条证明"宽容地当空库"还保留给 init 用 —— 跟坏库严格拒绝是两个不同分支。
    #[test]
    fn missing_file_still_works_for_init() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path());
        s.init_master_key().unwrap();
        // 文件还不存在
        assert!(!d.path().join("secrets.json").exists());
        // set 应创建文件
        s.set_secret("secret://github/init", "v").unwrap();
        assert!(d.path().join("secrets.json").exists());
        assert_eq!(s.get_secret("secret://github/init").unwrap().unwrap(), "v");
    }

    /// 单元测试 has_git_conflict_markers 自身 —— 几个变体都该识别。
    #[test]
    fn conflict_marker_detection() {
        assert!(Store::has_git_conflict_markers("<<<<<<<"));
        assert!(Store::has_git_conflict_markers(">>>>>>>"));
        assert!(Store::has_git_conflict_markers("|||||||")); // diff3 风格
        assert!(Store::has_git_conflict_markers(r#"{"a": "<<<<<<< HEAD"}"#));
        assert!(!Store::has_git_conflict_markers(r#"{"a": "x"}"#));
        assert!(!Store::has_git_conflict_markers(
            "密文里包含 < 和 > 字符也不该误判"
        ));
    }

    #[test]
    fn meta_json_enrichment_and_annotate() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path());
        s.init_master_key().unwrap();
        s.save_meta(&serde_json::Map::new()).unwrap();

        // 写入初始密钥（meta.json 存在时自动记录 last4 / sha256 / length）
        s.set_secret("secret://github/pat", "ghp_12345678").unwrap();

        // 创建 meta.json 并 annotate
        s.annotate(
            "secret://github/pat",
            Some("my-account"),
            Some("Personal Token"),
            Some("my-org"),
            Some("repo,read:org"),
            Some("local"),
        )
        .unwrap();

        // list_secrets 应该富集这些字段
        let list = s.list_secrets().unwrap();
        let hit = list
            .iter()
            .find(|x| x.name == "pat" && x.platform == "github")
            .unwrap();
        assert_eq!(hit.account, "my-account");
        assert_eq!(hit.kind, "Personal Token");
        assert_eq!(hit.org, "my-org");
        assert_eq!(hit.scopes, "repo,read:org");
        assert_eq!(hit.visibility, "local");
        assert_eq!(hit.last4, "5678");

        // 删除密钥应同步清理 meta.json
        s.delete_secret("secret://github/pat").unwrap();
        let meta = s.load_meta().unwrap();
        assert!(!meta.contains_key("github/pat"));
    }

    #[test]
    fn store_lock_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(dir.path());
        let lock1 = s.lock().unwrap();

        // 另一个尝试应该失败或者无法立即获取
        let lock_path = dir.path().join(".lock");
        let f2 = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        // try_lock_exclusive should fail because lock1 holds it
        assert!(f2.try_lock_exclusive().is_err());
        drop(lock1);
        // now lock can be acquired
        assert!(f2.try_lock_exclusive().is_ok());
    }
}
