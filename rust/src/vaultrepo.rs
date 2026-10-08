// vaultrepo.rs —— 密钥库自己的 Git 仓库。
//
// owner 2026-10-05 定的语义：**每个密钥库默认有自己的 Git 仓库存自己的密文**，
// 用户可以给自己的 GitLab 地址作为上传仓库，也可以不给 —— 不给就纯本地，
// 只有版本历史，没有远端。
//
// 为什么要有这层：密文（secrets.json）本身适合进 Git —— AES-256-GCM 密文进了仓
// 也解不开，**但必须有一个自己的仓库**。共用一个仓意味着「A 的密文和 B 的密文在
// 同一个历史里」，任何一次 push 都会把别人的密文一起带走；而解开密文要的是
// master.key，那把钥匙永不入仓。一仓一库是这套安全模型的最小完整单元。
//
// 三条硬约束，任何改动都不能破坏：
//  1. master.key / master.key.enc / auth.json 一律 .gitignore。它们凑齐就能解锁，
//     「没被跟踪」不等于安全 —— 谁跑一次 `git add -A` 就进去了。
//  2. 建仓是**本地**动作，不需要网络、不需要 token。不给远端就是不给远端。
//  3. 建远端仓库是**可选**动作，且只在有 token 时做；没有 token 就只设 origin
//     并明确告诉用户「远端仓库不存在，我没建」，不能假装建好了。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

/// 库根目录里**永不进 Git** 的东西。
///
/// 2026-10-05 补的 v2.4.0 形态：原先只写 master.key 和 \*.key，而
/// master.key.enc 和 auth.json 这对组合两个都不匹配既有规则，一直处于未跟踪
/// 状态 —— 未跟踪不等于安全，一次 `git add -A` 就把它们一起推上去了。
pub const PROTECTED_GITIGNORE: &str = "\
# ── 解开密文的东西，一律不入仓 ──
#
# 密文入仓是安全的（AES-256-GCM），但密钥和密文放一起就等于明文。
master.key
master.key.enc
auth.json
*.key

# ── 运行时与本地产物 ──
.lock
*.tmp
*.bak
secrets.json.bak
";

/// 本地仓库用的固定身份。
///
/// 为什么要固定而不是用全局 git config：这套仓库由 `ky init` 在别人的机器上、
/// 在别人没配 user.name 的情况下建。没身份时第一次 commit 直接失败，症状是
/// 「init 说好了但没有历史」—— 仓库在、提交不在。固定身份只作用在这个库上
/// （`git config` 不带 --global），不会污染用户的全局配置。
const LOCAL_USER_NAME: &str = "kyvault";
const LOCAL_USER_EMAIL: &str = "kyvault@localhost";

pub struct VaultRepo {
    root: PathBuf,
}

impl VaultRepo {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .with_context(|| format!("执行 git {} 失败", args.join(" ")))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            // git init 在已存在的目录上会报 reinit，但那不是错误
            let msg = err.trim();
            if !(args.first() == Some(&"init") && msg.contains("reinitial")) {
                return Err(anyhow!("git {} 失败：{}", args.join(" "), msg));
            }
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn is_repo(&self) -> bool {
        self.root.join(".git").exists()
    }

    pub fn origin(&self) -> Option<String> {
        let out = Command::new("git")
            .args([
                "-C",
                &self.root.to_string_lossy(),
                "remote",
                "get-url",
                "origin",
            ])
            .output()
            .ok()?;
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            None
        }
    }

    /// 建本地仓库（幂等）。返回是否**新建**了仓库。
    ///
    /// 不碰网络：不给远端地址的用户也能拿到完整版本历史。
    pub fn ensure_local(&self) -> Result<bool> {
        if !self.root.exists() {
            fs::create_dir_all(&self.root)
                .with_context(|| format!("建库目录失败：{}", self.root.display()))?;
        }
        let created = !self.is_repo();
        if created {
            self.git(&["init", "-q"])?;
        }
        self.write_gitignore()?;
        // 身份只落在这个库里；先 check-config 避免覆盖用户已有的设置
        for (k, v) in [
            ("user.name", LOCAL_USER_NAME),
            ("user.email", LOCAL_USER_EMAIL),
        ] {
            let has = Command::new("git")
                .args(["-C", &self.root.to_string_lossy(), "config", "--get", k])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !has {
                self.git(&["config", k, v])?;
            }
        }
        Ok(created)
    }

    fn write_gitignore(&self) -> Result<()> {
        let p = self.root.join(".gitignore");
        let existing = fs::read_to_string(&p).unwrap_or_default();
        // 已有 .gitignore 不覆盖 —— 用户可能自己加过东西。但要确保受保护项在里面。
        let mut merged = existing.clone();
        let mut missing = Vec::new();
        for pat in ["master.key", "master.key.enc", "auth.json", "*.key"] {
            if !merged.lines().any(|l| l.trim() == pat) {
                missing.push(pat);
            }
        }
        if !missing.is_empty() {
            if !merged.is_empty() && !merged.ends_with('\n') {
                merged.push('\n');
            }
            merged.push_str("# kyvault 自动补的排除项（缺了就危险：一次 git add -A 就能推上去）\n");
            for pat in missing {
                merged.push_str(pat);
                merged.push('\n');
            }
            fs::write(&p, merged)
                .with_context(|| format!("写 .gitignore 失败：{}", p.display()))?;
        }
        Ok(())
    }

    /// 首次提交（库里已有密文时才有东西可提交；空库不硬造一个空提交）。
    pub fn commit_initial(&self) -> Result<bool> {
        self.git(&["add", "-A"])?;
        let staged = self
            .git(&["diff", "--cached", "--name-only"])?
            .lines()
            .count();
        if staged == 0 {
            return Ok(false);
        }
        self.git(&[
            "commit",
            "-q",
            "-m",
            "chore(vault): 密钥库初始提交（仅密文）",
        ])?;
        Ok(true)
    }

    /// 记远端地址。**不建远端仓库** —— 建不建由用户有没有给 token 决定。
    pub fn set_remote(&self, url: &str) -> Result<()> {
        self.git(&["remote", "remove", "origin"]).ok();
        self.git(&["remote", "add", "origin", url])?;
        Ok(())
    }

    /// 建远端仓库并推首推。没有 token 就明确返回「没建」，不假装成功。
    ///
    /// 远端已存在时按「已存在」处理（clone 来的库就是这种）。
    pub fn push_initial(&self, token: Option<&str>) -> Result<PushOutcome> {
        let url = self
            .origin()
            .ok_or_else(|| anyhow!("没有远端地址，先 set_remote 或用 --remote 指定"))?;
        if let Some(t) = token {
            if !t.is_empty() {
                match ensure_gitlab_project(&url, t)? {
                    ProjectState::Exists => {}
                    ProjectState::Created => {}
                }
            }
        }
        self.git(&["push", "-u", "origin", "HEAD"])?;
        Ok(PushOutcome::Pushed)
    }
}

#[derive(Debug, PartialEq)]
pub enum ProjectState {
    Exists,
    Created,
}

/// GitLab 上没有这个项目就建一个。只在明确给了 token 时调用。
fn ensure_gitlab_project(url: &str, token: &str) -> Result<ProjectState> {
    // 从 remote URL 推出 <host>/<group>/<project>
    let (host, path) = parse_remote_path(url)?;
    let api = if host.starts_with("gitlab.com") || host.starts_with("www.gitlab.com") {
        "https://gitlab.com/api/v4".to_string()
    } else {
        format!("https://{host}/api/v4")
    };
    let encoded = path.replace('/', "%2F");
    let exists = Command::new("curl")
        .args([
            "-sS",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "--header",
            &format!("PRIVATE-TOKEN: {token}"),
            &format!("{api}/projects/{encoded}"),
        ])
        .output()
        .context("探测 GitLab 项目失败（需要 curl）")?;
    let code = String::from_utf8_lossy(&exists.stdout).trim().to_string();
    if code == "200" {
        return Ok(ProjectState::Exists);
    }
    let out = Command::new("curl")
        .args([
            "-sS",
            "--request",
            "POST",
            "--header",
            &format!("PRIVATE-TOKEN: {token}"),
            "--form",
            &format!("name={}", project_name(&path)),
            "--form",
            &format!("path={}", project_name(&path)),
            &format!("{api}/projects"),
        ])
        .output()
        .context("在 GitLab 上创建项目失败（需要 curl）")?;
    if !out.status.success() {
        return Err(anyhow!(
            "创建 GitLab 项目失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(ProjectState::Created)
}

/// 去掉 URL 里的 userinfo 与 .git 后缀，取 <host>/<group>/<project>。
pub fn parse_remote_path(url: &str) -> Result<(String, String)> {
    let rest = url
        .trim()
        .trim_start_matches("git@")
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("ssh://git@");
    let (host, path) = rest
        .split_once(':')
        .or_else(|| rest.split_once('/'))
        .ok_or_else(|| anyhow!("认不出远端地址的 host/path：{url}"))?;
    let path = path
        .trim_start_matches('/')
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string();
    if path.is_empty() {
        return Err(anyhow!("远端地址里没有项目路径：{url}"));
    }
    Ok((host.to_string(), path))
}

fn project_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[derive(Debug, PartialEq)]
pub enum PushOutcome {
    Pushed,
}

/// 把 $HOME 下的绝对路径归一成 ~/… 形式。
///
/// 存在的理由不是好看：`auth.json` 里的 `key_path` 会被**带外复制到另一台机**，
/// 而那台机的 home 不同。记录绝对路径会让目标机展开成自己的 home 找不到文件、
/// unlock 静默跳过，最后报「没有匹配的 SSH 公钥」—— 2026-10-05 给南烛授权
/// 第一次就是这么断的。而 shell 里 `~` 默认会被 bash 展开后才传给程序，
/// 工具收到的一直是绝对路径，所以必须在工具这侧归一。
pub fn home_relative(p: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if let Ok(rel) = p.strip_prefix(&home) {
            let s = rel.to_string_lossy().replace('\\', "/");
            return format!("~/{s}");
        }
    }
    p.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_gitignore_covers_authguard_pair() {
        // v2.4.0 才有的 master.key.enc + auth.json，漏任一个就等于留后门
        assert!(PROTECTED_GITIGNORE.contains("master.key.enc"));
        assert!(PROTECTED_GITIGNORE.contains("auth.json"));
        assert!(PROTECTED_GITIGNORE.contains("master.key\n"));
    }

    #[test]
    fn parse_remote_url_variants() {
        let cases = [
            (
                "git@gitlab.com:webkubor/kyvault-store.git",
                "gitlab.com",
                "webkubor/kyvault-store",
            ),
            (
                "https://gitlab.com/webkubor/kyvault-store",
                "gitlab.com",
                "webkubor/kyvault-store",
            ),
            (
                "https://gitlab.com/webkubor/kv.git",
                "gitlab.com",
                "webkubor/kv",
            ),
            (
                "git@gitlab.ops.modelgo.com:team/vault.git",
                "gitlab.ops.modelgo.com",
                "team/vault",
            ),
        ];
        for (url, host, path) in cases {
            let (h, p) = parse_remote_path(url).expect(url);
            assert_eq!(h, host, "url={url}");
            assert_eq!(p, path, "url={url}");
        }
    }

    #[test]
    fn parse_remote_rejects_empty_path() {
        assert!(parse_remote_path("https://gitlab.com/").is_err());
    }

    #[test]
    fn home_relative_normalizes() {
        let home = std::env::var("HOME").expect("HOME");
        let p = PathBuf::from(&home).join(".ssh").join("kyvault_store.pub");
        assert_eq!(home_relative(&p), "~/.ssh/kyvault_store.pub");
    }

    #[test]
    fn home_relative_keeps_outside_paths() {
        let p = PathBuf::from("/etc/ssh/keys");
        assert_eq!(home_relative(&p), "/etc/ssh/keys");
    }
}
