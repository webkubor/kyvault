//! `kyvault doctor` —— 自检。
//!
//! 与 Python 版（kyvault/doctor.py）同样的四段，但**运行环境那段的内容变了**：
//! Python 版检查「Python 版本 >= 3.10」「cryptography 装没装」，那是它自己的依赖；
//! Rust 版是静态二进制，这两项不存在 —— 换成真正还会坏的东西：后端选到了哪个、
//! D1 需要的环境变量齐不齐（这是本机最常见的故障：cs 注入了就走 D1，裸跑就落回
//! file，两套数据不通，人却以为是同一个库）。
//!
//! 输出保持「一行一项 + OK/FAIL」的形状，方便 grep 和肉眼扫。

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::store::Store;

/// 文件权限描述。密钥文件必须 0600 —— 组和其他人可读就等于没加密。
fn perm_desc(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(m) => {
                let mode = m.permissions().mode() & 0o777;
                if mode == 0o600 {
                    format!("权限 {mode:o} (OK)")
                } else {
                    format!(
                        "权限 {mode:o} (FAIL, 应为 600 —— chmod 600 {})",
                        path.display()
                    )
                }
            }
            Err(e) => format!("读不到权限：{e}"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        "非 unix 平台，跳过权限检查".to_string()
    }
}

fn exists_mark(p: &Path) -> &'static str {
    if p.exists() {
        "已连接 (OK)"
    } else {
        "未连接"
    }
}

pub fn run() -> Result<()> {
    println!("🏥 kyvault 自检");
    println!("{}", "-".repeat(52));

    // ① 运行环境
    println!("\n⚙️  1. 运行环境：");
    println!(
        "  - kyvault 版本：{} (Rust 静态二进制)",
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "  - 平台：{}/{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let backend = std::env::var("KYVAULT_BACKEND").unwrap_or_else(|_| "file".into());
    println!("  - 当前后端：{backend}（KYVAULT_BACKEND，缺省 file）");
    if backend == "d1" {
        // D1 三件套缺任何一个都会在下一次读写时才炸，这里提前点出来。
        // token 那项必须和 d1.rs 读的口径完全一致：它先看 CLOUDFLARE_API_TOKEN、
        // 再退回 CF_API_TOKEN。doctor 只查后者的话，cs kyvault（注入的正是前者）
        // 下会报 FAIL 而实际能用 —— 报假警的自检比没有自检更糟，人会学着无视它。
        for v in ["KYVAULT_D1_ACCOUNT_ID", "KYVAULT_D1_DATABASE_ID"] {
            let ok = std::env::var(v).map(|s| !s.is_empty()).unwrap_or(false);
            println!(
                "  - {v}：{}",
                if ok {
                    "已注入 (OK)"
                } else {
                    "缺失 (FAIL)"
                }
            );
        }
        match ["CLOUDFLARE_API_TOKEN", "CF_API_TOKEN"]
            .iter()
            .find(|v| std::env::var(v).map(|s| !s.is_empty()).unwrap_or(false))
        {
            Some(v) => println!("  - CF token：已注入 (OK，来自 {v})"),
            None => println!("  - CF token：缺失 (FAIL，设 CLOUDFLARE_API_TOKEN 或 CF_API_TOKEN)"),
        }
    } else {
        println!("  - 提示：cs kyvault 会自动注入 d1 三件套；裸跑走 file，两套数据不通");
    }

    // ② 文件与权限（只对 file 后端有意义）
    let store = Store::default_location()?;
    println!("\n📁  2. 本地文件与权限：");
    println!(
        "  - 数据目录：{} ({})",
        store.root().display(),
        if store.root().exists() {
            "存在"
        } else {
            "未初始化"
        }
    );
    let mk = store.master_key_path();
    if mk.exists() {
        println!("  - master.key：存在, {}", perm_desc(&mk));
    } else {
        println!("  - master.key：缺失 (FAIL, 执行 kyvault init)");
    }
    let sf = store.secrets_path();
    if sf.exists() {
        println!("  - secrets.json：存在, {}", perm_desc(&sf));
    } else {
        println!("  - secrets.json：不存在（空密钥库）");
    }

    // ③ 解密自检 —— 能不能真的解开，而不是文件在不在
    println!("\n🔑  3. 本地密钥库解密：");
    if !mk.exists() {
        println!("  - 跳过：未初始化");
    } else {
        match store.master_key() {
            Ok(_) => {
                let metas = store.list_secrets();
                println!("  - 解密状态：成功 (OK)");
                println!("  - 条目数：{}", metas.len());
            }
            Err(e) => println!("  - 解密状态：失败 (FAIL) —— {e}"),
        }
    }

    // ④ AI 助手对接
    println!("\n🤖  4. AI 编码助手对接：");
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
        ("Claude Code 规则", home.join(".clauderules")),
        ("Codex 规则 (AGENTS.md)", home.join(".codex/AGENTS.md")),
        (
            "Hermes 技能包",
            home.join(".hermes/profiles/free/skills/kyvault-ops/SKILL.md"),
        ),
    ];
    for (label, p) in &items {
        println!("  - {label}：{}", exists_mark(p));
    }

    println!("\n💡 有「未连接」的跑 kyvault connect 一键修复");
    println!("{}", "-".repeat(52));
    Ok(())
}
