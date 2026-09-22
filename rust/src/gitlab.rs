//! GitLab 团队协作后端模块。
//!
//! 角色：file 存储后端的 Git 远端同步层。
//! 核心原则：
//! 1. **master.key 绝不入仓** —— 仅 secrets.json（密文）和 meta.json（元信息）入仓。
//! 2. **断网可用** —— 读取与修改始终作用于本地 store，网络同步是显式命令。
//! 3. **安全护栏** —— push 前强制检查 master.key 是否被 gitignore 排除且未被暂存。

use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Context, Result};

use crate::model::now_utc;
use crate::store::Store;

pub struct GitLabRepo<'a> {
    store: &'a Store,
}

impl<'a> GitLabRepo<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    fn root(&self) -> &Path {
        self.store.root()
    }

    /// 检查当前 store 目录是否是一个 Git 仓库
    pub fn is_git_repo(&self) -> bool {
        self.root().join(".git").exists()
    }

    fn run_git(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.root())
            .output()
            .with_context(|| format!("执行 git 命令失败: git {}", args.join(" ")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git {} 失败：{}", args.join(" "), stderr.trim()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// 安全检查：确认 master.key 绝对没有被 Git 跟踪或暂存
    fn assert_master_key_safe(&self) -> Result<()> {
        let tracked = self.run_git(&["ls-files", "master.key"])?;
        if !tracked.trim().is_empty() {
            return Err(anyhow!(
                "🚨 安全严重警告：master.key 已被 Git 暂存或跟踪！\n\
                 绝对不能将其 push 到远端仓库！\n\
                 请立即执行: git rm --cached master.key 并将其写入 .gitignore"
            ));
        }

        // 检查 master.key 是否被 .gitignore 忽略
        let status = Command::new("git")
            .args(["check-ignore", "-q", "master.key"])
            .current_dir(self.root())
            .status();
        if let Ok(st) = status {
            if !st.success() {
                // 未被忽略，警告并尝试自动修护 .gitignore
                let gitignore = self.root().join(".gitignore");
                let mut content = std::fs::read_to_string(&gitignore).unwrap_or_default();
                if !content.contains("master.key") {
                    if !content.is_empty() && !content.ends_with('\n') {
                        content.push('\n');
                    }
                    content.push_str("master.key\n.lock\n*.tmp\n");
                    std::fs::write(&gitignore, &content)?;
                }
            }
        }

        Ok(())
    }

    /// `kyvault gitlab status`
    pub fn status(&self) -> Result<()> {
        if !self.is_git_repo() {
            println!("📁 当前存储目录：{}", self.root().display());
            println!("⚠️  当前目录不是 Git 仓库。");
            println!("💡 若要连接 GitLab 团队密钥库，请运行：kyvault gitlab setup <repo-url>");
            return Ok(());
        }

        println!("🦊 GitLab 团队密钥库状态：");
        println!("  - 存储目录：{}", self.root().display());

        let remote = self
            .run_git(&["remote", "get-url", "origin"])
            .unwrap_or_else(|_| "(未设置 origin)".to_string());
        println!("  - 远端地址：{remote}");

        let branch = self
            .run_git(&["branch", "--show-current"])
            .unwrap_or_else(|_| "HEAD".to_string());
        println!("  - 当前分支：{branch}");

        // 检查与上游分歧
        let tracking = self.run_git(&["rev-list", "--left-right", "--count", "HEAD...@{u}"]);
        match tracking {
            Ok(counts) => {
                let parts: Vec<&str> = counts.split_whitespace().collect();
                if parts.len() == 2 {
                    let ahead = parts[0];
                    let behind = parts[1];
                    println!("  - 同步状态：领先远端 {ahead} 次提交，落后远端 {behind} 次提交");
                }
            }
            Err(_) => {
                println!("  - 同步状态：未设置远程跟踪分支");
            }
        }

        // 文件状态
        let st = self.run_git(&["status", "--porcelain"])?;
        if st.is_empty() {
            println!("  - 工作区：干净，无未同步的本地改动");
        } else {
            println!("  - 本地改动：");
            for line in st.lines() {
                println!("    {line}");
            }
        }

        // 安全检查
        self.assert_master_key_safe()?;
        println!("  - 密钥安全：master.key 未入仓并受 .gitignore 保护 (OK)");

        Ok(())
    }

    /// `kyvault gitlab pull`
    pub fn pull(&self) -> Result<()> {
        if !self.is_git_repo() {
            return Err(anyhow!("当前目录不是 Git 仓库，无法 pull"));
        }

        let _lock = self.store.lock()?;
        println!("📥 正在从 GitLab 远端拉取最新密文与元信息...");

        let branch = self
            .run_git(&["branch", "--show-current"])
            .unwrap_or_else(|_| "main".to_string());

        let output = Command::new("git")
            .args(["pull", "--rebase", "--autostash", "origin", &branch])
            .current_dir(self.root())
            .output()
            .context("执行 git pull 失败")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let combined = format!("{stdout}\n{stderr}");

            // 检查是否有冲突标记
            if combined.contains("conflict") || combined.contains("CONFLICT") {
                return Err(anyhow!(
                    "❌ git pull 出现冲突！\n\
                     可能与队友同时修改了密钥。请先执行 git rebase --abort 取消合并，\n\
                     或手动解决冲突后确保 secrets.json 为合法 JSON。"
                ));
            }
            return Err(anyhow!("git pull 失败：{}", stderr.trim()));
        }

        // 验证拉取后的 secrets.json 完整性
        if let Err(e) = self.store.load() {
            return Err(anyhow!("拉取后的密钥库损坏：{e}。请检查并恢复。"));
        }

        println!("✅ 拉取并更新成功。");
        Ok(())
    }

    /// `kyvault gitlab push`
    pub fn push(&self) -> Result<()> {
        if !self.is_git_repo() {
            return Err(anyhow!("当前目录不是 Git 仓库，无法 push"));
        }

        let _lock = self.store.lock()?;
        self.assert_master_key_safe()?;

        // Stage secrets.json, meta.json, .gitignore
        for file in ["secrets.json", "meta.json", ".gitignore"] {
            if self.root().join(file).exists() {
                self.run_git(&["add", file])?;
            }
        }

        let st = self.run_git(&["status", "--porcelain"])?;
        let has_staged = st
            .lines()
            .any(|l| l.starts_with('M') || l.starts_with('A') || l.starts_with('D'));

        let branch = self
            .run_git(&["branch", "--show-current"])
            .unwrap_or_else(|_| "main".to_string());

        if has_staged {
            let msg = format!("feat(secrets): sync from kyvault {}", now_utc());
            self.run_git(&["commit", "-m", &msg])?;
            println!("📝 已创建提交: {msg}");
        }

        println!("📤 正在推送到 GitLab 远端 (origin/{branch})...");
        self.run_git(&["push", "origin", &branch])?;
        println!("✅ 推送成功，团队密文已同步至 GitLab。");

        Ok(())
    }

    /// `kyvault gitlab sync`
    pub fn sync(&self) -> Result<()> {
        self.pull()?;
        self.push()?;
        Ok(())
    }

    /// `kyvault gitlab setup <repo-url>`
    pub fn setup(repo_url: &str, target_dir: Option<&Path>) -> Result<()> {
        let target = match target_dir {
            Some(d) => d.to_path_buf(),
            None => {
                let home = dirs::home_dir().ok_or_else(|| anyhow!("找不到 home 目录"))?;
                home.join(".config").join("kyvault").join("store")
            }
        };

        if target.join(".git").exists() {
            println!(
                "ℹ️  目录 {} 已是 Git 仓库，无需重复 setup。",
                target.display()
            );
            return Ok(());
        }

        println!("🚀 正在克隆 GitLab 仓库至 {}...", target.display());
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let output = Command::new("git")
            .args(["clone", repo_url, target.to_str().unwrap()])
            .output()
            .context("执行 git clone 失败")?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("克隆失败：{}", err.trim()));
        }

        // 确保 .gitignore 包含 master.key
        let gitignore = target.join(".gitignore");
        let mut gi_content = std::fs::read_to_string(&gitignore).unwrap_or_default();
        if !gi_content.contains("master.key") {
            if !gi_content.is_empty() && !gi_content.ends_with('\n') {
                gi_content.push('\n');
            }
            gi_content.push_str("master.key\n.lock\n*.tmp\n");
            std::fs::write(&gitignore, gi_content)?;
        }

        println!("✅ 仓库克隆成功！");
        let master_key = target.join("master.key");
        if !master_key.exists() {
            println!("\n🔑 关键安全步骤（master.key 不入仓）：");
            println!("  GitLab 仓库只包含密文，没有解密钥匙。");
            println!("  请通过带外信道（1Password / 飞书加密消息）将团队 master.key 拷贝到：");
            println!("    {}", master_key.display());
            println!("  并设置权限：chmod 600 {}", master_key.display());
        } else {
            println!("🔑 本地 master.key 已存在。");
        }

        Ok(())
    }
}
