//! TUI 样式工具 —— ANSI 颜色、边框、对齐。
//!
//! 不引第三方 crate，纯 ANSI 转义序列。支持 NO_COLOR 环境变量关闭颜色。

/// 是否启用颜色输出（遵守 NO_COLOR 标准 + 检测 tty）
fn color_enabled() -> bool {
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    // 非 tty 时关闭颜色（管道、重定向）
    #[cfg(unix)]
    {
        unsafe { libc_isatty(1) != 0 }
    }
    #[cfg(not(unix))]
    true
}

#[cfg(unix)]
extern "C" {
    fn isatty(fd: i32) -> i32;
}

#[cfg(unix)]
unsafe fn libc_isatty(fd: i32) -> i32 {
    unsafe { isatty(fd) }
}

// ─── ANSI 颜色 ───

pub fn bold(s: &str) -> String {
    if color_enabled() {
        format!("\x1b[1m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn dim(s: &str) -> String {
    if color_enabled() {
        format!("\x1b[2m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn green(s: &str) -> String {
    if color_enabled() {
        format!("\x1b[32m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn red(s: &str) -> String {
    if color_enabled() {
        format!("\x1b[31m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn yellow(s: &str) -> String {
    if color_enabled() {
        format!("\x1b[33m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn cyan(s: &str) -> String {
    if color_enabled() {
        format!("\x1b[36m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn magenta(s: &str) -> String {
    if color_enabled() {
        format!("\x1b[35m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn bold_green(s: &str) -> String {
    if color_enabled() {
        format!("\x1b[1;32m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn bold_red(s: &str) -> String {
    if color_enabled() {
        format!("\x1b[1;31m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn bold_cyan(s: &str) -> String {
    if color_enabled() {
        format!("\x1b[1;36m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn bold_yellow(s: &str) -> String {
    if color_enabled() {
        format!("\x1b[1;33m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

// ─── 边框绘制 ───

const W: usize = 58;

/// 顶部圆角边框
pub fn box_top(title: &str) {
    let inner = W - 2;
    let t = format!(" {title} ");
    let pad = inner.saturating_sub(t.len());
    let left = pad / 2;
    let right = pad - left;
    println!(
        "{}{}{}{}{}",
        dim("╭"),
        dim(&"─".repeat(left)),
        bold_cyan(&t),
        dim(&"─".repeat(right)),
        dim("╮")
    );
}

/// 底部圆角边框
pub fn box_bottom() {
    println!("{}{}{}", dim("╰"), dim(&"─".repeat(W - 2)), dim("╯"));
}

/// 分隔线（在 box 内）
pub fn box_sep() {
    println!("{}{}{}", dim("├"), dim(&"─".repeat(W - 2)), dim("┤"));
}

/// 小节标题（在 box 内）
pub fn section(icon: &str, title: &str) {
    println!();
    println!("  {} {}", icon, bold(title));
}

/// 状态行：label + OK/FAIL
pub fn status_line(label: &str, ok: bool, detail: &str) {
    let tag = if ok {
        bold_green("✓")
    } else {
        bold_red("✗")
    };
    let detail_colored = if ok { green(detail) } else { red(detail) };
    println!("    {tag}  {:<30} {detail_colored}", label);
}

/// 普通信息行
pub fn info_line(label: &str, value: &str) {
    println!("    {}  {}", dim(label), value);
}

/// 警告行
pub fn warn_line(msg: &str) {
    println!("    {}  {}", bold_yellow("⚠"), yellow(msg));
}

/// 键值对行（带标签颜色）
pub fn kv(key: &str, val: &str) {
    println!("    {:<20} {}", dim(key), val);
}
