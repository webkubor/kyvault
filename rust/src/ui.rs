//! 本地嵌入式 Web GUI 服务。
//!
//! 零外部依赖，使用标准库 `std::net::TcpListener` 启动超轻量 localhost Web 服务。
//! 单页极客科技暗黑界面，支持可视化分类点选、规范化 URI 生成、密码加解密存取。

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

    let url = format!("http://127.0.0.1:{current_port}");
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
                thread::spawn(move || {
                    let _ = handle_connection(&mut s);
                });
            }
            Err(_) => break,
        }
    }
    Ok(())
}

fn handle_connection(stream: &mut TcpStream) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut req_line = String::new();
    reader.read_line(&mut req_line)?;
    let parts: Vec<&str> = req_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0];
    let path = parts[1];

    let mut content_length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            break;
        }
        if line.to_lowercase().starts_with("content-length:") {
            if let Some(val) = line.split(':').nth(1) {
                content_length = val.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }

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
            let secrets = store.list_secrets().unwrap_or_default();
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
                let uri = format_uri(&payload.platform, &payload.name);
                let store = Store::default_location()?;
                let _lock = store.lock()?;
                store.set_secret(&uri, &payload.value)?;
                let kind = payload.kind.as_deref().unwrap_or("API Key");
                let acct = payload.account.as_deref().filter(|s| !s.is_empty());
                store.annotate(&uri, Some(kind), acct, None, None, None)?;
                if let Some(ref alias) = payload.alias {
                    if !alias.is_empty() {
                        let aliases = crate::alias::Aliases::default_location()?;
                        let _ = aliases.set(alias, &uri);
                    }
                }
                let run_cmd = format!("kyvault run --env TOKEN={} -- <command>", uri);
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
                let store = Store::default_location()?;
                match store.get_secret(&payload.uri) {
                    Ok(val) => {
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
                    Err(e) => {
                        let res = json!({ "ok": false, "error": format!("{e}") });
                        send_response(
                            stream,
                            "404 Not Found",
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
                    b"{\"ok\":false}",
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
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

const HTML_CONTENT: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>kyvault 极客凭证中心</title>
<style>
  :root {
    --bg: #0d1117;
    --card: #161b22;
    --card-border: #30363d;
    --accent: #58a6ff;
    --accent-glow: rgba(88, 166, 255, 0.2);
    --green: #3fb950;
    --text: #c9d1d9;
    --text-dim: #8b949e;
    --text-bright: #f0f6fc;
  }
  * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, monospace; }
  body { background: var(--bg); color: var(--text); min-height: 100vh; padding: 24px; display: flex; flex-direction: column; align-items: center; }
  .container { width: 100%; max-width: 900px; }
  header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 24px; padding-bottom: 16px; border-bottom: 1px solid var(--card-border); }
  .logo-box { display: flex; align-items: center; gap: 12px; }
  .logo { font-size: 28px; }
  h1 { font-size: 22px; color: var(--text-bright); font-weight: 700; letter-spacing: -0.5px; }
  .badge { background: #238636; color: #fff; padding: 3px 8px; border-radius: 12px; font-size: 11px; font-weight: 600; }
  .nav-tabs { display: flex; gap: 8px; margin-bottom: 24px; }
  .tab-btn { background: var(--card); border: 1px solid var(--card-border); color: var(--text-dim); padding: 10px 20px; border-radius: 8px; cursor: pointer; font-size: 14px; font-weight: 600; transition: all 0.2s; }
  .tab-btn.active { background: #1f2937; border-color: var(--accent); color: var(--accent); box-shadow: 0 0 12px var(--accent-glow); }
  .card { background: var(--card); border: 1px solid var(--card-border); border-radius: 12px; padding: 24px; margin-bottom: 20px; }
  .step-title { font-size: 14px; text-transform: uppercase; color: var(--text-dim); margin-bottom: 12px; font-weight: 700; display: flex; align-items: center; gap: 8px; }
  .category-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(130px, 1fr)); gap: 10px; margin-bottom: 20px; }
  .cat-item, .plat-item { background: #0d1117; border: 1px solid var(--card-border); border-radius: 8px; padding: 12px; text-align: center; cursor: pointer; transition: all 0.15s; }
  .cat-item:hover, .plat-item:hover { border-color: var(--accent); transform: translateY(-1px); }
  .cat-item.active, .plat-item.active { border-color: var(--accent); background: rgba(88, 166, 255, 0.1); color: var(--text-bright); font-weight: 600; }
  .cat-icon { font-size: 22px; margin-bottom: 6px; }
  .cat-name { font-size: 13px; }
  .uri-preview { background: #000; border: 1px dashed var(--accent); border-radius: 8px; padding: 12px 16px; margin: 16px 0; display: flex; align-items: center; justify-content: space-between; }
  .uri-text { font-family: monospace; font-size: 15px; color: var(--green); font-weight: 600; }
  .form-group { margin-bottom: 16px; }
  label { display: block; font-size: 13px; color: var(--text-dim); margin-bottom: 6px; font-weight: 600; }
  input, select { width: 100%; background: #0d1117; border: 1px solid var(--card-border); border-radius: 8px; padding: 10px 14px; color: var(--text-bright); font-size: 14px; outline: none; transition: border-color 0.2s; }
  input:focus { border-color: var(--accent); }
  .btn-submit { width: 100%; background: #238636; border: none; border-radius: 8px; color: #fff; padding: 12px; font-size: 15px; font-weight: 600; cursor: pointer; transition: background 0.2s; margin-top: 10px; }
  .btn-submit:hover { background: #2ea043; }
  .secret-table { width: 100%; border-collapse: collapse; font-size: 13px; }
  .secret-table th, .secret-table td { padding: 12px; text-align: left; border-bottom: 1px solid var(--card-border); }
  .secret-table th { color: var(--text-dim); font-weight: 600; }
  .btn-copy { background: transparent; border: 1px solid var(--card-border); color: var(--text); padding: 4px 8px; border-radius: 6px; cursor: pointer; font-size: 12px; }
  .btn-copy:hover { border-color: var(--accent); color: var(--accent); }
  .toast { position: fixed; bottom: 24px; right: 24px; background: #238636; color: #fff; padding: 12px 20px; border-radius: 8px; font-weight: 600; display: none; box-shadow: 0 4px 12px rgba(0,0,0,0.5); }
</style>
</head>
<body>
<div class="container">
  <header>
    <div class="logo-box">
      <span class="logo">🔐</span>
      <div>
        <h1>kyvault 极客凭证向导</h1>
        <div style="font-size:12px; color:var(--text-dim); margin-top:2px;">纯本地端对端加密 · 自动规范化命名</div>
      </div>
    </div>
    <div style="display:flex; align-items:center; gap:8px;">
      <span id="store-count" class="badge">加载中...</span>
    </div>
  </header>

  <div class="nav-tabs">
    <button class="tab-btn active" onclick="switchTab('store')">➕ 智能存入向导</button>
    <button class="tab-btn" onclick="switchTab('list')">🔍 凭证检索台账</button>
  </div>

  <!-- 选项卡 1：智能存入向导 -->
  <div id="tab-store" class="tab-content">
    <div class="card">
      <div class="step-title"><span>Step 1</span> 选择凭据大类</div>
      <div id="category-grid" class="category-grid"></div>

      <div class="step-title"><span>Step 2</span> 选择或指定平台</div>
      <div id="platform-grid" class="category-grid"></div>

      <div class="step-title"><span>Step 3</span> 确认用途与标准 URI</div>
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
          <span style="font-size:11px; color:var(--text-dim); display:block;">规范寻址 URI</span>
          <span id="uri-display" class="uri-text">secret://deepseek/main</span>
        </div>
        <button class="btn-copy" onclick="copyText(document.getElementById('uri-display').innerText)">复制 URI</button>
      </div>

      <div class="step-title"><span>Step 4</span> 输入安全目标值</div>
      <div class="form-group">
        <label>密钥明文 (写入磁盘前进行 AES-256-GCM 军事级加密)</label>
        <div style="display:flex; gap:8px;">
          <input id="input-value" type="password" placeholder="粘贴密钥、Token 或密码...">
          <button class="btn-copy" onclick="togglePassword()">👁️</button>
        </div>
      </div>
      <div class="form-group">
        <label>绑定账号 / 备注信息 (可选)</label>
        <input id="input-account" placeholder="如 admin@gmail.com 或 生产集群Token">
      </div>

      <button class="btn-submit" onclick="submitSecret()">🔒 安全加密存入 kyvault</button>
    </div>
  </div>

  <!-- 选项卡 2：凭据检索台账 -->
  <div id="tab-list" class="tab-content" style="display:none;">
    <div class="card">
      <div style="margin-bottom:16px; display:flex; justify-content:space-between; align-items:center;">
        <input id="search-input" placeholder="🔍 搜索平台、别名或分类..." oninput="filterSecrets()" style="max-width:300px;">
        <button class="btn-copy" onclick="loadSecrets()">🔄 刷新列表</button>
      </div>
      <table class="secret-table">
        <thead>
          <tr>
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
let gCategories = [];
let gCurrentCat = null;
let gCurrentPlat = null;
let gSecrets = [];

async function init() {
  const catRes = await fetch('/api/categories');
  gCategories = await catRes.json();
  renderCategories();
  selectCategory(gCategories[0].id);

  const statusRes = await fetch('/api/status');
  const status = await statusRes.json();
  document.getElementById('store-count').innerText = `${status.secrets_count} 条密钥已就绪`;
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
    <div class="plat-item ${gCurrentPlat?.id === p.id ? 'active' : ''}" onclick="selectPlatform('${p.id}')">
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
  document.querySelectorAll('.plat-item').forEach(el => el.classList.remove('active'));
  event?.currentTarget?.classList.add('active');

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

  if (!name || !val) {
    alert('请填写名称和密钥值！');
    return;
  }

  const res = await fetch('/api/set', {
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
    alert('保存失败：' + data.error);
  }
}

async function loadSecrets() {
  const res = await fetch('/api/secrets');
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
      <td><span style="font-weight:600; color:var(--text-bright);">${s.ref}</span></td>
      <td><span class="badge" style="background:#1f2937; color:#8b949e;">${s.kind || '-'}</span></td>
      <td><code>...${s.last4 || '****'}</code></td>
      <td style="color:var(--text-dim);">${s.account || '-'}</td>
      <td>
        <button class="btn-copy" onclick="revealSecret('${s.ref}')">复制明文</button>
      </td>
    </tr>
  `).join('');
}

async function revealSecret(uri) {
  const res = await fetch('/api/reveal', {
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
