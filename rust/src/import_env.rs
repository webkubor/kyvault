//! `kyvault import` —— 从 .env 批量导入密钥。
//!
//! 与 Python 版（kyvault/import_env.py）同一套猜测规则，逐条对齐 —— 猜错了会把密钥
//! 存到别的 platform 名下，之后 `get` 找不到，人会以为导入失败又导一遍。
//!
//! 三个刻意保留的行为：
//!   · **跳过空值**：`.env` 里 `FOO=` 这种占位行导进去只会污染库
//!   · **dry-run 只打印映射**，不写库也不建别名 —— 批量操作前先看一眼落点
//!   · **别名默认建**（env 名小写），因为导入的动机通常就是"以后用别名注入"

use anyhow::Result;

/// 解析 .env。只认 `KEY=VALUE`，跳过空行与 `#` 注释，去掉成对的引号。
pub fn parse_env(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        // 变量名必须是 [A-Za-z_][A-Za-z0-9_]* —— 否则那行不是环境变量赋值
        let mut cs = k.chars();
        let head_ok = cs
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false);
        if !head_ok || !cs.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let mut v = v.trim().to_string();
        if v.len() >= 2 {
            let b = v.as_bytes();
            if (b[0] == b'"' || b[0] == b'\'') && b[0] == b[v.len() - 1] {
                v = v[1..v.len() - 1].to_string();
            }
        }
        out.push((k.to_string(), v));
    }
    out
}

/// 从变量名猜平台。顺序有意义：先具体后宽泛（`cf_` 在 `cloudflare` 之后同组判断，
/// 而 google 归 gcp —— 与 Python 版一致，改顺序会改变既有导入结果）。
pub fn guess_platform(env_key: &str) -> &'static str {
    let k = env_key.to_lowercase();
    let has = |s: &str| k.contains(s);
    if has("github") {
        return "github";
    }
    if has("gitlab") {
        return "gitlab";
    }
    if has("cloudflare") || has("cf_") {
        return "cloudflare";
    }
    if has("deepseek") {
        return "deepseek";
    }
    if has("zhipu") || has("chatglm") {
        return "zhipu";
    }
    if has("volcengine") || has("ark") {
        return "volcengine";
    }
    if has("feishu") || has("lark") {
        return "feishu";
    }
    if has("jenkins") {
        return "jenkins";
    }
    if has("aws") {
        return "aws";
    }
    if has("gcp") || has("google") {
        return "gcp";
    }
    if has("azure") {
        return "azure";
    }
    if has("openai") {
        return "openai";
    }
    if has("anthropic") || has("claude") {
        return "anthropic";
    }
    "custom"
}

/// 从变量名猜类型。token/pat → Token，key → API Key，secret → Secret，password → Password。
pub fn guess_kind(env_key: &str) -> &'static str {
    let k = env_key.to_lowercase();
    if k.contains("token") || k.contains("pat") {
        return "Token";
    }
    if k.contains("key") {
        return "API Key";
    }
    if k.contains("secret") {
        return "Secret";
    }
    if k.contains("password") || k.contains("passwd") {
        return "Password";
    }
    "API Key"
}

/// 变量名 → 密钥名：削掉平台前缀，非字母数字压成单个 `-`。
pub fn make_name(env_key: &str, platform: &str) -> String {
    let lower = env_key.to_lowercase();
    let mut name = lower.as_str();
    for pfx in [format!("{platform}_"), "app_".into(), "service_".into()] {
        if let Some(rest) = name.strip_prefix(pfx.as_str()) {
            name = rest;
            break;
        }
    }
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "default".into()
    } else {
        out
    }
}

pub struct Planned {
    pub env_key: String,
    pub value: String,
    pub r#ref: String,
    pub kind: &'static str,
}

/// 只做规划，不碰任何存储 —— 这样 dry-run 与真导入走的是同一份逻辑，
/// 不会出现「预览说会存到 A、实际存到 B」。
pub fn plan(text: &str, prefix: &str) -> Vec<Planned> {
    parse_env(text)
        .into_iter()
        .filter(|(k, v)| !v.is_empty() && (prefix.is_empty() || k.starts_with(prefix)))
        .map(|(k, v)| {
            let platform = guess_platform(&k);
            let name = make_name(&k, platform);
            Planned {
                r#ref: format!("secret://{platform}/{name}"),
                kind: guess_kind(&k),
                env_key: k,
                value: v,
            }
        })
        .collect()
}

pub fn read_file(path: &str) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("读不到 {path}：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_comments_and_strips_quotes() {
        let t = "# c\n\nA=1\nB=\"two\"\nC='three'\n不是变量=x\nD_E1=v\nBAD-KEY=y\n";
        let got = parse_env(t);
        let keys: Vec<&str> = got.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["A", "B", "C", "D_E1"], "非法变量名必须被跳过");
        assert_eq!(got[1].1, "two", "双引号要去掉");
        assert_eq!(got[2].1, "three", "单引号要去掉");
    }

    #[test]
    fn guesses_match_python_rules() {
        assert_eq!(guess_platform("GITHUB_TOKEN"), "github");
        assert_eq!(guess_platform("CF_API_TOKEN"), "cloudflare");
        assert_eq!(guess_platform("GOOGLE_APPLICATION_CREDENTIALS"), "gcp");
        assert_eq!(guess_platform("WHATEVER"), "custom");
        assert_eq!(guess_kind("GITHUB_TOKEN"), "Token");
        assert_eq!(guess_kind("OPENAI_API_KEY"), "API Key");
        assert_eq!(guess_kind("DB_PASSWORD"), "Password");
    }

    #[test]
    fn name_drops_platform_prefix() {
        assert_eq!(make_name("GITHUB_TOKEN", "github"), "token");
        assert_eq!(make_name("APP_SECRET_ID", "custom"), "secret-id");
        // 削完为空要兜底，否则会生成 secret://x/ 这种读不回来的 ref
        assert_eq!(make_name("GITHUB_", "github"), "default");
    }

    #[test]
    fn plan_skips_empty_and_honors_prefix() {
        let t = "GITHUB_TOKEN=t\nEMPTY=\nOPENAI_API_KEY=k\n";
        assert_eq!(plan(t, "").len(), 2, "空值必须跳过");
        let p = plan(t, "GITHUB_");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].r#ref, "secret://github/token");
    }
}
