//! `kyvault connect` —— 把密钥使用规则/技能包写进本地各家 AI 助手。
//!
//! 与 Python 版（kyvault/connect.py）同样的落点，但**写入方式改了**，因为那边有个
//! 会累积垃圾的 bug：写 `.clauderules` / Codex `AGENTS.md` 时追加的是 RULE_CONTENT，
//! 而幂等判据查的是字符串 "Multi-Account" —— 那个词只在 CURSOR_INSTRUCTIONS 里、
//! RULE_CONTENT 里没有，于是判据永远不成立，**每跑一次 connect 就再追加一份**。
//! 2026-09-10 实测本机 ~/.clauderules 里已经堆了 4 份一模一样的规则。
//!
//! 这里改成标记块：
//!   <!-- kyvault:begin --> … <!-- kyvault:end -->
//! 有块就整块替换（规则升级时内容自动跟着更新，这是追加做不到的），没块就追加。
//! 同时清掉「与当前内容完全一致的无标记旧副本」—— 精确整段匹配才删，改过版本的
//! 匹配不上就保留并提示，绝不去猜边界删用户自己写的东西。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

const SKILL_MD: &str = include_str!("../assets/SKILL.md");
const RULE_MD: &str = include_str!("../assets/RULE.md");
const CURSOR_MD: &str = include_str!("../assets/CURSOR.md");

const BEGIN: &str = "<!-- kyvault:begin -->";
const END: &str = "<!-- kyvault:end -->";

enum Action {
    Created,
    Updated,
    Unchanged,
    /// 清掉了 n 份无标记旧副本
    Deduped(usize),
}

/// 整块覆写一个文件（技能包/规则包这种整个文件都归我们的）。
fn write_whole(path: &Path, content: &str) -> Result<Action> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
    }
    if path.exists() && fs::read_to_string(path).unwrap_or_default() == content {
        return Ok(Action::Unchanged);
    }
    let existed = path.exists();
    fs::write(path, content)?;
    Ok(if existed {
        Action::Updated
    } else {
        Action::Created
    })
}

/// 往共享文件里插一块（.clauderules / AGENTS.md 这种还有别人内容的）。
///
/// 统一走一条路径：**先把标记块整个抠掉，剩下的是「块外文本」**，在块外做去重与
/// 旧版检测，最后把新块追到末尾。
///
/// 一开始写成了两条分支（有块就替换、没块才去重），结果是：已有标记块的文件
/// 根本走不到去重和旧版检测那段 —— 本机 ~/.clauderules 正是这种（块内是新版、
/// 块外躺着一份缺了第 4 条规则的旧版），跑多少次都不吭声。两条路径处理同一件事
/// 却只在其中一条上实现，是这类 bug 的常见形状。
fn upsert_block(path: &Path, content: &str) -> Result<Action> {
    let block = format!("{BEGIN}\n{}\n{END}\n", content.trim_end());
    let Ok(original) = fs::read_to_string(path) else {
        if let Some(d) = path.parent() {
            fs::create_dir_all(d)?;
        }
        fs::write(path, &block)?;
        return Ok(Action::Created);
    };

    // ① 抠掉标记块，得到块外文本
    let mut outside = original.clone();
    let mut had_block = false;
    if let (Some(i), Some(j)) = (outside.find(BEGIN), outside.find(END)) {
        if i < j {
            outside.replace_range(i..j + END.len(), "");
            had_block = true;
        }
    }

    // ② 块外清掉与当前内容完全一致的副本（那些一定是本工具早先追加的）
    let needle = content.trim();
    let mut removed = 0;
    while let Some(i) = outside.find(needle) {
        outside.replace_range(i..i + needle.len(), "");
        removed += 1;
        if removed > 20 {
            break; // 兜底：别在异常内容上转圈
        }
    }

    // ③ 块外若还留着本工具**旧版本**的内容，出声但不动它。
    //    精确整段匹配删不掉它（内容变过版本就对不上），保守不删是对的 —— 猜边界
    //    去删可能连用户自己写的一起削掉。但只保留不吭声同样不行：两份互相不一致
    //    的规则并存在同一个文件里，agent 读到哪份全看运气。
    //    判据用内容首行（标题行）：它在版本间稳定，而整段会变。
    let head = needle.lines().next().unwrap_or("").trim();
    let stale = !head.is_empty() && outside.contains(head);

    while outside.contains("\n\n\n") {
        outside = outside.replace("\n\n\n", "\n\n");
    }
    let mut text = outside.trim_end().to_string();
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&block);

    if stale {
        println!(
            "     ⚠ {} 块外还有一份旧版规则（内容和当前版本对不上，没敢自动删）",
            path.display()
        );
        println!("       搜 \"{head}\" 手动清掉 —— 否则同一份文件里两套规则并存");
    }
    if text == original {
        return Ok(Action::Unchanged);
    }
    fs::write(path, text)?;
    Ok(match (removed > 0, had_block) {
        (true, _) => Action::Deduped(removed),
        (false, true) => Action::Updated,
        (false, false) => Action::Updated,
    })
}

fn report(label: &str, path: &Path, a: Action) {
    let p = path.display();
    match a {
        Action::Created => println!("  ✓ {label}：已写入 {p}"),
        Action::Updated => println!("  ✓ {label}：已更新 {p}"),
        Action::Unchanged => println!("  · {label}：已是最新，未改动"),
        Action::Deduped(n) => {
            println!("  ✓ {label}：已更新 {p}（顺带清掉 {n} 份重复的旧副本）")
        }
    }
}

pub fn run() -> Result<()> {
    println!("🤖 连接本地 AI 编码助手…");
    let home = dirs::home_dir().unwrap_or_default();

    // ① Gemini / agy —— 只在它的 config 目录已存在时写，不替人凭空造目录
    let gem = home.join(".gemini/config");
    if gem.exists() {
        report(
            "Gemini/agy 技能",
            &gem.join("skills/kyvault-ops/SKILL.md"),
            write_whole(&gem.join("skills/kyvault-ops/SKILL.md"), SKILL_MD)?,
        );
        report(
            "Gemini/agy 规则",
            &gem.join("rules/kyvault.md"),
            write_whole(&gem.join("rules/kyvault.md"), RULE_MD)?,
        );
    } else {
        println!("  · Gemini/agy：没有 ~/.gemini/config，跳过");
    }

    // ② Claude Code —— 共享文件，插标记块
    let cr = home.join(".clauderules");
    report("Claude Code 规则", &cr, upsert_block(&cr, RULE_MD)?);

    // ③ Codex
    let cx = home.join(".codex/AGENTS.md");
    if home.join(".codex").exists() {
        report("Codex 规则", &cx, upsert_block(&cx, RULE_MD)?);
    } else {
        println!("  · Codex：没有 ~/.codex，跳过");
    }

    // ④ Hermes 技能包
    let hm = home.join(".hermes");
    if hm.exists() {
        let p = hm.join("profiles/free/skills/kyvault-ops/SKILL.md");
        report("Hermes 技能包", &p, write_whole(&p, SKILL_MD)?);

        // Python 版还会去改 hermes 自己的 agent-operating-principles 里的「禁止事项」
        // 段落（split("## 禁止事项") 再找最后一个 "- ❌" 插一行）。**这里刻意不做**：
        // 那是在编辑别人维护的文件，靠字符串切割定位插入点，hermes 改一版就失效；
        // 而且失败是静默的。改成检测 + 提示，把动手的决定交回人。
        let std_path =
            hm.join("skills/autonomous-ai-agents/agent-operating-principles/references/secret-vault-naming-standard.md");
        if std_path.exists() {
            let c = fs::read_to_string(&std_path).unwrap_or_default();
            if !c.contains("强制覆写") && !c.contains("必须通过") {
                println!(
                    "  ⚠ Hermes 命名标准里没有「密钥失效必须覆写」这条：{}",
                    std_path.display()
                );
                println!("     那是 hermes 自己维护的文件，kyvault 不代改 —— 需要就手动加");
            }
        }
    } else {
        println!("  · Hermes：没有 ~/.hermes，跳过");
    }

    // ⑤ 当前目录是项目 → 注入局部规则
    let cwd = std::env::current_dir()?;
    let is_project = ["/.git", "/package.json", "/pyproject.toml", "/Cargo.toml"]
        .iter()
        .any(|s| cwd.join(s.trim_start_matches('/')).exists());
    if is_project {
        println!("\n检测到项目工作区 {}，注入局部规则：", cwd.display());
        let agents = cwd.join(".agents");
        let items: [(&str, PathBuf, &str); 3] = [
            (
                "局部技能包",
                agents.join("skills/kyvault-ops/SKILL.md"),
                SKILL_MD,
            ),
            ("局部规则", agents.join("rules/kyvault.md"), RULE_MD),
            ("局部 AGENTS.md", agents.join("AGENTS.md"), RULE_MD),
        ];
        for (label, p, c) in &items {
            report(label, p, write_whole(p, c)?);
        }
        for f in [".cursorrules", ".copilotinstructions", ".clauderules"] {
            let p = cwd.join(f);
            report(f, &p, upsert_block(&p, CURSOR_MD)?);
        }
    } else {
        println!("\n· 当前目录不像项目（没有 .git/package.json/pyproject.toml/Cargo.toml），跳过局部注入");
    }

    println!("\n✅ 连接完成。跑 kyvault doctor 复查对接状态。");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_is_idempotent() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(".clauderules");
        fs::write(&p, "用户自己的内容\n").unwrap();

        upsert_block(&p, "规则正文").unwrap();
        let once = fs::read_to_string(&p).unwrap();
        upsert_block(&p, "规则正文").unwrap();
        upsert_block(&p, "规则正文").unwrap();
        let thrice = fs::read_to_string(&p).unwrap();

        assert_eq!(
            once, thrice,
            "跑三次必须和跑一次一样 —— 这正是 Python 版的 bug"
        );
        assert!(thrice.contains("用户自己的内容"), "不能动用户原有内容");
        assert_eq!(thrice.matches(BEGIN).count(), 1, "只能有一个标记块");
    }

    #[test]
    fn upsert_replaces_on_content_change() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("AGENTS.md");
        upsert_block(&p, "旧规则").unwrap();
        upsert_block(&p, "新规则").unwrap();
        let t = fs::read_to_string(&p).unwrap();
        assert!(t.contains("新规则"));
        assert!(
            !t.contains("旧规则"),
            "升级规则时旧内容要被替换掉，不是并存"
        );
    }

    #[test]
    fn stale_version_is_kept_not_deleted() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(".clauderules");
        // 旧版本：标题相同但正文少一条 —— 精确匹配对不上
        fs::write(&p, "# 规则标题\n旧的第一条\n").unwrap();
        upsert_block(&p, "# 规则标题\n新的第一条\n新的第二条").unwrap();
        let t = fs::read_to_string(&p).unwrap();
        assert!(
            t.contains("旧的第一条"),
            "内容变过版本的旧副本不该被猜边界删掉"
        );
        assert!(t.contains("新的第二条"), "新版本要写进去");
        assert_eq!(t.matches(BEGIN).count(), 1);
    }

    #[test]
    fn upsert_dedupes_unmarked_copies() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(".clauderules");
        // 模拟 Python 版留下的 4 份无标记重复
        let body = "规则正文";
        fs::write(&p, format!("头部\n{body}\n{body}\n{body}\n{body}\n")).unwrap();
        upsert_block(&p, body).unwrap();
        let t = fs::read_to_string(&p).unwrap();
        assert_eq!(t.matches(body).count(), 1, "重复副本应被清成一份");
        assert!(t.contains("头部"), "别人的内容不能丢");
    }
}
