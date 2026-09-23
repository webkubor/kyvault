//! `kyvault doctor` —— 自检（带 TUI 颜色输出）。

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::store::Store;
use crate::tui::*;

/// 文件权限检查，返回 (is_ok, description)
pub fn perm_check(path: &Path) -> (bool, String) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(m) => {
                let mode = m.permissions().mode() & 0o777;
                if mode == 0o600 {
                    (true, format!("{:o}", mode))
                } else {
                    (false, format!("{:o} (应为 600)", mode))
                }
            }
            Err(e) => (false, format!("读不到：{e}")),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        (true, "非 unix".to_string())
    }
}

/// 文件权限描述（兼容旧接口）
pub fn perm_desc(path: &Path) -> String {
    let (ok, desc) = perm_check(path);
    if ok {
        format!("权限 {} (OK)", desc)
    } else {
        format!("权限 {} (FAIL)", desc)
    }
}

pub fn run() -> Result<()> {
    box_top("🏥 kyvault doctor");
    println!();

    // ① 运行环境
    section("⚙️", "运行环境");
    let ver = env!("CARGO_PKG_VERSION");
    kv(
        "版本",
        &format!(
            "{} {}",
            bold_cyan(&format!("v{ver}")),
            dim("(Rust 静态二进制)")
        ),
    );
    kv(
        "平台",
        &format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
    );
    let backend = std::env::var("KYVAULT_BACKEND").unwrap_or_else(|_| "file".into());
    kv("后端", &backend);
    if backend == "d1" {
        for v in ["KYVAULT_D1_ACCOUNT_ID", "KYVAULT_D1_DATABASE_ID"] {
            let ok = std::env::var(v).map(|s| !s.is_empty()).unwrap_or(false);
            status_line(v, ok, if ok { "已注入" } else { "缺失" });
        }
        match ["CLOUDFLARE_API_TOKEN", "CF_API_TOKEN"]
            .iter()
            .find(|v| std::env::var(v).map(|s| !s.is_empty()).unwrap_or(false))
        {
            Some(v) => status_line("CF token", true, &format!("来自 {v}")),
            None => status_line("CF token", false, "缺失"),
        }
        warn_line("D1 处于过渡期，推荐 kyvault gitlab setup");
    } else {
        kv("模式", "本地存储 / GitLab 团队仓库");
    }

    // ② 文件与权限
    let store = Store::default_location()?;
    section("📁", "文件与权限");
    kv("数据目录", &format!("{}", store.root().display()));

    let is_git = store.root().join(".git").exists();
    if is_git {
        status_line("Git 托管", true, "GitLab 团队密钥库");
        let status = std::process::Command::new("git")
            .args(["check-ignore", "-q", "master.key"])
            .current_dir(store.root())
            .status();
        let safe = status.map(|s| s.success()).unwrap_or(false);
        status_line(
            ".gitignore",
            safe,
            if safe {
                "master.key 已排除"
            } else {
                "⚠ 未排除！"
            },
        );
    }

    let mk = store.master_key_path();
    let mk_enc = store.root().join("master.key.enc");
    let auth_json = store.root().join("auth.json");
    let guard_enabled = mk_enc.exists() && auth_json.exists();

    if guard_enabled {
        status_line("master.key", true, "Auth Guard 加密保护中");
        let (ok, desc) = perm_check(&mk_enc);
        status_line("master.key.enc", ok, &format!("权限 {desc}"));
        let (ok2, desc2) = perm_check(&auth_json);
        status_line("auth.json", ok2, &format!("权限 {desc2}"));
        if mk.exists() {
            warn_line("明文 master.key 仍存在，建议删除");
        }
    } else if mk.exists() {
        let (ok, desc) = perm_check(&mk);
        status_line("master.key", ok, &format!("明文, 权限 {desc}"));
    } else {
        status_line("master.key", false, "缺失 → kyvault init");
    }

    let sf = store.secrets_path();
    if sf.exists() {
        let (ok, desc) = perm_check(&sf);
        status_line("secrets.json", ok, &format!("权限 {desc}"));
    } else {
        status_line("secrets.json", false, "不存在（空密钥库）");
    }
    let mf = store.meta_file();
    if mf.exists() {
        let (ok, desc) = perm_check(&mf);
        status_line("meta.json", ok, &format!("权限 {desc}"));
    }

    // ③ 解密自检
    section("🔑", "密钥库解密");
    if !mk.exists() && !guard_enabled {
        info_line("  ", &dim("跳过：未初始化"));
    } else {
        match store.master_key() {
            Ok(_) => {
                let metas = store.list_secrets()?;
                status_line("解密", true, "成功");
                kv("条目数", &bold(&format!("{}", metas.len())));
            }
            Err(e) => status_line("解密", false, &format!("{e}")),
        }
    }

    // ④ Auth Guard
    section("🛡️", "Auth Guard 守卫");
    if guard_enabled {
        match crate::auth_guard::AuthGuard::load(store.root()) {
            Ok(guard) => {
                let ssh_count = guard
                    .methods()
                    .iter()
                    .filter(|m| m.method_type == "ssh_key")
                    .count();
                let has_totp = guard.methods().iter().any(|m| m.method_type == "totp");
                status_line("守卫", true, "已启用");
                kv("策略", &cyan(guard.policy()));
                let mut methods_str = format!("SSH Key × {ssh_count}");
                if has_totp {
                    methods_str.push_str(&format!("  {}  TOTP", dim("+")));
                }
                kv("绑定", &methods_str);
                if mk.exists() {
                    warn_line("明文 master.key 仍存在");
                } else {
                    status_line("明文残留", true, "已清除");
                }
            }
            Err(e) => status_line("守卫", false, &format!("读取失败：{e}")),
        }
    } else {
        status_line("守卫", false, "未启用");
        info_line("  ", &dim("kyvault auth setup 启用"));
    }

    // ⑤ AI 助手对接
    section("🤖", "AI 编码助手对接");
    let home = dirs::home_dir().unwrap_or_default();
    let items: [(&str, PathBuf); 5] = [
        (
            "Gemini/agy 规则",
            home.join(".gemini/config/rules/kyvault.md"),
        ),
        (
            "Gemini/agy 技能",
            home.join(".gemini/config/skills/kyvault-ops/SKILL.md"),
        ),
        ("Claude Code", home.join(".clauderules")),
        ("Codex", home.join(".codex/AGENTS.md")),
        (
            "Hermes",
            home.join(".hermes/profiles/free/skills/kyvault-ops/SKILL.md"),
        ),
    ];
    for (label, p) in &items {
        status_line(
            label,
            p.exists(),
            if p.exists() { "已连接" } else { "未连接" },
        );
    }

    println!();
    println!(
        "  {} {}",
        dim("💡"),
        dim("有「未连接」的跑 kyvault connect 一键修复")
    );
    println!();
    box_bottom();
    Ok(())
}
