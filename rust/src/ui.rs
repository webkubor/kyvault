//! 本地嵌入式 Web GUI 服务。
//!
//! 零外部依赖，使用标准库 `std::net::TcpListener` 启动超轻量 localhost Web 服务。
//! 单页极客科技暗黑界面，支持可视化分类点选、规范化 URI 生成、密码加解密存取。
//!
//! 绑 127.0.0.1 只挡得住别的机器，挡不住本机浏览器里打开的任意网页：网页的 JS
//! 照样能请求 127.0.0.1。所以这里能读明文的接口必须再加三道：
//! - URL 带每次启动随机生成的 token，猜不到路径就只有 404；
//! - Host 只认 127.0.0.1/localhost（挡 DNS rebinding）；
//! - POST 只收 application/json 且不回 CORS 头 —— 跨站页面发这种请求必先预检，
//!   预检得不到许可，请求就发不出来，响应也读不到。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::json;

use crate::category::{format_uri, CATEGORIES};
use crate::store::Store;
use crate::tui::*;

#[derive(Deserialize)]
struct SetPayload {
    platform: String,
    name: String,
    value: String,
    kind: Option<String>,
    account: Option<String>,
    alias: Option<String>,
}

#[derive(Deserialize)]
struct RevealPayload {
    uri: String,
}

pub fn start_server(port: u16, auto_open: bool) -> Result<()> {
    let mut current_port = port;
    let listener = loop {
        match TcpListener::bind(format!("127.0.0.1:{current_port}")) {
            Ok(l) => break l,
            Err(_) => {
                current_port += 1;
                if current_port > port + 50 {
                    return Err(anyhow!("无法找到可用的本地端口"));
                }
            }
        }
    };

    let token = format!("{:032x}", rand::random::<u128>());
    let url = format!("http://127.0.0.1:{current_port}/{token}/");
    box_top("🌐 kyvault Web GUI");
    println!();
    status_line("本地服务", true, &format!("已监听 {url}"));
    if auto_open {
        status_line("浏览器", true, "正在自动拉起默认浏览器...");
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&url).status();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(&url).status();
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", &url])
            .status();
    }
    info_line("提示", "按 Ctrl+C 可停止本地 Web 服务");
    println!();
    box_bottom();

    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                let token = token.clone();
                thread::spawn(move || {
                    let _ = handle_connection(&mut s, &token, current_port);
                });
            }
            Err(_) => break,
        }
    }
    Ok(())
}

fn handle_connection(stream: &mut TcpStream, token: &str, port: u16) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut req_line = String::new();
    reader.read_line(&mut req_line)?;
    let parts: Vec<&str> = req_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0];
    let full_path = parts[1];

    let mut content_length = 0;
    let mut host = String::new();
    let mut content_type = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => content_length = v.trim().parse::<usize>().unwrap_or(0),
                "host" => host = v.trim().to_string(),
                "content-type" => content_type = v.trim().to_ascii_lowercase(),
                _ => {}
            }
        }
    }

    let host_ok = host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}");
    let routed = full_path
        .split('?')
        .next()
        .and_then(|p| p.strip_prefix('/'))
        .and_then(|p| p.strip_prefix(token))
        .filter(|_| host_ok)
        .filter(|_| method != "POST" || content_type.starts_with("application/json"))
        .filter(|_| content_length <= 64 * 1024);
    let Some(rest) = routed else {
        return send_response(stream, "404 Not Found", "text/plain", b"Not Found");
    };
    let path = if rest.is_empty() { "/" } else { rest };

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    match (method, path) {
        ("GET", "/") => send_response(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            HTML_CONTENT.as_bytes(),
        )?,
        ("GET", "/api/categories") => {
            let data = serde_json::to_string(&CATEGORIES)?;
            send_response(stream, "200 OK", "application/json", data.as_bytes())?;
        }
        ("GET", "/api/status") => {
            let store = Store::default_location().map_err(|e| anyhow!("{e}"))?;
            let root = store.root();
            let is_guard = crate::auth_guard::AuthGuard::is_enabled(root);
            let count = store.list_secrets().map(|s| s.len()).unwrap_or(0);
            let res = json!({
                "ok": true,
                "version": env!("CARGO_PKG_VERSION"),
                "store_dir": root.to_string_lossy(),
                "secrets_count": count,
                "guard_enabled": is_guard,
                "backend": "file / gitlab"
            });
            send_response(
                stream,
                "200 OK",
                "application/json",
                res.to_string().as_bytes(),
            )?;
        }
        ("GET", "/api/secrets") => {
            let store = Store::default_location().map_err(|e| anyhow!("{e}"))?;
            let secrets: Vec<serde_json::Value> = store
                .list_secrets()
                .unwrap_or_default()
                .into_iter()
                .map(|s| {
                    let mut val = serde_json::to_value(&s).unwrap_or_default();
                    if let Some(obj) = val.as_object_mut() {
                        obj.insert("ref".into(), json!(s.r#ref()));
                    }
                    val
                })
                .collect();
            let res = json!({
                "ok": true,
                "secrets": secrets
            });
            send_response(
                stream,
                "200 OK",
                "application/json",
                res.to_string().as_bytes(),
            )?;
        }
        ("POST", "/api/set") => {
            if let Ok(payload) = serde_json::from_slice::<SetPayload>(&body) {
                let write_res: Result<(String, String)> = (|| {
                    let uri = format_uri(&payload.platform, &payload.name);
                    let store = Store::default_location()?;
                    store.set_secret(&uri, &payload.value)?;
                    let kind = payload.kind.as_deref().unwrap_or("API Key");
                    let acct = payload.account.as_deref().filter(|s| !s.is_empty());
                    store.annotate(&uri, acct, Some(kind), None, None, None)?;
                    if let Some(ref alias) = payload.alias {
                        if !alias.is_empty() {
                            let aliases = crate::alias::Aliases::default_location()?;
                            let _ = aliases.set(alias, &uri);
                        }
                    }
                    let run_cmd = format!("kyvault run --env TOKEN={} -- <command>", uri);
                    Ok((uri, run_cmd))
                })();

                match write_res {
                    Ok((uri, run_cmd)) => {
                        let res = json!({
                            "ok": true,
                            "uri": uri,
                            "run_cmd": run_cmd
                        });
                        send_response(
                            stream,
                            "200 OK",
                            "application/json",
                            res.to_string().as_bytes(),
                        )?;
                    }
                    Err(e) => {
                        let res = json!({
                            "ok": false,
                            "error": format!("{e}")
                        });
                        send_response(
                            stream,
                            "500 Internal Server Error",
                            "application/json",
                            res.to_string().as_bytes(),
                        )?;
                    }
                }
            } else {
                send_response(
                    stream,
                    "400 Bad Request",
                    "application/json",
                    b"{\"ok\":false,\"error\":\"Invalid payload\"}",
                )?;
            }
        }
        ("POST", "/api/reveal") => {
            if let Ok(payload) = serde_json::from_slice::<RevealPayload>(&body) {
                let store_res = Store::default_location();
                match store_res {
                    Ok(store) => match store.get_secret(&payload.uri) {
                        Ok(Some(val)) => {
                            let res = json!({
                                "ok": true,
                                "value": val
                            });
                            send_response(
                                stream,
                                "200 OK",
                                "application/json",
                                res.to_string().as_bytes(),
                            )?;
                        }
                        Ok(None) => {
                            let res = json!({ "ok": false, "error": "未找到指定密钥或值为空" });
                            send_response(
                                stream,
                                "404 Not Found",
                                "application/json",
                                res.to_string().as_bytes(),
                            )?;
                        }
                        Err(e) => {
                            let res = json!({ "ok": false, "error": format!("{e}") });
                            send_response(
                                stream,
                                "500 Internal Server Error",
                                "application/json",
                                res.to_string().as_bytes(),
                            )?;
                        }
                    },
                    Err(e) => {
                        let res = json!({ "ok": false, "error": format!("{e}") });
                        send_response(
                            stream,
                            "500 Internal Server Error",
                            "application/json",
                            res.to_string().as_bytes(),
                        )?;
                    }
                }
            } else {
                send_response(
                    stream,
                    "400 Bad Request",
                    "application/json",
                    b"{\"ok\":false,\"error\":\"Invalid payload\"}",
                )?;
            }
        }
        _ => {
            send_response(stream, "404 Not Found", "text/plain", b"Not Found")?;
        }
    }
    Ok(())
}

fn send_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

const HTML_CONTENT: &str = r#"<!DOCTYPE html>
<html lang="zh-CN" data-theme="dark">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>KYVAULT // 极客凭证管理引擎</title>
<style>
  :root, [data-theme="dark"] {
    color-scheme: dark;
    --bg: #070b09;
    --bg-grid: rgba(102, 240, 181, 0.035);
    --card: rgba(12, 20, 16, 0.86);
    --card-border: rgba(102, 240, 181, 0.16);
    --accent: #66f0b5;
    --accent-glow: rgba(102, 240, 181, 0.28);
    --accent-soft: rgba(102, 240, 181, 0.09);
    --cold-line: rgba(102, 240, 181, 0.24);
    --cold-line-active: rgba(102, 240, 181, 0.85);
    --text-bright: #f2f7f4;
    --text: #c2d4cb;
    --text-dim: #7a9487;
    --input-bg: #050907;
    --code-bg: #030604;
  }
  @media (prefers-color-scheme: light) {
    :root:not([data-theme="dark"]) {
      color-scheme: light;
      --bg: #f4f8f6;
      --bg-grid: rgba(16, 128, 88, 0.04);
      --card: #ffffff;
      --card-border: rgba(16, 128, 88, 0.2);
      --accent: #0f9f6e;
      --accent-glow: rgba(16, 159, 110, 0.2);
      --accent-soft: rgba(16, 159, 110, 0.08);
      --cold-line: rgba(16, 128, 88, 0.32);
      --cold-line-active: #0f9f6e;
      --text-bright: #0c1813;
      --text: #233830;
      --text-dim: #5c786c;
      --input-bg: #ffffff;
      --code-bg: #edf5f0;
    }
  }
  :root {
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Display", "Segoe UI", Roboto, "Helvetica Neue", sans-serif;
  }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body {
    background-color: var(--bg);
    background-image:
      linear-gradient(to right, var(--bg-grid) 1px, transparent 1px),
      linear-gradient(to bottom, var(--bg-grid) 1px, transparent 1px);
    background-size: 32px 32px;
    color: var(--text);
    min-height: 100vh;
    padding: 32px 20px 48px;
    display: flex;
    flex-direction: column;
    align-items: center;
    -webkit-font-smoothing: antialiased;
  }
  .container { width: 100%; max-width: 900px; }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 24px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--cold-line);
    position: relative;
  }
  header::after {
    content: "";
    position: absolute;
    left: 0;
    bottom: -1px;
    width: 104px;
    height: 2px;
    background: var(--accent);
    box-shadow: 0 0 12px var(--accent-glow);
  }
  .logo-box { display: flex; align-items: center; gap: 14px; }
  .logo-mark {
    width: 38px;
    height: 38px;
    border: 1px solid var(--cold-line);
    border-radius: 10px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--accent-soft);
    box-shadow: 0 0 16px var(--accent-glow);
    font-size: 18px;
  }
  h1 {
    font-size: 19px;
    color: var(--text-bright);
    font-weight: 700;
    letter-spacing: 0.5px;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  }
  .sub-tag { font-size: 12px; color: var(--text-dim); margin-top: 3px; letter-spacing: 0.2px; }
  .status-pill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    background: var(--accent-soft);
    border: 1px solid var(--cold-line);
    color: var(--accent);
    padding: 5px 12px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    box-shadow: 0 0 12px var(--accent-glow);
  }
  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 8px var(--accent);
    animation: pulse 2.4s infinite ease-in-out;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.45; transform: scale(0.9); }
    50% { opacity: 1; transform: scale(1.15); }
  }
  .nav-tabs { display: flex; gap: 10px; margin-bottom: 22px; }
  .tab-btn {
    background: var(--card);
    border: 1px solid var(--card-border);
    color: var(--text-dim);
    padding: 10px 18px;
    border-radius: 8px;
    cursor: pointer;
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0.2px;
    transition: all 0.2s cubic-bezier(0.2, 0.8, 0.2, 1);
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .tab-btn:hover {
    color: var(--text-bright);
    border-color: var(--cold-line-active);
  }
  .tab-btn.active {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
    box-shadow: 0 0 16px var(--accent-glow);
  }
  .card {
    background: var(--card);
    border: 1px solid var(--card-border);
    border-radius: 12px;
    padding: 24px;
    margin-bottom: 20px;
    backdrop-filter: blur(8px);
    box-shadow: 0 16px 36px rgba(0, 0, 0, 0.35);
  }
  .step-title {
    font-size: 12px;
    text-transform: uppercase;
    color: var(--text-dim);
    margin-bottom: 12px;
    font-weight: 700;
    display: flex;
    align-items: center;
    gap: 8px;
    letter-spacing: 1px;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  }
  .step-badge {
    color: var(--accent);
    background: var(--accent-soft);
    border: 1px solid var(--cold-line);
    padding: 2px 6px;
    border-radius: 4px;
    font-size: 11px;
  }
  .step-line {
    flex: 1;
    height: 1px;
    background: linear-gradient(to right, var(--cold-line), transparent);
  }
  .category-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(136px, 1fr));
    gap: 10px;
    margin-bottom: 22px;
  }
  .cat-item, .plat-item {
    background: var(--input-bg);
    border: 1px solid var(--card-border);
    border-radius: 8px;
    padding: 14px 10px;
    text-align: center;
    cursor: pointer;
    transition: all 0.18s cubic-bezier(0.2, 0.8, 0.2, 1);
    position: relative;
    overflow: hidden;
  }
  .cat-item:hover, .plat-item:hover {
    border-color: var(--cold-line-active);
    transform: translateY(-2px);
    box-shadow: 0 6px 16px rgba(0, 0, 0, 0.25);
  }
  .cat-item.active, .plat-item.active {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--text-bright);
    box-shadow: 0 0 16px var(--accent-glow);
  }
  .cat-icon { font-size: 20px; margin-bottom: 6px; }
  .cat-name { font-size: 12px; font-weight: 600; letter-spacing: 0.2px; }
  .uri-preview {
    background: var(--code-bg);
    border: 1px solid var(--cold-line);
    border-radius: 8px;
    padding: 14px 16px;
    margin: 18px 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    box-shadow: inset 0 0 12px rgba(0,0,0,0.4);
  }
  .uri-label {
    font-size: 11px;
    color: var(--text-dim);
    display: block;
    margin-bottom: 4px;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    letter-spacing: 0.6px;
  }
  .uri-text {
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    font-size: 14px;
    color: var(--accent);
    font-weight: 600;
    word-break: break-all;
    text-shadow: 0 0 10px var(--accent-glow);
  }
  .form-group { margin-bottom: 16px; }
  label {
    display: block;
    font-size: 12px;
    color: var(--text-dim);
    margin-bottom: 6px;
    font-weight: 600;
    letter-spacing: 0.2px;
  }
  input, select {
    width: 100%;
    background: var(--input-bg);
    border: 1px solid var(--card-border);
    border-radius: 8px;
    padding: 10px 14px;
    color: var(--text-bright);
    font-size: 13px;
    outline: none;
    transition: border-color 0.2s, box-shadow 0.2s;
  }
  input:focus, select:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-soft);
  }
  .btn-submit {
    width: 100%;
    background: linear-gradient(135deg, var(--accent) 0%, #34d399 100%);
    border: 1px solid var(--accent);
    border-radius: 8px;
    color: #03140a;
    padding: 12px;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    transition: all 0.2s cubic-bezier(0.2, 0.8, 0.2, 1);
    margin-top: 12px;
    letter-spacing: 0.3px;
    box-shadow: 0 0 18px var(--accent-glow);
  }
  .btn-submit:hover:not(:disabled) {
    filter: brightness(1.08);
    transform: translateY(-1px);
    box-shadow: 0 0 24px rgba(102, 240, 181, 0.45);
  }
  .btn-submit:disabled {
    opacity: 0.6;
    cursor: not-allowed;
    transform: none;
  }
  .secret-table { width: 100%; border-collapse: collapse; font-size: 13px; }
  .secret-table th, .secret-table td { padding: 12px; text-align: left; border-bottom: 1px solid var(--card-border); vertical-align: middle; }
  .secret-table th {
    color: var(--text-dim);
    font-weight: 600;
    font-size: 11px;
    letter-spacing: 0.8px;
    text-transform: uppercase;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  }
  .secret-table code {
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    color: var(--text-dim);
  }
  .btn-copy {
    background: var(--input-bg);
    border: 1px solid var(--card-border);
    color: var(--text);
    padding: 5px 10px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 12px;
    transition: all 0.15s;
    white-space: nowrap;
  }
  .btn-copy:hover {
    border-color: var(--accent);
    color: var(--accent);
    box-shadow: 0 0 10px var(--accent-glow);
  }
  .toast {
    position: fixed;
    bottom: 24px;
    right: 24px;
    background: rgba(7, 16, 11, 0.94);
    border: 1px solid var(--accent);
    color: var(--accent);
    padding: 12px 20px;
    border-radius: 8px;
    font-weight: 600;
    display: none;
    box-shadow: 0 8px 24px rgba(0,0,0,0.5), 0 0 16px var(--accent-glow);
    font-size: 13px;
    z-index: 1000;
    backdrop-filter: blur(10px);
  }
</style>
</head>
<body>
<div class="container">
  <header>
    <div class="logo-box">
      <div class="logo-mark">⚡</div>
      <div>
        <h1>KYVAULT // 凭证管理引擎</h1>
        <div class="sub-tag">零信任本地端对端加密 · 冷光线条 · 自动规范化命名</div>
      </div>
    </div>
    <div style="display:flex; align-items:center; gap:8px;">
      <div id="store-count" class="status-pill"><span class="status-dot"></span><span>连接引擎中...</span></div>
    </div>
  </header>

  <div class="nav-tabs">
    <button class="tab-btn active" onclick="switchTab('store')"><span>⚡</span><span>智能存入向导</span></button>
    <button class="tab-btn" onclick="switchTab('list')"><span>⌘</span><span>凭证检索台账</span></button>
  </div>

  <!-- 选项卡 1：智能存入向导 -->
  <div id="tab-store" class="tab-content">
    <div class="card">
      <div class="step-title"><span class="step-badge">STEP 01</span><span>选择凭据大类</span><span class="step-line"></span></div>
      <div id="category-grid" class="category-grid"></div>

      <div class="step-title"><span class="step-badge">STEP 02</span><span>选择或指定平台</span><span class="step-line"></span></div>
      <div id="platform-grid" class="category-grid"></div>

      <div class="step-title"><span class="step-badge">STEP 03</span><span>确认用途与标准 URI</span><span class="step-line"></span></div>
      <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 12px;">
        <div class="form-group">
          <label>标识 / 环境名 (Name)</label>
          <input id="input-name" value="main" oninput="updateUriPreview()" placeholder="如 main, pat, ci">
        </div>
        <div class="form-group">
          <label>快捷别名 (Alias, 可选)</label>
          <input id="input-alias" placeholder="留空自动生成">
        </div>
      </div>

      <div class="uri-preview">
        <div>
          <span class="uri-label">规范寻址 URI</span>
          <span id="uri-display" class="uri-text">secret://deepseek/main</span>
        </div>
        <button class="btn-copy" onclick="copyText(document.getElementById('uri-display').innerText)">复制规范 URI</button>
      </div>

      <div class="step-title"><span class="step-badge">STEP 04</span><span>输入安全目标值</span><span class="step-line"></span></div>
      <div class="form-group">
        <label>密钥明文 (写入磁盘前进行 AES-256-GCM 军事级加密)</label>
        <div style="display:flex; gap:8px;">
          <input id="input-value" type="password" placeholder="粘贴密钥、Token 或密码...">
          <button class="btn-copy" onclick="togglePassword()" title="显示/隐藏明文">👁️</button>
        </div>
      </div>
      <div class="form-group">
        <label>绑定账号 / 备注信息 (可选)</label>
        <input id="input-account" placeholder="如 admin@gmail.com 或 生产集群Token">
      </div>

      <button class="btn-submit" onclick="submitSecret()">⚡ 安全加密存入 kyvault</button>
    </div>
  </div>

  <!-- 选项卡 2：凭据检索台账 -->
  <div id="tab-list" class="tab-content" style="display:none;">
    <div class="card">
      <div style="margin-bottom:16px; display:flex; justify-content:space-between; align-items:center;">
        <input id="search-input" placeholder="🔍 搜索平台、别名或分类..." oninput="filterSecrets()" style="max-width:300px;">
        <button class="btn-copy" onclick="loadSecrets()">⟳ 刷新列表</button>
      </div>
      <table class="secret-table">
        <thead>
          <tr style="border-bottom: 1px solid var(--cold-line);">
            <th>URI 寻址 / 别名</th>
            <th>类型</th>
            <th>末位标识</th>
            <th>备注/账号</th>
            <th>操作</th>
          </tr>
        </thead>
        <tbody id="secrets-tbody">
          <tr><td colspan="5" style="text-align:center; color:var(--text-dim);">加载中...</td></tr>
        </tbody>
      </table>
    </div>
  </div>
</div>

<div id="toast" class="toast">操作成功</div>

<script>
const base = location.pathname.replace(/\/$/, '');
const esc = v => String(v ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
let gCategories = [];
let gCurrentCat = null;
let gCurrentPlat = null;
let gSecrets = [];

async function init() {
  const catRes = await fetch(base + '/api/categories');
  gCategories = await catRes.json();
  renderCategories();
  selectCategory(gCategories[0].id);

  const statusRes = await fetch(base + '/api/status');
  if (statusRes.ok) {
    const status = await statusRes.json();
    document.getElementById('store-count').innerHTML = `<span class="status-dot"></span><span>${status.secrets_count} 条凭据就绪</span>`;
  } else {
    document.getElementById('store-count').innerHTML = `<span class="status-dot" style="background:#ef4444;box-shadow:0 0 8px #ef4444;"></span><span>状态离线</span>`;
  }
}

function renderCategories() {
  const grid = document.getElementById('category-grid');
  grid.innerHTML = gCategories.map(c => `
    <div class="cat-item ${gCurrentCat?.id === c.id ? 'active' : ''}" onclick="selectCategory('${c.id}')">
      <div class="cat-icon">${c.icon}</div>
      <div class="cat-name">${c.name}</div>
    </div>
  `).join('');
}

function selectCategory(catId) {
  gCurrentCat = gCategories.find(c => c.id === catId);
  renderCategories();

  const platGrid = document.getElementById('platform-grid');
  platGrid.innerHTML = gCurrentCat.platforms.map(p => `
    <div class="plat-item ${gCurrentPlat?.id === p.id ? 'active' : ''}" data-plat-id="${p.id}" onclick="selectPlatform('${p.id}')">
      <div class="cat-icon">${p.icon}</div>
      <div class="cat-name">${p.name}</div>
    </div>
  `).join('');

  if (gCurrentCat.platforms.length > 0) {
    selectPlatform(gCurrentCat.platforms[0].id);
  }
}

function selectPlatform(platId) {
  gCurrentPlat = gCurrentCat.platforms.find(p => p.id === platId);
  document.querySelectorAll('.plat-item').forEach(el => {
    const isCurrent = el.getAttribute('data-plat-id') === platId;
    el.classList.toggle('active', isCurrent);
  });

  const nameInput = document.getElementById('input-name');
  nameInput.value = gCurrentPlat.default_name || 'main';
  updateUriPreview();
}

function updateUriPreview() {
  const plat = gCurrentPlat ? gCurrentPlat.id : 'custom';
  const name = document.getElementById('input-name').value.trim() || 'main';
  const uri = `secret://${plat}/${name}`;
  document.getElementById('uri-display').innerText = uri;
  document.getElementById('input-alias').placeholder = `${plat}_${name}`;
}

function togglePassword() {
  const el = document.getElementById('input-value');
  el.type = el.type === 'password' ? 'text' : 'password';
}

async function submitSecret() {
  const plat = gCurrentPlat ? gCurrentPlat.id : 'custom';
  const name = document.getElementById('input-name').value.trim();
  const val = document.getElementById('input-value').value.trim();
  const acct = document.getElementById('input-account').value.trim();
  const alias = document.getElementById('input-alias').value.trim();
  const btn = document.querySelector('.btn-submit');

  if (!name || !val) {
    alert('请填写名称和密钥值！');
    return;
  }

  const originalText = btn ? btn.innerText : '';
  if (btn) {
    btn.disabled = true;
    btn.innerText = '⏳ 正在加密写入磁盘...';
  }
  try {
    const res = await fetch(base + '/api/set', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        platform: plat,
        name: name,
        value: val,
        kind: gCurrentCat?.default_kind || 'API Key',
        account: acct,
        alias: alias
      })
    });
    const data = await res.json();
    if (data.ok) {
      showToast(`✅ 成功加密存入 ${data.uri}`);
      document.getElementById('input-value').value = '';
      loadSecrets();
    } else {
      alert('保存失败：' + (data.error || '未知服务端错误'));
    }
  } catch (err) {
    alert('网络或服务异常：' + (err.message || err));
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.innerText = originalText;
    }
  }
}

async function loadSecrets() {
  const res = await fetch(base + '/api/secrets');
  const data = await res.json();
  gSecrets = data.secrets || [];
  renderSecretsTable(gSecrets);
}

function renderSecretsTable(list) {
  const tbody = document.getElementById('secrets-tbody');
  if (list.length === 0) {
    tbody.innerHTML = '<tr><td colspan="5" style="text-align:center; color:var(--text-dim);">无匹配密钥</td></tr>';
    return;
  }
  tbody.innerHTML = list.map(s => `
    <tr>
      <td><span style="font-weight:600; color:var(--accent); font-family:ui-monospace,monospace;">${esc(s.ref)}</span></td>
      <td><span class="step-badge" style="font-size:11px;">${esc(s.kind || 'API Key')}</span></td>
      <td><code>...${esc(s.last4 || '****')}</code></td>
      <td style="color:var(--text-dim);">${esc(s.account || '-')}</td>
      <td>
        <button class="btn-copy" data-ref="${esc(s.ref)}" onclick="revealSecret(this.dataset.ref)">复制明文</button>
      </td>
    </tr>
  `).join('');
}

async function revealSecret(uri) {
  const res = await fetch(base + '/api/reveal', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ uri })
  });
  const data = await res.json();
  if (data.ok) {
    copyText(data.value);
    showToast(`🔑 明文已复制到剪贴板`);
  } else {
    alert('解密失败：' + data.error);
  }
}

function filterSecrets() {
  const q = document.getElementById('search-input').value.toLowerCase();
  const filtered = gSecrets.filter(s => s.ref.toLowerCase().includes(q) || (s.account && s.account.toLowerCase().includes(q)));
  renderSecretsTable(filtered);
}

function switchTab(tab) {
  document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
  document.querySelectorAll('.tab-content').forEach(c => c.style.display = 'none');
  if (tab === 'store') {
    document.querySelector('.tab-btn:nth-child(1)').classList.add('active');
    document.getElementById('tab-store').style.display = 'block';
  } else {
    document.querySelector('.tab-btn:nth-child(2)').classList.add('active');
    document.getElementById('tab-list').style.display = 'block';
    loadSecrets();
  }
}

function copyText(txt) {
  navigator.clipboard.writeText(txt);
  showToast('已复制到剪切板');
}

function showToast(msg) {
  const t = document.getElementById('toast');
  t.innerText = msg;
  t.style.display = 'block';
  setTimeout(() => { t.style.display = 'none'; }, 3000);
}

window.onload = init;
</script>
</body>
</html>
"#;
