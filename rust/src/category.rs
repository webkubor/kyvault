//! 规范化分类与平台预设系统。
//!
//! 解决密钥命名不统一、分类混乱的痛点。定义标准的大类、常见平台、推荐用途命名规范，
//! 以及统一的 `secret://<platform>/<name>` 生成逻辑。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PlatformPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub default_name: &'static str,
    pub suggested_names: &'static [&'static str],
    pub placeholder: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub default_kind: &'static str,
    pub platforms: &'static [PlatformPreset],
}

pub const CATEGORIES: &[CategoryPreset] = &[
    CategoryPreset {
        id: "ai",
        name: "大模型 / AI API",
        icon: "🤖",
        default_kind: "API Key",
        platforms: &[
            PlatformPreset {
                id: "deepseek",
                name: "DeepSeek",
                icon: "🔵",
                default_name: "main",
                suggested_names: &["main", "backup", "v3", "coder"],
                placeholder: "sk-...",
            },
            PlatformPreset {
                id: "openai",
                name: "OpenAI",
                icon: "🟢",
                default_name: "main",
                suggested_names: &["main", "backup", "project-pat"],
                placeholder: "sk-...",
            },
            PlatformPreset {
                id: "anthropic",
                name: "Anthropic (Claude)",
                icon: "🟠",
                default_name: "main",
                suggested_names: &["main", "claude-code", "backup"],
                placeholder: "sk-ant-...",
            },
            PlatformPreset {
                id: "gemini",
                name: "Google Gemini",
                icon: "💎",
                default_name: "main",
                suggested_names: &["main", "backup"],
                placeholder: "AIzaSy...",
            },
            PlatformPreset {
                id: "zhipu",
                name: "智谱 AI (GLM)",
                icon: "🟣",
                default_name: "main",
                suggested_names: &["main", "backup"],
                placeholder: "...",
            },
            PlatformPreset {
                id: "moonshot",
                name: "Moonshot (Kimi)",
                icon: "🌙",
                default_name: "main",
                suggested_names: &["main", "backup"],
                placeholder: "sk-...",
            },
            PlatformPreset {
                id: "qwen",
                name: "通义千问 (DashScope)",
                icon: "☁️",
                default_name: "main",
                suggested_names: &["main", "backup"],
                placeholder: "sk-...",
            },
            PlatformPreset {
                id: "doubao",
                name: "字节豆包 (Ark)",
                icon: "🫘",
                default_name: "main",
                suggested_names: &["main", "coding-plan"],
                placeholder: "...",
            },
            PlatformPreset {
                id: "minimax",
                name: "MiniMax",
                icon: "🔷",
                default_name: "main",
                suggested_names: &["main", "backup"],
                placeholder: "...",
            },
        ],
    },
    CategoryPreset {
        id: "code",
        name: "代码与开发平台",
        icon: "💻",
        default_kind: "Token",
        platforms: &[
            PlatformPreset {
                id: "github",
                name: "GitHub",
                icon: "🐙",
                default_name: "pat",
                suggested_names: &["pat", "main", "ci-token", "personal"],
                placeholder: "ghp_...",
            },
            PlatformPreset {
                id: "gitlab",
                name: "GitLab",
                icon: "🦊",
                default_name: "token",
                suggested_names: &["token", "deploy-token", "pat"],
                placeholder: "glpat-...",
            },
            PlatformPreset {
                id: "vercel",
                name: "Vercel",
                icon: "▲",
                default_name: "token",
                suggested_names: &["token", "deploy"],
                placeholder: "...",
            },
            PlatformPreset {
                id: "docker",
                name: "Docker Hub",
                icon: "🐳",
                default_name: "pat",
                suggested_names: &["pat", "pull-token"],
                placeholder: "dckr_pat_...",
            },
            PlatformPreset {
                id: "npm",
                name: "NPM Registry",
                icon: "📦",
                default_name: "token",
                suggested_names: &["token", "publish-token"],
                placeholder: "npm_...",
            },
        ],
    },
    CategoryPreset {
        id: "cloud",
        name: "云与基础设施",
        icon: "🌐",
        default_kind: "API Key",
        platforms: &[
            PlatformPreset {
                id: "cloudflare",
                name: "Cloudflare",
                icon: "🧡",
                default_name: "api-token",
                suggested_names: &["api-token", "global-key", "d1-token"],
                placeholder: "...",
            },
            PlatformPreset {
                id: "supabase",
                name: "Supabase",
                icon: "⚡",
                default_name: "anon-key",
                suggested_names: &["anon-key", "service-role-key", "jwt-secret"],
                placeholder: "eyJhbGciOi...",
            },
            PlatformPreset {
                id: "aliyun",
                name: "阿里云 (Aliyun)",
                icon: "☁️",
                default_name: "access-key",
                suggested_names: &["access-key", "secret-key"],
                placeholder: "LTAI...",
            },
            PlatformPreset {
                id: "tencentcloud",
                name: "腾讯云 (TencentCloud)",
                icon: "🐧",
                default_name: "secret-key",
                suggested_names: &["secret-key", "secret-id"],
                placeholder: "AKID...",
            },
            PlatformPreset {
                id: "bitiful",
                name: "缤纷云 (Bitiful S4)",
                icon: "🌈",
                default_name: "access-key",
                suggested_names: &["access-key", "secret-key"],
                placeholder: "...",
            },
        ],
    },
    CategoryPreset {
        id: "server",
        name: "服务器与运维资产",
        icon: "🖥️",
        default_kind: "Server",
        platforms: &[PlatformPreset {
            id: "server",
            name: "自定义主机",
            icon: "🖥️",
            default_name: "root-password",
            suggested_names: &["root-password", "ip", "port", "ssh-key"],
            placeholder: "root 密码或服务器 IP",
        }],
    },
    CategoryPreset {
        id: "account",
        name: "网站与系统账号",
        icon: "👤",
        default_kind: "Account",
        platforms: &[
            PlatformPreset {
                id: "google",
                name: "Google 账号",
                icon: "🔍",
                default_name: "password",
                suggested_names: &["password", "backup-code"],
                placeholder: "密码或备用验证码",
            },
            PlatformPreset {
                id: "apple",
                name: "Apple ID",
                icon: "🍎",
                default_name: "password",
                suggested_names: &["password", "app-specific-password"],
                placeholder: "Apple 密码",
            },
            PlatformPreset {
                id: "feishu",
                name: "飞书 / Lark",
                icon: "🐦",
                default_name: "tenant-token",
                suggested_names: &["tenant-token", "app-secret"],
                placeholder: "t-...",
            },
        ],
    },
];

/// 格式化标准的 secret:// URI
pub fn format_uri(platform: &str, name: &str) -> String {
    let p = platform.trim().to_lowercase();
    let n = name.trim().to_lowercase().replace(' ', "-");
    format!("secret://{p}/{n}")
}
