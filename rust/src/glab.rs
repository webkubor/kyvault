//! 复用本机 GitLab CLI（`glab`）的登录态。
//!
//! 存在的理由很简单：用户本机已经 `glab auth login` 过了，凭什么还要再手抄一遍
//! PAT 给 kyvault？此前的实现只认 `KY_GITLAB_TOKEN`，没有就「不代你建仓库」——
//! 明明凭证就在旁边的配置文件里躺着。
//!
//! 三个来源，按可信度排序：
//! 1. `KY_GITLAB_TOKEN`（调用方处理，显式优先）
//! 2. `~/.config/glab-cli/config.yml` 里该 host 的 `token:`
//! 3. `glab auth status --show-token` 的 `Token:` 行
//!
//! ## 为什么不解析 YAML
//!
//! 为了一个 `hosts.<host>.token` 引入 serde_yaml 不值得（还会和 ureq 的版本
//! 约束打架）。glab 的配置文件结构是固定两层，按缩进扫一遍足够；扫不出来就
//! 走第 3 条兜底，不会因为解析失败就报废。
//!
//! ## token 不进 argv
//!
//! 调 GitLab API 时优先走 `glab api` —— token 由 glab 自己从配置读，不出现在
//! 进程参数里。没有 glab 才回落到 curl，且用 stdin 传 header（见 `curl_api`），
//! 同样不走 argv：`ps` 看得到别人的命令行。

use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Context, Result};

/// 一把能用的 GitLab 凭证，以及它是从哪儿翻出来的。
#[derive(Debug, Clone, PartialEq)]
pub struct Credential {
    pub host: String,
    pub token: String,
    /// `env` / `glab-config` / `glab-cli`，用于在输出里说清楚来源（可审计性）
    pub source: &'static str,
}

/// `glab` 是否可执行。只探版本，不发网络请求。
pub fn installed() -> bool {
    Command::new("glab")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// glab 配置文件位置：`GLAB_CONFIG_DIR`（glab 自己也认这个变量）优先。
pub fn config_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("GLAB_CONFIG_DIR") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("config.yml"));
        }
    }
    dirs::config_dir()
        .or_else(dirs::home_dir)
        .map(|d| d.join("glab-cli").join("config.yml"))
}

/// 从 `config.yml` 里取某个 host 的 token。
///
/// 目标结构（glab ≥1.30 起稳定）：
/// ```yaml
/// hosts:
///     gitlab.com:
///         token: glpat-xxx
///         user: someone
/// ```
/// `host` 传 `None` 时取第一个有 token 的 host —— 用户只有一个 GitLab 实例时
/// 这是最常见的情形，不该逼他再把域名写一遍。
pub fn token_from_config(host: Option<&str>) -> Option<(String, String)> {
    let path = config_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    parse_config(&text, host)
}

/// 纯函数版，便于单测。
pub fn parse_config(text: &str, want_host: Option<&str>) -> Option<(String, String)> {
    let mut in_hosts = false;
    let mut hosts_indent: Option<usize> = None;
    let mut current: Option<(String, usize)> = None;
    let mut first_hit: Option<(String, String)> = None;

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let body = line.trim();

        if body == "hosts:" {
            in_hosts = true;
            hosts_indent = Some(indent);
            continue;
        }
        if !in_hosts {
            continue;
        }
        // 回到 hosts 的层级或更浅 → hosts 段结束
        if let Some(hi) = hosts_indent {
            if indent <= hi {
                break;
            }
        }
        if body.ends_with(':') && !body.contains(": ") {
            // 形如 `    gitlab.com:` —— host 头
            let name = body.trim_end_matches(':').trim().to_string();
            current = Some((name, indent));
            continue;
        }
        if let Some((tok, v)) = body.split_once(':') {
            if tok.trim() != "token" {
                continue;
            }
            let v = v.trim().trim_matches(['"', '\'']).to_string();
            if v.is_empty() {
                continue;
            }
            let Some((name, _)) = &current else { continue };
            match want_host {
                Some(w) if w != name => continue,
                _ => {
                    if want_host.is_some() {
                        return Some((name.clone(), v));
                    }
                    first_hit.get_or_insert((name.clone(), v));
                }
            }
        }
    }
    first_hit
}

/// `glab auth status --show-token` 的输出里抓 `Token:` 行。
///
/// 输出形如：
/// ```text
/// gitlab.com
///   ✓ Logged in to gitlab.com as someone (/path/config.yml)
///   ✓ Token: glpat-xxx
/// ```
/// host 以「行首非空格 + 含点」判定；token 归到最近的 host 下。
pub fn parse_status(text: &str, want_host: Option<&str>) -> Option<(String, String)> {
    let mut current: Option<String> = None;
    let mut first_hit: Option<(String, String)> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let is_header = !raw.starts_with(char::is_whitespace)
            && !line.contains(' ')
            && line.contains('.')
            && !line.contains(':');
        if is_header {
            current = Some(line.to_string());
            continue;
        }
        if let Some(rest) = line.split_once("Token:") {
            let _ = rest.0;
            let v = rest.1.trim();
            if v.is_empty() {
                continue;
            }
            let Some(host) = &current else { continue };
            match want_host {
                Some(w) if w != host => continue,
                _ => {
                    if want_host.is_some() {
                        return Some((host.clone(), v.to_string()));
                    }
                    first_hit.get_or_insert((host.clone(), v.to_string()));
                }
            }
        }
    }
    first_hit
}

fn token_from_cli(host: Option<&str>) -> Option<(String, String)> {
    if !installed() {
        return None;
    }
    let out = Command::new("glab")
        .args(["auth", "status", "--show-token"])
        .output()
        .ok()?;
    // 未登录时 glab 退出码非 0，但 stdout/stderr 仍可能有内容
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    parse_status(&text, host)
}

/// 按 env → 配置文件 → CLI 的顺序找一把凭证。
///
/// `host` 是提示，不是硬约束：远端地址是 `gitlab.com` 就直接命中；自建实例
/// 没写端口时按 hostname 匹配。
pub fn resolve(host: Option<&str>) -> Option<Credential> {
    let host = normalize_host(host);

    if let Ok(t) = std::env::var("KY_GITLAB_TOKEN") {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Some(Credential {
                host: host.clone().unwrap_or_else(|| "gitlab.com".into()),
                token: t,
                source: "env",
            });
        }
    }
    if let Some((h, t)) = token_from_config(host.as_deref()) {
        return Some(Credential {
            host: h,
            token: t,
            source: "glab-config",
        });
    }
    if let Some((h, t)) = token_from_cli(host.as_deref()) {
        return Some(Credential {
            host: h,
            token: t,
            source: "glab-cli",
        });
    }
    None
}

fn normalize_host(host: Option<&str>) -> Option<String> {
    let h = host?.trim();
    if h.is_empty() {
        return None;
    }
    let h = h
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("ssh://git@")
        .trim_start_matches("git@");
    // 去掉端口与路径：`gitlab.example.com:8443/api` → `gitlab.example.com`
    let h = h.split('/').next().unwrap_or(h);
    // 注意 `git@host:group/repo` 已被 split_once(':') 的调用方切开了；
    // 这里只可能是 `host:port`
    let h = match h.split_once(':') {
        Some((a, b)) if b.chars().all(|c| c.is_ascii_digit()) => a,
        _ => h,
    };
    Some(h.to_string())
}

/// 是否能用 `glab api` 当 API 通道（装了 + 该 host 有登录态）。
pub fn api_available(host: &str) -> bool {
    installed() && (token_from_config(Some(host)).is_some() || token_from_cli(Some(host)).is_some())
}

fn api_base(host: &str) -> String {
    if host == "gitlab.com" || host == "www.gitlab.com" {
        "https://gitlab.com/api/v4".to_string()
    } else {
        format!("https://{host}/api/v4")
    }
}

/// GitLab 上有没有这个项目。`full_path` 形如 `group/project`。
pub fn project_exists(host: &str, full_path: &str, token: Option<&str>) -> Result<bool> {
    let encoded = full_path.replace('/', "%2F");
    if let Some(c) = token.filter(|t| !t.is_empty()) {
        let code = curl(
            &[
                "-sS",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                &format!("{}/projects/{encoded}", api_base(host)),
            ],
            Some(c),
            &[],
        )?;
        return Ok(code.trim() == "200");
    }
    // 没显式 token 时走 glab api：token 不经过我们的 argv
    if api_available(host) {
        let out = Command::new("glab")
            .args(["api", "--hostname", host, &format!("projects/{encoded}")])
            .output()
            .context("调用 glab api 失败")?;
        let text = String::from_utf8_lossy(&out.stdout);
        if out.status.success() && text.contains("\"id\"") {
            return Ok(true);
        }
        if text.contains("404") || String::from_utf8_lossy(&out.stderr).contains("404") {
            return Ok(false);
        }
        // 权限不足等情况下不要谎报「不存在」—— 交给创建步骤去撞真实错误
        return Ok(false);
    }
    Err(anyhow!(
        "找不到 {} 的 GitLab 凭证：设 KY_GITLAB_TOKEN，或先 glab auth login",
        host
    ))
}

/// 确保项目存在，不存在就建（private）。返回 `true` 表示新建了。
///
/// 建仓是**显式动作**：调用方必须已经拿到凭证或用户明确要求。
pub fn ensure_project(host: &str, full_path: &str, token: Option<&str>) -> Result<bool> {
    if project_exists(host, full_path, token)? {
        return Ok(false);
    }
    let name = full_path.rsplit('/').next().unwrap_or(full_path);
    let namespace = full_path
        .rsplit_once('/')
        .map(|(ns, _)| ns.to_string())
        .unwrap_or_default();

    if let Some(c) = token.filter(|t| !t.is_empty()) {
        let mut form = vec![
            "--form".to_string(),
            format!("name={name}"),
            "--form".to_string(),
            format!("path={name}"),
            "--form".to_string(),
            "visibility=private".to_string(),
        ];
        if !namespace.is_empty() {
            form.push("--form".to_string());
            form.push(format!("namespace_path={namespace}"));
        }
        let mut args: Vec<&str> = vec!["-sS", "--request", "POST"];
        args.extend(form.iter().map(|s| s.as_str()));
        let api = format!("{}/projects", api_base(host));
        args.push(&api);
        let out = curl(&args, Some(c), &[])?;
        if out.contains("\"message\"") && !out.contains("\"id\"") {
            return Err(anyhow!("创建 GitLab 项目被拒：{out}"));
        }
        return Ok(true);
    }

    if api_available(host) {
        let mut cmd = Command::new("glab");
        cmd.args([
            "api",
            "--hostname",
            host,
            "--method",
            "POST",
            "projects",
            "--raw-field",
            &format!("name={name}"),
            "--raw-field",
            &format!("path={name}"),
            "--raw-field",
            "visibility=private",
        ]);
        if !namespace.is_empty() {
            cmd.args(["--raw-field", &format!("namespace_path={namespace}")]);
        }
        let out = cmd.output().context("调用 glab api 创建项目失败")?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if !out.status.success() {
            return Err(anyhow!("创建 GitLab 项目失败：{}", text.trim()));
        }
        return Ok(true);
    }

    Err(anyhow!(
        "找不到 {} 的 GitLab 凭证，没法建仓库：设 KY_GITLAB_TOKEN，或先 glab auth login",
        host
    ))
}

/// curl 的 token 走 stdin 配置，不进 argv。
///
/// `curl --config -` 从标准输入读配置，`header = "PRIVATE-TOKEN: …"` 因此不出现在
/// 命令行里 —— `ps aux` 看不到，history 也没有。
fn curl(args: &[&str], token: Option<&str>, extra: &[(&str, &str)]) -> Result<String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut cmd = Command::new("curl");
    cmd.args(args);
    for (k, v) in extra {
        cmd.args(["--header", &format!("{k}: {v}")]);
    }
    let mut child = if token.is_some() {
        cmd.arg("--config")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("启动 curl 失败（需要 curl）")?
    } else {
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("启动 curl 失败（需要 curl）")?
    };

    if let Some(t) = token {
        if let Some(mut stdin) = child.stdin.take() {
            // 引号里的内容由 curl 自己解析；token 里不会出现引号（PAT 是 base62）
            let _ = writeln!(stdin, "header = \"PRIVATE-TOKEN: {t}\"");
        }
    }

    let out = child.wait_with_output().context("等待 curl 结束失败")?;
    if !out.status.success() {
        return Err(anyhow!(
            "curl 失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\
# glab config
git_protocol: ssh
hosts:
    gitlab.com:
        token: glpat-aaa
        user: someone
    gitlab.example.com:
        token: glpat-bbb
        user: other
editor: vim
";

    #[test]
    fn parse_config_picks_requested_host() {
        assert_eq!(
            parse_config(CONFIG, Some("gitlab.com")),
            Some(("gitlab.com".into(), "glpat-aaa".into()))
        );
        assert_eq!(
            parse_config(CONFIG, Some("gitlab.example.com")),
            Some(("gitlab.example.com".into(), "glpat-bbb".into()))
        );
    }

    #[test]
    fn parse_config_falls_back_to_first_host() {
        let got = parse_config(CONFIG, None).unwrap();
        assert_eq!(got.0, "gitlab.com");
    }

    #[test]
    fn parse_config_ignores_hosts_segment_end() {
        // editor 在 hosts 段之后，不该被当成 host
        assert_eq!(parse_config(CONFIG, Some("editor")), None);
    }

    #[test]
    fn parse_config_handles_empty_and_quoted() {
        assert_eq!(parse_config("", None), None);
        assert_eq!(
            parse_config(
                "hosts:\n    a.com:\n        token: \"glpat-x\"\n",
                Some("a.com")
            ),
            Some(("a.com".into(), "glpat-x".into()))
        );
    }

    #[test]
    fn parse_status_reads_show_token_output() {
        let out = "\
gitlab.com
  ✓ Logged in to gitlab.com as someone (/home/user/.config/glab-cli/config.yml)
  ✓ Token: glpat-ccc
";
        assert_eq!(
            parse_status(out, Some("gitlab.com")),
            Some(("gitlab.com".into(), "glpat-ccc".into()))
        );
        assert_eq!(parse_status(out, Some("other.com")), None);
    }

    #[test]
    fn normalize_host_strips_scheme_port_path() {
        assert_eq!(
            normalize_host(Some("https://gitlab.com")),
            Some("gitlab.com".into())
        );
        assert_eq!(
            normalize_host(Some("gitlab.example.com:8443")),
            Some("gitlab.example.com".into())
        );
        assert_eq!(normalize_host(Some("")), None);
    }
}
