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
broker-devices.json
broker-audit.jsonl

# ── 运行时与本地产物 ──
.lock
*.tmp
*.bak
secrets.json.bak
# 审计日志有读事件，默认不入仓（团队共享时设 KY_AUDIT_SYNC=1 并手工删掉这行）
audit.log
";

/// 本地仓库用的固定身份。
///
/// 为什么要固定而不是用全局 git config：这套仓库由 `ky init` 在别人的机器上、
/// 在别人没配 user.name 的情况下建。没身份时第一次 commit 直接失败，症状是
/// 「init 说好了但没有历史」—— 仓库在、提交不在。固定身份只作用在这个库上
/// （`git config` 不带 --global），不会污染用户的全局配置。
const LOCAL_USER_NAME: &str = "kyvault";
const LOCAL_USER_EMAIL: &str = "kyvault@localhost";

/// 提交用的署名：优先真实操作者，退回到固定名（保证 commit 不会因为没身份失败）。
fn whoami_name() -> String {
    let who = crate::audit::actor(None);
    if who.is_empty() || who == "unknown" {
        LOCAL_USER_NAME.to_string()
    } else {
        who
    }
}

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
        // 身份只落在这个库里；先 check-config 避免覆盖用户已有的设置。
        // 名字取真实操作者（KY_ACTOR → $USER），而不是固定写死 "kyvault"：
        // 团队仓的 git log 是「谁改的」唯一可信来源，作者恒为 kyvault 等于自废武功。
        for (k, v) in [
            ("user.name", whoami_name()),
            ("user.email", LOCAL_USER_EMAIL.to_string()),
        ] {
            let has = Command::new("git")
                .args(["-C", &self.root.to_string_lossy(), "config", "--get", k])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !has {
                self.git(&["config", k, v.as_str()])?;
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
        let mut patterns = vec![
            "master.key",
            "master.key.enc",
            "auth.json",
            "*.key",
            "broker-devices.json",
            "broker-audit.jsonl",
        ];
        // 审计日志默认只留本地：里面有读事件，推到团队仓就变成监视。
        // 需要团队共享审计时设 KY_AUDIT_SYNC=1，这里就不再排除它。
        if !matches!(
            std::env::var("KY_AUDIT_SYNC").unwrap_or_default().trim(),
            "1" | "on" | "true" | "yes"
        ) {
            patterns.push("audit.log");
        }
        for pat in patterns {
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
            &format!(
                "chore(vault): 密钥库初始提交（仅密文）· by {}",
                whoami_name()
            ),
        ])?;
        Ok(true)
    }

    /// 记远端地址。**不建远端仓库** —— 建不建由用户有没有给 token 决定。
    pub fn set_remote(&self, url: &str) -> Result<()> {
        self.git(&["remote", "remove", "origin"]).ok();
        self.git(&["remote", "add", "origin", url])?;
        Ok(())
    }

    /// 建远端仓库并推首推。没有凭证就明确返回「没建」，不假装成功。
    ///
    /// 远端已存在时按「已存在」处理（clone 来的库就是这种）。
    ///
    /// 凭证来源由 `glab::resolve` 决定：`KY_GITLAB_TOKEN` → glab 配置文件 →
    /// `glab auth status`。用户本机已经 `glab auth login` 过就不该再抄一遍 PAT。
    pub fn push_initial(&self, token: Option<&str>) -> Result<PushOutcome> {
        let url = self
            .origin()
            .ok_or_else(|| anyhow!("没有远端地址，先 set_remote 或用 --remote 指定"))?;

        // 本地裸仓库 / 文件路径：不查凭证、不建项目，直接 push
        if is_local_remote(&url) {
            self.push_with_credential(None)?;
            return Ok(PushOutcome::Pushed);
        }

        let (host, path) = parse_remote_path(&url)?;

        // token 为 None 时让 glab 自己找；找到就用，找不到就只 push（可能是 ssh 远端）
        let cred = match token.filter(|t| !t.is_empty()) {
            Some(t) => Some(crate::glab::Credential {
                host: host.clone(),
                token: t.to_string(),
                source: "explicit",
            }),
            None => crate::glab::resolve(Some(&host)),
        };

        if let Some(c) = cred.as_ref() {
            let created = crate::glab::ensure_project(&host, &path, Some(&c.token))?;
            if created {
                println!(
                    "  🦊 已在 {host} 上创建私有仓库 {path}（凭证来源：{}）",
                    c.source
                );
            }
        } else {
            println!("  ℹ️  没找到 {host} 的凭证（KY_GITLAB_TOKEN / glab auth），不代你建仓库。");
        }

        self.push_with_credential(cred.as_ref())?;
        Ok(PushOutcome::Pushed)
    }

    /// HTTPS 远端 + 有 token 时，用一次性 credential helper 把 token 交给 git。
    ///
    /// token 走环境变量而不是 `-c credential.helper='...password=xxx'`：后者会
    /// 出现在 `ps aux` 里，等于把 PAT 贴在进程列表上。glab 配置过 credential
    /// helper 的话本来就不需要这段，这里只是兜住「只有 token、没有 helper」的人。
    fn push_with_credential(&self, cred: Option<&crate::glab::Credential>) -> Result<()> {
        let url = self.origin().unwrap_or_default();
        let is_http = url.starts_with("http://") || url.starts_with("https://");
        match (is_http, cred) {
            (true, Some(c)) if c.source != "glab-config" || !self.git_helper_configured() => {
                let helper =
                    "!f() { echo \"username=oauth2\"; echo \"password=$KYVAULT_GIT_TOKEN\"; }; f";
                let out = Command::new("git")
                    .args([
                        "-c",
                        "credential.helper=",
                        "-c",
                        &format!("credential.helper={helper}"),
                        "push",
                        "-u",
                        "origin",
                        "HEAD",
                    ])
                    .env("KYVAULT_GIT_TOKEN", &c.token)
                    .current_dir(&self.root)
                    .output()
                    .context("执行 git push 失败")?;
                if !out.status.success() {
                    return Err(anyhow!(
                        "git push 失败：{}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    ));
                }
                Ok(())
            }
            _ => {
                self.git(&["push", "-u", "origin", "HEAD"])?;
                Ok(())
            }
        }
    }

    /// glab 是否已经给 git 装好了 credential helper（装了就别再插手）。
    fn git_helper_configured(&self) -> bool {
        Command::new("git")
            .args(["config", "--get", "credential.helper"])
            .current_dir(&self.root)
            .output()
            .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false)
    }
}

/// 远端地址是不是本地路径/本地裸仓库。
///
/// 存在的理由不只是「支持离线测试」：如果 URL 是 `/tmp/bare.git`，
/// `parse_remote_path` 会切出一个空 host，而空 host 一旦被拿去问凭证，
/// `glab::resolve(None)` 会回落到**用户真实的 GitLab token** 并尝试建仓库 ——
/// 在本地路径上触发远端写操作，这是最不该发生的意外。
pub fn is_local_remote(url: &str) -> bool {
    let u = url.trim();
    u.starts_with("file://")
        || u.starts_with('/')
        || u.starts_with("./")
        || u.starts_with("../")
        || u.starts_with('~')
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
