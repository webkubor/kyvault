//! LLM / 服务平台的 key 校验表 —— `kyvault check` 与 `kyvault providers` 的数据源。
//!
//! 为什么这张表值得单独一个模块：CLAUDE.md 的密钥红线里有一条「失效就覆写，不要保留
//! 旧值」——要做到这点，前提是能廉价地问出「这把 key 现在还活着吗」。逐个平台去翻文档
//! 找验证端点是最耗人的一步，所以把它固化下来。
//!
//! 与 Python 版（kyvault/validator.py）保持同一份表和同一套判据，逐条对齐：
//! 401 → 认证失败、403 → 权限不足、429 → 频繁但 key 有效（**这条是 valid=true**，
//! 把限流报成失效会让人误删一把好 key）。

use serde_json::Value;

/// key 放在哪：多数平台走 header，Gemini 拼在 URL 上。
#[derive(Clone, Copy, PartialEq)]
pub enum KeyPos {
    Header,
    Url,
}

/// 余额接口的返回形状。Python 版用 lambda 逐个解析，Rust 用枚举把形状显式列出来 ——
/// 免得下次加平台时不知道该往哪塞。
#[derive(Clone, Copy)]
pub enum BalanceShape {
    /// balance_infos[0].total_balance（元）
    DeepSeek,
    /// data.available_balance（元）
    Moonshot,
    /// balance（分）
    ZhipuCents,
    /// balance（原样）
    Plain,
    /// data.balance（万分之一元）
    DoubaoTenThousandth,
}

pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub header: &'static str,
    pub prefix: &'static str,
    pub env_key: &'static str,
    pub logo: &'static str,
    pub key_pos: KeyPos,
    pub post_body: Option<&'static str>,
    pub extra_headers: &'static [(&'static str, &'static str)],
    pub balance_url: Option<&'static str>,
    pub balance_shape: Option<BalanceShape>,
}

const ANTHROPIC_BODY: &str =
    r#"{"model":"claude-3-haiku-20240307","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#;

/// 一个只填了通用字段的模板，下面用 `..OPENAI_LIKE` 展开 —— 22 个平台里 17 个只差
/// id/name/url/env_key/logo，全写一遍纯属噪音。
const OPENAI_LIKE: Provider = Provider {
    id: "",
    name: "",
    url: "",
    header: "Authorization",
    prefix: "Bearer ",
    env_key: "",
    logo: "",
    key_pos: KeyPos::Header,
    post_body: None,
    extra_headers: &[],
    balance_url: None,
    balance_shape: None,
};

pub static PROVIDERS: &[Provider] = &[
    Provider { id: "openai", name: "OpenAI", url: "https://api.openai.com/v1/models", env_key: "OPENAI_API_KEY", logo: "🟢", ..OPENAI_LIKE },
    Provider { id: "deepseek", name: "DeepSeek", url: "https://api.deepseek.com/v1/models", env_key: "DEEPSEEK_API_KEY", logo: "🔵",
        balance_url: Some("https://api.deepseek.com/user/balance"), balance_shape: Some(BalanceShape::DeepSeek), ..OPENAI_LIKE },
    Provider { id: "zhipu", name: "智谱 AI", url: "https://open.bigmodel.cn/api/paas/v4/models", env_key: "ZHIPU_API_KEY", logo: "🟣",
        balance_url: Some("https://open.bigmodel.cn/api/paas/v4/user/balance"), balance_shape: Some(BalanceShape::ZhipuCents), ..OPENAI_LIKE },
    Provider { id: "moonshot", name: "Moonshot (Kimi)", url: "https://api.moonshot.cn/v1/models", env_key: "MOONSHOT_API_KEY", logo: "🌙",
        balance_url: Some("https://api.moonshot.cn/v1/users/me/balance"), balance_shape: Some(BalanceShape::Moonshot), ..OPENAI_LIKE },
    // Anthropic 没有 /models 列表接口，只能拿最便宜的模型发一条 max_tokens=1 的请求探活
    Provider { id: "anthropic", name: "Anthropic (Claude)", url: "https://api.anthropic.com/v1/messages",
        header: "x-api-key", prefix: "", env_key: "ANTHROPIC_API_KEY", logo: "🟠",
        post_body: Some(ANTHROPIC_BODY),
        extra_headers: &[("anthropic-version", "2023-06-01"), ("content-type", "application/json")], ..OPENAI_LIKE },
    // Gemini 的 key 拼在 URL 上，不走 header
    Provider { id: "gemini", name: "Google Gemini", url: "https://generativelanguage.googleapis.com/v1beta/models?key=",
        header: "", prefix: "", env_key: "GEMINI_API_KEY", logo: "💎", key_pos: KeyPos::Url, ..OPENAI_LIKE },
    Provider { id: "qwen", name: "通义千问", url: "https://dashscope.aliyuncs.com/compatible-mode/v1/models", env_key: "DASHSCOPE_API_KEY", logo: "☁️",
        balance_url: Some("https://dashscope.aliyuncs.com/api/v1/services/billing/usage"), balance_shape: Some(BalanceShape::Plain), ..OPENAI_LIKE },
    Provider { id: "aliyun", name: "阿里云百炼", url: "https://dashscope.aliyuncs.com/compatible-mode/v1/models", env_key: "DASHSCOPE_API_KEY", logo: "☁️",
        balance_url: Some("https://dashscope.aliyuncs.com/api/v1/services/billing/usage"), balance_shape: Some(BalanceShape::Plain), ..OPENAI_LIKE },
    Provider { id: "minimax", name: "MiniMax", url: "https://api.minimaxi.chat/v1/models", env_key: "MINIMAX_API_KEY", logo: "🔷", ..OPENAI_LIKE },
    Provider { id: "doubao", name: "字节豆包", url: "https://ark.cn-beijing.volces.com/api/v3/models", env_key: "DOUBAO_API_KEY", logo: "🫘",
        balance_url: Some("https://ark.cn-beijing.volces.com/api/v3/account/balance"), balance_shape: Some(BalanceShape::DoubaoTenThousandth), ..OPENAI_LIKE },
    Provider { id: "groq", name: "Groq", url: "https://api.groq.com/openai/v1/models", env_key: "GROQ_API_KEY", logo: "⚡", ..OPENAI_LIKE },
    Provider { id: "together", name: "Together AI", url: "https://api.together.xyz/v1/models", env_key: "TOGETHER_API_KEY", logo: "🤝", ..OPENAI_LIKE },
    Provider { id: "openrouter", name: "OpenRouter", url: "https://openrouter.ai/api/v1/models", env_key: "OPENROUTER_API_KEY", logo: "🔀", ..OPENAI_LIKE },
    Provider { id: "fireworks", name: "Fireworks AI", url: "https://api.fireworks.ai/inference/v1/models", env_key: "FIREWORKS_API_KEY", logo: "🔥", ..OPENAI_LIKE },
    Provider { id: "siliconflow", name: "SiliconFlow", url: "https://api.siliconflow.cn/v1/models", env_key: "SILICONFLOW_API_KEY", logo: "🧊", ..OPENAI_LIKE },
    Provider { id: "baichuan", name: "百川", url: "https://api.baichuan-ai.com/v1/models", env_key: "BAICHUAN_API_KEY", logo: "🌊", ..OPENAI_LIKE },
    Provider { id: "spark", name: "讯飞星火", url: "https://spark-api-open.xf-yun.com/v1/models", env_key: "SPARK_API_KEY", logo: "✨", ..OPENAI_LIKE },
    Provider { id: "github", name: "GitHub", url: "https://api.github.com/user", prefix: "token ", env_key: "GITHUB_TOKEN", logo: "🐙", ..OPENAI_LIKE },
    Provider { id: "cloudflare", name: "Cloudflare", url: "https://api.cloudflare.com/client/v4/user/tokens/verify", env_key: "CLOUDFLARE_API_TOKEN", logo: "🧡", ..OPENAI_LIKE },
    Provider { id: "gitlab", name: "GitLab", url: "https://gitlab.com/api/v4/user", header: "PRIVATE-TOKEN", prefix: "", env_key: "GITLAB_TOKEN", logo: "🦊", ..OPENAI_LIKE },
    Provider { id: "feishu", name: "Feishu", url: "https://open.feishu.cn/open-apis/bot/v3/info", env_key: "FEISHU_TOKEN", logo: "🐦", ..OPENAI_LIKE },
];

pub fn get_provider(name: &str) -> Option<&'static Provider> {
    let n = name.to_lowercase();
    PROVIDERS.iter().find(|p| p.id == n)
}

pub struct CheckResult {
    pub valid: bool,
    pub message: String,
    pub models: Vec<String>,
    pub balance: Option<String>,
}

/// 校验一把 key 是否还活着。
///
/// 判据与 Python 版逐条对齐，其中 **429 判 valid=true** 是刻意的：限流说明 key 被服务端
/// 认了，只是这一刻打得太密。把它报成失效，人就会去覆写一把本来好的 key。
pub fn validate_key(provider_name: &str, api_key: &str) -> CheckResult {
    let Some(p) = get_provider(provider_name) else {
        return CheckResult {
            valid: false,
            message: format!("不支持的平台：{provider_name}（kyvault providers 看清单）"),
            models: vec![],
            balance: None,
        };
    };

    let url = if p.key_pos == KeyPos::Url {
        format!("{}{}", p.url, api_key)
    } else {
        p.url.to_string()
    };

    let mut req = if p.post_body.is_some() {
        ureq::post(&url)
    } else {
        ureq::get(&url)
    };
    if !p.header.is_empty() {
        req = req.set(p.header, &format!("{}{}", p.prefix, api_key));
    }
    for (k, v) in p.extra_headers {
        req = req.set(k, v);
    }

    let resp = match p.post_body {
        Some(b) => req.send_string(b),
        None => req.call(),
    };

    match resp {
        Ok(r) => {
            let body: Value = r.into_json().unwrap_or(Value::Null);
            // 飞书即使 token 失效也返 HTTP 200，得看 body 里的 code
            if p.id == "feishu" && body.get("code").and_then(Value::as_i64) != Some(0) {
                let msg = body
                    .get("msg")
                    .and_then(Value::as_str)
                    .unwrap_or("Token 无效或已过期");
                return CheckResult {
                    valid: false,
                    message: format!("{} {} 验证失败：{}", p.logo, p.name, msg),
                    models: vec![],
                    balance: None,
                };
            }
            let models = body
                .get("data")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .take(5)
                        .filter_map(|m| m.get("id").and_then(Value::as_str).map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            CheckResult {
                valid: true,
                message: format!("{} {} 验证通过", p.logo, p.name),
                models,
                balance: check_balance(p, api_key),
            }
        }
        Err(ureq::Error::Status(code, _)) => {
            let msg = match code {
                401 => "认证失败：API Key 无效或已过期".to_string(),
                403 => "权限不足：API Key 无访问权限".to_string(),
                // 限流不等于失效 —— 报错也要说清 key 是好的
                429 => "请求过于频繁（但 Key 有效）".to_string(),
                c => format!("HTTP {c}"),
            };
            CheckResult {
                valid: code == 429,
                message: format!("{} {} {}", p.logo, p.name, msg),
                models: vec![],
                balance: None,
            }
        }
        Err(e) => CheckResult {
            valid: false,
            message: format!("网络错误：{e}"),
            models: vec![],
            balance: None,
        },
    }
}

/// 余额是尽力而为：查不到就不显示，绝不因此把 key 判成失效。
fn check_balance(p: &Provider, api_key: &str) -> Option<String> {
    let url = p.balance_url?;
    let shape = p.balance_shape?;
    let body: Value = ureq::get(url)
        .set("Authorization", &format!("Bearer {api_key}"))
        .call()
        .ok()?
        .into_json()
        .ok()?;
    let f = |v: Option<&Value>| v.and_then(Value::as_f64);
    match shape {
        BalanceShape::DeepSeek => {
            let v = f(body
                .get("balance_infos")?
                .as_array()?
                .first()?
                .get("total_balance"))
            .or_else(|| {
                // total_balance 有时是字符串
                body.get("balance_infos")?
                    .as_array()?
                    .first()?
                    .get("total_balance")?
                    .as_str()?
                    .parse()
                    .ok()
            })?;
            Some(format!("余额：¥{v}"))
        }
        BalanceShape::Moonshot => {
            let v = f(body.get("data")?.get("available_balance"))?;
            Some(format!("余额：¥{v:.4}"))
        }
        BalanceShape::ZhipuCents => {
            let v = f(body.get("balance"))?;
            Some(format!("余额：¥{:.4}", v / 100.0))
        }
        BalanceShape::Plain => {
            let v = body.get("balance")?;
            Some(format!("余额：{v}"))
        }
        BalanceShape::DoubaoTenThousandth => {
            let v = f(body.get("data")?.get("balance"))?;
            Some(format!("余额：¥{:.4}", v / 10000.0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_table_is_consistent() {
        assert_eq!(PROVIDERS.len(), 21, "平台数变了就同步更新 SKILL.md 的清单");
        for p in PROVIDERS {
            assert!(!p.id.is_empty(), "有平台没填 id");
            assert!(!p.name.is_empty(), "{} 没填 name", p.id);
            assert!(p.url.starts_with("https://"), "{} 的 url 必须是 https", p.id);
            assert!(!p.env_key.is_empty(), "{} 没填 env_key", p.id);
            // 余额 url 与解析形状必须成对出现，只填一个等于查了不会解
            assert_eq!(
                p.balance_url.is_some(),
                p.balance_shape.is_some(),
                "{} 的 balance_url 与 balance_shape 没成对",
                p.id
            );
            // key 不放 header 时必须拼 URL，否则这把 key 根本没被送出去
            if p.header.is_empty() {
                assert!(p.key_pos == KeyPos::Url, "{} 既不放 header 也不拼 URL", p.id);
            }
        }
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert!(get_provider("OpenAI").is_some());
        assert!(get_provider("deepseek").is_some());
        assert!(get_provider("nope").is_none());
    }

    #[test]
    fn unknown_provider_does_not_panic() {
        let r = validate_key("nope", "x");
        assert!(!r.valid);
        assert!(r.message.contains("不支持的平台"));
    }
}
