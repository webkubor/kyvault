//! 只读托管面板 —— 一份静态 HTML，可以扔到任何静态托管上。
//!
//! ## 为什么是静态导出，而不是「把 `ky ui` 挂到公网」
//!
//! `ky ui` 是**可写**的：页面上能填密钥、能点开看明文。它绑 127.0.0.1 不是
//! 将就，是安全模型的一部分。把它开在公网上等于把 master.key 的保护全部作废 ——
//! 一个能解出明文的页面，暴露在互联网上就不再是密钥库，是数据泄露事故。
//!
//! 所以对外那一层只导出**元信息**：谁有什么账号、挂了哪些 ref、类型、组织、
//! 权限范围、最后更新时间、操作计数。没有明文、没有密文、也没有 last4 ——
//! last4 看着无害，四条一组配上平台就能拿去做撞库线索。
//!
//! 生成的 HTML 是自包含的（内联 CSS，无外链、无 JS 请求），丢进 GitHub Pages /
//! 对象存储 / 内网 nginx 都能用。默认带 `<meta name="robots" content="noindex">`。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::audit;
use crate::model::now_utc;
use crate::registry::Registry;
use crate::store::Store;

/// 面板里的所有动态文本都从这里过 —— 密钥名和备注是用户输入，
/// 直接拼进 HTML 就等于给自己开一个 XSS。
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// 空字段显示破折号：表格里一整片空白读起来像「没读到」，而不是「本来就没有」。
fn cell(s: &str) -> String {
    if s.trim().is_empty() {
        "—".to_string()
    } else {
        esc(s)
    }
}

pub fn render(store: &Store) -> Result<String> {
    let metas = store.list_secrets()?;
    let reg = Registry::at(store.root())?;
    let profiles = reg.list();
    let active = reg.active().unwrap_or("");
    let events = audit::read(store, 20, None, None)?;
    let acounts = audit::counts(store)?;
    let broker_status = crate::broker::status(store)?;
    let health = crate::health::read(store)?;
    let mut device_rows = String::new();
    for d in broker_status["devices"].as_array().into_iter().flatten() {
        device_rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(d["id"].as_str().unwrap_or("")),
            esc(d["principal_label"].as_str().unwrap_or("")),
            esc(d["state"].as_str().unwrap_or("")),
            esc(d["source_ranges"].to_string().as_str()),
            esc(d["last_authorized_at"]
                .as_str()
                .unwrap_or("未观察到成功请求"))
        ));
    }
    if device_rows.is_empty() {
        device_rows.push_str("<tr><td colspan=\"5\">暂无已登记代理设备</td></tr>");
    }
    let mut health_rows = String::new();
    for (reference, h) in health.as_object().into_iter().flatten() {
        health_rows.push_str(&format!(
            "<tr><td class=\"mono\">{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(reference),
            esc(h["status"].as_str().unwrap_or("unknown")),
            esc(h["checked_at"].as_str().unwrap_or("")),
            esc(&h["elapsed_ms"].to_string())
        ));
    }
    if health_rows.is_empty() {
        health_rows.push_str("<tr><td colspan=\"4\">尚未执行平台有效性检查</td></tr>");
    }

    let platforms: std::collections::BTreeSet<&str> =
        metas.iter().map(|m| m.platform.as_str()).collect();

    let mut profile_rows = String::new();
    if profiles.is_empty() {
        profile_rows.push_str(
            "<tr><td colspan=\"6\" class=\"empty\">还没有登记账号 —— <code>ky profile add work --platform github --username alice</code></td></tr>",
        );
    }
    for p in &profiles {
        let is_active = p.name == active;
        profile_rows.push_str(&format!(
            "<tr><td><span class=\"mono\">{}</span>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"num\">{}</td></tr>\n",
            esc(&p.name),
            if is_active { " <span class=\"pill on\">当前</span>" } else { "" },
            cell(&p.platform),
            cell(&p.username),
            cell(&p.org),
            cell(&p.cli),
            p.refs.len()
        ));
    }

    let mut secret_rows = String::new();
    for m in &metas {
        secret_rows.push_str(&format!(
            "<tr><td class=\"mono\">{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"ts\">{}</td></tr>\n",
            esc(&m.r#ref()),
            cell(&m.kind),
            cell(&m.org),
            cell(&m.scopes),
            cell(&m.visibility),
            esc(&m.updated_at)
        ));
    }
    if metas.is_empty() {
        secret_rows.push_str("<tr><td colspan=\"6\" class=\"empty\">库里还没有密钥</td></tr>");
    }

    let mut audit_rows = String::new();
    for e in &events {
        let result = e["result"].as_str().unwrap_or("");
        let cls = if result == "ok" { "ok" } else { "warn" };
        audit_rows.push_str(&format!(
            "<tr><td class=\"ts\">{}</td><td class=\"mono\">{}</td><td><span class=\"pill {}\">{}</span></td><td class=\"mono\">{}</td></tr>\n",
            esc(e["ts"].as_str().unwrap_or("")),
            esc(e["actor"].as_str().unwrap_or("")),
            cls,
            esc(e["action"].as_str().unwrap_or("")),
            esc(e["target"].as_str().unwrap_or(""))
        ));
    }
    if events.is_empty() {
        audit_rows.push_str(
            "<tr><td colspan=\"4\" class=\"empty\">没有审计记录（KY_AUDIT 被关掉，或还没跑过命令）</td></tr>",
        );
    }

    let mut count_rows = String::new();
    for (a, n) in &acounts {
        count_rows.push_str(&format!(
            "<span class=\"chip\">{} <b>{}</b></span>",
            esc(a),
            n
        ));
    }
    if acounts.is_empty() {
        count_rows.push_str("<span class=\"empty\">—</span>");
    }

    let generated = now_utc();
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>kyvault · 凭证总览</title>
<style>
  :root {{
    --bg:#0d1117; --card:#161b22; --line:#30363d; --text:#c9d1d9;
    --dim:#8b949e; --bright:#f0f6fc; --accent:#58a6ff; --green:#3fb950; --amber:#d29922;
  }}
  * {{ box-sizing:border-box; margin:0; padding:0; }}
  body {{ background:var(--bg); color:var(--text); padding:32px 20px 64px;
         font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"PingFang SC",sans-serif; }}
  .wrap {{ max-width:1040px; margin:0 auto; }}
  header {{ display:flex; align-items:baseline; justify-content:space-between;
            border-bottom:1px solid var(--line); padding-bottom:16px; margin-bottom:24px; gap:16px; flex-wrap:wrap; }}
  h1 {{ font-size:20px; color:var(--bright); letter-spacing:-.3px; }}
  h1 span {{ color:var(--accent); }}
  .sub {{ font-size:12px; color:var(--dim); }}
  .kpis {{ display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:12px; margin-bottom:28px; }}
  .kpi {{ background:var(--card); border:1px solid var(--line); border-radius:10px; padding:16px; }}
  .kpi .n {{ font-size:26px; color:var(--bright); font-weight:700; }}
  .kpi .l {{ font-size:12px; color:var(--dim); margin-top:4px; }}
  section {{ margin-bottom:28px; }}
  h2 {{ font-size:13px; text-transform:uppercase; letter-spacing:.08em; color:var(--dim);
        margin-bottom:10px; font-weight:700; }}
  .card {{ background:var(--card); border:1px solid var(--line); border-radius:10px; overflow:auto; }}
  table {{ width:100%; border-collapse:collapse; font-size:13px; }}
  th,td {{ padding:10px 14px; text-align:left; border-bottom:1px solid var(--line); white-space:nowrap; }}
  th {{ color:var(--dim); font-weight:600; font-size:12px; }}
  tr:last-child td {{ border-bottom:none; }}
  .mono {{ font-family:ui-monospace,SFMono-Regular,Menlo,monospace; color:var(--bright); }}
  .ts {{ color:var(--dim); font-size:12px; font-family:ui-monospace,Menlo,monospace; }}
  .num {{ text-align:right; font-variant-numeric:tabular-nums; }}
  .empty {{ color:var(--dim); text-align:center; padding:20px; white-space:normal; }}
  .pill {{ display:inline-block; padding:2px 8px; border-radius:10px; font-size:11px; font-weight:600; }}
  .pill.on {{ background:rgba(63,185,80,.15); color:var(--green); border:1px solid rgba(63,185,80,.35); }}
  .pill.ok {{ background:rgba(63,185,80,.12); color:var(--green); }}
  .pill.warn {{ background:rgba(210,153,34,.15); color:var(--amber); }}
  .chip {{ display:inline-block; background:#0d1117; border:1px solid var(--line);
           border-radius:8px; padding:6px 10px; margin:0 8px 8px 0; font-size:12px; color:var(--dim); }}
  .chip b {{ color:var(--bright); }}
  footer {{ border-top:1px solid var(--line); margin-top:36px; padding-top:16px;
            font-size:12px; color:var(--dim); line-height:1.7; }}
  code {{ background:#0d1117; border:1px solid var(--line); border-radius:4px; padding:1px 5px;
          font-family:ui-monospace,Menlo,monospace; font-size:12px; color:var(--accent); }}
  .warn-box {{ border-left:3px solid var(--amber); padding-left:12px; }}
</style>
</head>
<body>
<div class="wrap">
  <header>
    <div>
      <h1>kyvault <span>凭证总览</span></h1>
      <div class="sub">只读元信息 · 生成于 {generated}</div>
    </div>
    <div class="sub">{np} 个账号 · {nm} 条密钥 · {nplat} 个平台</div>
  </header>

  <div class="kpis">
    <div class="kpi"><div class="n">{np}</div><div class="l">已登记账号</div></div>
    <div class="kpi"><div class="n">{nm}</div><div class="l">密钥条目</div></div>
    <div class="kpi"><div class="n">{nplat}</div><div class="l">平台</div></div>
    <div class="kpi"><div class="n">{nev}</div><div class="l">审计事件</div></div>
  </div>

  <section>
    <h2>账号 / Profile</h2>
    <div class="card"><table>
      <thead><tr><th>Profile</th><th>平台</th><th>登录名</th><th>组织</th><th>CLI</th><th class="num">密钥</th></tr></thead>
      <tbody>
{profile_rows}
      </tbody>
    </table></div>
  </section>

  <section>
    <h2>密钥条目（不含明文、不含 last4）</h2>
    <div class="card"><table>
      <thead><tr><th>Ref</th><th>类型</th><th>组织</th><th>Scopes</th><th>可见性</th><th>更新时间</th></tr></thead>
      <tbody>
{secret_rows}
      </tbody>
    </table></div>
  </section>

  <section>
    <h2>服务器 Agent 设备（SSH broker 登记）</h2>
    <div class="card"><table><thead><tr><th>设备</th><th>身份标签</th><th>授权状态</th><th>来源 IP/CIDR</th><th>最近成功请求</th></tr></thead><tbody>{device_rows}</tbody></table></div>
  </section>
  <section>
    <h2>最近有效性检查（历史结果，不代表此刻仍有效）</h2>
    <div class="card"><table><thead><tr><th>Ref</th><th>状态</th><th>检查时间</th><th>耗时 ms</th></tr></thead><tbody>{health_rows}</tbody></table></div>
  </section>
  <section>
    <h2>操作计数</h2>
    <div>{count_rows}</div>
  </section>

  <section>
    <h2>最近 20 条审计</h2>
    <div class="card"><table>
      <thead><tr><th>时间</th><th>操作人</th><th>动作</th><th>目标</th></tr></thead>
      <tbody>
{audit_rows}
      </tbody>
    </table></div>
  </section>

  <footer>
    <div class="warn-box">
      本页由 <code>ky panel</code> 生成，不包含凭据值、密文或尾号。
      <b>账号、设备、来源 IP 与审计仍属于敏感元信息</b>，请按访问权限托管。
      <code>noindex</code> 不是访问控制，本地操作人标签也不是身份认证。设备访问
      仅在受信 OpenSSH forced-command 部署下通过 SSH 认证；服务端日志不防本机管理员篡改。
    </div>
    <div style="margin-top:10px">
      重新生成：<code>ky panel --out panel.html</code> ·
      本地可视化编辑（仅本机）：<code>ky ui</code>
    </div>
  </footer>
</div>
</body>
</html>
"#,
        generated = esc(&generated),
        np = profiles.len(),
        nm = metas.len(),
        nplat = platforms.len(),
        nev = acounts.values().sum::<usize>(),
        profile_rows = profile_rows,
        secret_rows = secret_rows,
        audit_rows = audit_rows,
        count_rows = count_rows,
    );
    Ok(html)
}

/// 写到磁盘。默认文件模式 0644 —— 它本来就是要被静态服务器读的；
/// 里面没有密钥材料，收紧权限只会让托管读不到。
pub fn write(store: &Store, out: &Path) -> Result<PathBuf> {
    let html = render(store)?;
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("建目录失败：{}", parent.display()))?;
        }
    }
    std::fs::write(out, html).with_context(|| format!("写 {} 失败", out.display()))?;
    Ok(out.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_user_controlled_text() {
        assert_eq!(esc("<script>"), "&lt;script&gt;");
        assert_eq!(esc("a&b"), "a&amp;b");
        assert_eq!(esc("\"x\""), "&quot;x&quot;");
    }

    #[test]
    fn render_never_contains_plaintext_or_ciphertext() {
        let dir = tempfile::tempdir().unwrap();
        let st = Store::new(dir.path());
        st.init_master_key().unwrap();
        st.set_secret("secret://github/pat", "glpat-SUPERSECRET")
            .unwrap();
        let html = render(&st).unwrap();
        assert!(html.contains("secret://github/pat"));
        assert!(!html.contains("glpat-SUPERSECRET"));
        // last4 也不该出现
        assert!(!html.contains("CRET"));
        // 有 noindex，避免被搜索引擎收录
        assert!(html.contains("noindex"));
    }
}
