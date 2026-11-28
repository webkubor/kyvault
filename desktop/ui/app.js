// Ky desktop UI wiring. ES modules; helpers in core.js, rendering in
// render.js, strings in i18n.js, language glue in lang.js.
// Top-level await keeps __TAURI__ (mocked before load in tests) authoritative
// by the time the desktop flag is computed.
//
// Rule: every runtime string is passed as an i18n KEY (+ optional vars), never
// as finished text. On a language switch the keys are re-rendered, so no stale
// copy survives anywhere.

const { messages, state, $, $$, setStatus, snapshotReady, setError, setFormError, setConfirmText, invoke, listen, isComposing, readPref, writePref } = await import('./core.js');
const { setView, renderList, renderDetail, renderNoBridge, relocalizeDynamic, closeDetail, revealEntry } = await import('./render.js');
const { t, applyLanguage, syncLanguageFromSnapshot, currentLang, LANG_KEY, onLanguageChanged } = await import('./lang.js');

const workspaceViews = new Set(['private', 'drafts', 'shares', 'feedback']);
const { openWorkspace, closeWorkspace, canLeaveWorkspace } = await import('./workspace.js');
const { renderHome, clearHome } = await import('./home.js');
const { renderAccount, clearAccount } = await import('./account.js');
const { installControls } = await import('./controls.js');
installControls();
let epoch = 0;
let busy = false;
let captureErrorShown = false;
// 运行时判定，而不是导入期定值：Tauri v2 的 withGlobalTauri 注入的是
// window.__TAURI__，但 core 子对象是惰性填充的 —— 模块顶层 await 执行时
// core 可能还没挂上。那时候把 desktop 定成 false，会让下面整块
// `if (desktop)` 的弹框注册与轮询兜底全都不执行，Agent 的录入请求就永远
// 不会弹框（2026-11-12 实测：请求 pending、后端读得到、前端不弹）。
const desktop = Boolean(window.__TAURI__ && window.__TAURI__.core);

function wipeForms() {
  $('#secret-form').reset();
  $('#agent-request-form').reset();
}

function lockView() {
  clearHome();
  clearAccount();
  closeWorkspace();
  $('#vault-platform').replaceChildren();
  $('#vault-platform-row').hidden = true;
  epoch++;
  state.locked = true;
  state.snapshot = null;
  state.entry = null;
  state.selectedId = null;
  state.filter = '';
  $('#search').value = '';
  $('#entries').replaceChildren();
  $('#detail').replaceChildren();
  $('#detail').hidden = true;
  wipeForms();
  $$('dialog').forEach((d) => { if (d.open) d.close(); });
  $('#locked').showModal();
  setStatus('status.hidden');
}
async function refresh() {
  if (!desktop) return;
  const generation = epoch;
  try {
    const snapshot = await invoke('snapshot');
    if (generation !== epoch) return;
    if (snapshot.locked) return lockView();
    if (state.locked) return;
    if (messages.error === 'err.load') setError(null);
    renderResult(snapshot);
  } catch {
    if (generation !== epoch || state.locked) return;
    setError('err.load', null, refresh);
    setStatus('status.unavailable');
  }
}
function renderResult(snapshot) {
  state.snapshot = snapshot;
  if (state.view === 'home') renderHome(snapshot);
  $('#monitoring').checked = Boolean(snapshot.monitoring);
  $('#language').value = currentLang();
  renderList();
  renderDetail();
  if (!state.locked) snapshotReady();
}
async function action(command, args, messageKey, errorTarget) {
  if (busy || state.locked) return false;
  busy = true;
  const generation = epoch;
  setError(null);
  try {
    const result = await invoke(command, args);
    if (generation !== epoch) return false;
    // Only snapshot-shaped results redraw the library (set_monitoring returns
    // a bool, copy_* return void).
    if (result && typeof result === 'object' && Array.isArray(result.entries)) {
      if (result.locked) return lockView();
      renderResult(result);
    }
    setStatus(messageKey);
    return true;
  } catch {
    if (errorTarget) setFormError(errorTarget, 'err.generic');
    else setError('err.generic');
    return false;
  } finally { busy = false; }
}
function confirm(titleKey, copyKey, labelKey, vars) {
  const dialog = $('#confirm');
  setConfirmText({ titleKey, copyKey, labelKey, vars });
  dialog.returnValue = '';
  dialog.showModal();
  return new Promise((resolve) => {
    dialog.addEventListener('close', () => resolve(dialog.returnValue === 'yes' && !state.locked), { once: true });
  });
}
$('#confirm-action').addEventListener('click', () => $('#confirm').close('yes'));
$('#confirm-cancel').addEventListener('click', () => $('#confirm').close('no'));
$('#locked').addEventListener('cancel', (e) => e.preventDefault());

$$('.close').forEach((button) => button.addEventListener('click', () => button.closest('dialog').close()));
$('#credential').addEventListener('close', wipeForms);
$('#add').addEventListener('click', () => {
  // 添加按钮只在密钥页存在（添加凭证）。
  if (state.view !== 'vault') return;
  wipeForms();
  setFormError('#secret-error', null);
  resetCredentialForm();
  $('#credential').showModal();
});
// —— Agent 请求录入：下拉数据来自后端校验表，提交走「先验证后入库」。 ——
const PLATFORMS = [
  ['github', '🐙 GitHub'], ['gitlab', '🦊 GitLab'], ['openai', '🤖 OpenAI'],
  ['anthropic', '🟠 Anthropic'], ['deepseek', '🐋 DeepSeek'], ['moonshot', '🌙 Moonshot'],
  ['zhipu', '💡 智谱'], ['qwen', '🧵 通义'], ['doubao', '🌱 豆包'], ['gemini', '✨ Gemini'],
  ['groq', '⚡ Groq'], ['openrouter', '🛣️ OpenRouter'], ['siliconflow', '🌫️ SiliconFlow'],
  ['together', '🧩 Together'], ['fireworks', '🎆 Fireworks'], ['baichuan', '🐴 百川'],
  ['spark', '✨ 讯飞'], ['minimax', '📉 MiniMax'], ['aliyun', '☁️ 阿里云'],
  ['cloudflare', '🧱 Cloudflare'], ['feishu', '🐦 飞书'], ['xiaomi', '🌙 Xiaomi MiMo Token Plan'],
];
let agentRequest = null;

function resetCredentialForm() {
  const select = $('#credential-platforms');
  if (!select.options.length) {
    for (const [id, label] of [['apple','Apple'], ['alipay','支付宝'], ['amap','高德地图'], ...PLATFORMS]) {
      const opt = document.createElement('option');
      opt.value = id;
      opt.textContent = label;
      select.appendChild(opt);
    }
  }
  $('#cred-request-note').hidden = true;
  updateCredentialHint();
  const advanced = $('#credential .cred-advanced');
  if (advanced) advanced.open = false;
}

function updateCredentialHint() {
  const password = $('#credential-type').value === 'Password';
  $('#cred-verify-hint').textContent = t(password ? 'cred.local' : 'cred.value.hint');
  $('label[for="secret-value"]').textContent = t(password ? 'cred.password' : 'cred.value.label');
  $('#secret-form button[type="submit"]').textContent = t(password ? 'cred.savePassword' : 'action.saveCredential');
  $('#secret-value').setAttribute('aria-describedby', 'cred-verify-hint');
}
$('#credential-type').addEventListener('change', updateCredentialHint);
$('#secret-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  if (busy || state.locked) return;
  const platform = $('#secret-platform').value.trim().toLowerCase();
  const kind = $('#credential-type').value;
  // 名称留空自动填 main —— 用户不必理解"引用名"这个概念，选平台+粘 Key 就能存。
  const name = ($('#secret-name').value.trim().toLowerCase() || (kind === 'Password' ? 'password' : 'main'));
  const account = $('#secret-account').value.trim();
  const alias = $('#secret-alias').value.trim().toLowerCase();
  const value = $('#secret-value').value;
  setFormError('#secret-error', null);
  const platformValid = /^[\p{L}\p{N}][\p{L}\p{N}._-]*$/u.test(platform);
  if (!platformValid || !/^[a-z0-9][a-z0-9._-]*$/.test(name)) {
    const field = $(platformValid ? '#secret-name' : '#secret-platform');
    if (field.closest('details')) field.closest('details').open = true;
    setFormError('#secret-error', 'cred.err.name', undefined, field);
    field.focus();
    return;
  }
  const reference = `secret://${platform}/${name}`;
  try {
    const snapshot = await invoke('snapshot');
    if (state.locked || snapshot.locked) return lockView();
    if (snapshot.secrets.some((s) => s.reference === reference)) {
      const ok = await confirm('confirm.replace.title', 'confirm.replace.copy', 'confirm.replace.label', { ref: reference });
      if (!ok) return;
    }
  } catch { return setFormError('#secret-error', 'cred.err.check'); }
  busy = true;
  try {
    await invoke('save_secret_with_meta', {
      reference,
      value,
      kind,
      account: account || null,
      alias: alias || null,
    });
    setStatus('status.credSaved');
    $('#credential').close();
    await refresh();
  } catch (e) {
    // 透传后端的具体失败原因（如校验端点报错），别只给通用文案让用户猜。
    const detail = typeof e === 'string' ? e : (e && e.message) || '';
    setFormError('#secret-error', detail ? 'err.backend' : 'err.generic', detail ? { detail } : undefined);
  } finally { busy = false; }
});

function openAgentRequest(request) {
  agentRequest = request;
  const isXiaomiPlan = request.platform === 'xiaomi';
  $('#agent-request-meta').textContent = t(isXiaomiPlan ? 'req.meta.xiaomi' : 'req.meta', {
    platform: request.platform,
    name: request.name,
    who: request.requested_by || 'agent',
  }) + (request.note ? `\n${request.note}` : '');
  $('#agent-request-hint').textContent = t(isXiaomiPlan ? 'req.paste.xiaomi' : 'req.paste.hint');
  $('#agent-request-value').value = '';
  setFormError('#agent-request-error', null);
  $('#agent-request').showModal();
  $('#agent-request-value').focus();
}
$('#agent-request-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  // 锁定/忙碌时的录入尝试必须给出反馈 —— 静默吞掉会让人以为「存不进去」。
  if (!agentRequest) return;
  if (state.locked) {
    setFormError('#agent-request-error', 'req.err.locked');
    return;
  }
  if (busy) return;
  const value = $('#agent-request-value').value;
  setFormError('#agent-request-error', null);
  busy = true;
  try {
    await invoke('fulfill_request', { id: agentRequest.id, value });
    setStatus('status.reqSaved');
    $('#agent-request').close();
    agentRequest = null;
    await refresh();
  } catch (e) {
    // 透传后端校验失败的具体原因；请求仍在待处理，值保留在框里供重试。
    const detail = typeof e === 'string' ? e : (e && e.message) || '';
    setFormError('#agent-request-error', detail ? 'err.backend' : 'req.err.invalid', detail ? { detail } : undefined);
  } finally { busy = false; }
});
$('#agent-request-discard').addEventListener('click', async () => {
  if (!agentRequest) return;
  try { await invoke('discard_request', { id: agentRequest.id }); } catch {}
  $('#agent-request').close();
  agentRequest = null;
});
for (const id of ['agent-request']) $('#' + id).addEventListener('close', () => { agentRequest = null; });
// 录入框里的键盘输入计入「活动」，但保留后端 5 分钟空闲锁。
// 如果用户暂停过久，锁定会关闭弹窗并清除未保存内容；待请求保持在 inbox，
// 解锁后上面的轮询会重新打开，用户可再次粘贴，不能静默丢掉请求。
$('#agent-request-value').addEventListener('input', () => {
  if (state.locked || !desktop) return;
  invoke('touch_activity').catch(() => {});
});
// 用户粘贴完整密钥后直接走保存流程，不要求再点「校验并保存」。
// 延迟到 paste 默认动作完成后提交，确保表单读到刚粘贴的值。
$('#agent-request-value').addEventListener('paste', () => {
  setTimeout(() => {
    if (!busy && agentRequest && $('#agent-request').open) {
      $('#agent-request-form').requestSubmit();
    }
  }, 0);
});

// Agent 通过 `ky request declare --open` 唤起 Ky.app 时，后端把 id 转成事件。
// 锁定时先记下，解锁后兜底弹出 —— 不能因为“当时锁着”就丢掉 Agent 的请求。
// 事件可能在监听器就绪前发出（冷启动只发一次），所以再加轮询兜底：
// 每 5 秒查一次 inbox，发现 pending 请求且弹窗没开就补弹。
let pendingRequestId = null;
function tryOpenAgentRequest() {
  if (!pendingRequestId || state.locked) return;
  const id = pendingRequestId;
  invoke('pending_requests')
    .then((items) => {
      const hit = items.find((r) => r.id === id);
      if (hit && !$('#agent-request').open) openAgentRequest(hit);
    })
    .catch(() => {})
    .finally(() => { pendingRequestId = null; });
}
if (desktop) {
  listen('request-credential', (event) => {
    const id = typeof event.payload === 'string' ? event.payload : event.payload?.id;
    if (!id) return;
    pendingRequestId = id;
    tryOpenAgentRequest();
  });
  // 后端后台检测到新版 → 侧栏版本按钮显示小红点 + 记住版本号。
  listen('update-available', (event) => {
    const { version } = event.payload || {};
    if (!version) return;
    availableVersion = version;
    setUpdateDot(true);
    const label = $('#version-label');
    if (label) label.textContent = t('about.hasUpdate');
  });
  // 用户点"更新"后的下载/安装进度显示到状态条。
  listen('update-progress', (event) => {
    const { phase, version } = event.payload || {};
    if (phase === 'downloading') {
      setStatus('status.updateDownloading', { version: version || '' });
    } else if (phase === 'installed') {
      setStatus('status.updateInstalled', { version: version || '' });
    }
  });
  // 无条件兜底：直接问后端要 inbox 里的 pending 请求并弹框。
  // 不依赖事件、不依赖 pendingRequestId —— 事件在冷启动时可能早于监听器
  // 就发出去（single-instance 只转一次 argv），而 pendingRequestId 只在
  // 事件路径里赋值。两条路都断时，请求会一直 pending 却没人看见。
  async function openFirstPendingRequest() {
    if (state.locked || $('#agent-request').open) return;
    try {
      const items = await invoke('pending_requests');
      if (!Array.isArray(items) || items.length === 0) return;
      const hit = items[0];
      if (hit && !$('#agent-request').open) openAgentRequest(hit);
    } catch (e) {
      // 静默吞掉会让「请求 pending 但没人看见」变得无法排查 —— 至少留个状态。
      setStatus('status.reqPending');
    }
  }
  openFirstPendingRequest();
  // 事件丢失兜底：轮询拿到 pending 请求就补弹（幂等 —— 弹窗开着就不重复开）。
  setInterval(async () => {
    if (state.locked || $('#agent-request').open || pendingRequestId) return;
    try {
      const items = await invoke('pending_requests');
      const open = items.find((r) => !$('#agent-request').open);
      if (open) openAgentRequest(open);
    } catch {}
  }, 5000);
}

$('#detail').addEventListener('click', async (e) => {
  const command = e.target.closest('[data-action]')?.dataset.action;
  if (!command) return;
  if (command === 'close-detail') return closeDetail();
  const entry = state.entry;
  if (!entry || state.locked) return;
  const { id, reference } = entry;
  if (command === 'reveal-entry') {
    revealEntry(entry);
    const btn = e.target.closest('[data-action]');
    if (btn) btn.remove();
    return;
  }
  if (command === 'copy-entry') return action('copy_entry', { id }, 'status.entryCopied');
  if (command === 'copy-reference') return action('copy_reference', { reference }, 'status.refCopied');
  if (command === 'show-secret') {
    try { await invoke('show_secret', { reference }); } catch { setError('err.generic'); }
    return;
  }
  if (command === 'copy-secret') {
    if (await confirm('confirm.copySecret.title', 'confirm.copySecret.copy', 'confirm.copySecret.label')) {
      await action('copy_secret', { reference }, 'status.secretCopied');
    }
  }
  if (command === 'delete-secret' && await confirm('confirm.deleteSecret.title', 'confirm.deleteSecret.copy', 'confirm.deleteSecret.label', { ref: reference })) {
    await action('delete_secret', { reference }, 'status.secretDeleted');
  }
  if (command === 'delete-entry' && await confirm('confirm.deleteEntry.title', 'confirm.deleteEntry.copy', 'confirm.deleteEntry.label')) {
    await action('delete_entry', { id }, 'status.entryDeleted');
  }
});
async function navigate(view, id) {
  if (state.locked || state.view === view) return;
  if (workspaceViews.has(state.view) && !(await canLeaveWorkspace())) return;
  if (state.locked) return;
  closeWorkspace();
  clearAccount();
  clearHome();
  setView(view);
  if (view === 'home') renderHome(state.snapshot);
  if (view === 'account') renderAccount();
  if (workspaceViews.has(view)) await openWorkspace(view, id);
}
$$('.nav-item').forEach(button => button.addEventListener('click', () => navigate(button.dataset.view)));
window.addEventListener('ky-navigate', event => {
  const view = event.detail?.view;
  if ($$('.nav-item').some(button => button.dataset.view === view)) navigate(view);
});
window.addEventListener('workspace-navigate', event => {
  if (workspaceViews.has(event.detail?.view)) navigate(event.detail.view, event.detail.id);
});

// ---- 版本与更新日志（关于对话框）----
let availableVersion = null; // 后端检测到新版时填入
function setUpdateDot(on) {
  const dot = $('#update-dot');
  if (dot) dot.hidden = !on;
}

// 把发布说明渲染成可点击展开的列表（每条标题可点开 GitHub release 看全文）。
function renderReleaseNotes(notes) {
  const box = $('#release-notes');
  if (!box) return;
  box.replaceChildren();
  if (!notes || !notes.length) {
    const p = el('p', 'hint');
    p.textContent = t('about.noNotes');
    box.appendChild(p);
    return;
  }
  for (const n of notes) {
    const row = el('details', 'release-row');
    const sum = el('summary', '', `v${n.version} — ${n.name || n.published_at.slice(0, 10)}`);
    row.appendChild(sum);
    const body = el('pre', 'release-body', n.body || t('about.noDetail'));
    row.appendChild(body);
    if (n.url) {
      const a = el('a', 'release-link', t('about.openRelease'));
      a.href = n.url;
      a.target = '_blank';
      a.rel = 'noopener';
      row.appendChild(a);
    }
    box.appendChild(row);
  }
}

async function loadAbout() {
  const verLabel = $('#about-version');
  const status = $('#update-status');
  const detail = $('#update-detail');
  if (verLabel) verLabel.textContent = '…';
  if (status) status.textContent = t('about.loading');
  if (detail) detail.hidden = true;
  try {
    const info = await invoke('get_app_info');
    if (verLabel) verLabel.textContent = `v${info.current}`;
    renderReleaseNotes(info.notes);
    if (status) status.textContent = '';
    if (availableVersion) {
      if (detail) detail.hidden = false;
      const txt = $('#update-available-text');
      if (txt) txt.textContent = t('about.newAvailable', { version: availableVersion });
    }
  } catch {
    if (status) status.textContent = t('about.loadFailed');
  }
}

$('#about').addEventListener('click', () => {
  const dialog = document.querySelector('dialog#about-dialog');
  if (dialog) dialog.showModal();
  loadAbout();
});

// 检查更新按钮：主动拉一次检测（后台也在跑，这里即时反馈），带加载动画。
$('#check-update').addEventListener('click', async () => {
  const btn = $('#check-update');
  const status = $('#update-status');
  if (btn) { btn.disabled = true; btn.classList.add('loading'); }
  if (status) status.textContent = t('about.checking');
  try {
    // get_app_info 会重新拉 releases；版本检测仍由后台 check-only 事件给出。
    await invoke('get_app_info');
    if (status) status.textContent = availableVersion
      ? t('about.newAvailable', { version: availableVersion })
      : t('about.upToDate');
  } catch {
    if (status) status.textContent = t('about.checkFailed');
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('loading'); }
  }
});

// 更新按钮：用户主动触发下载安装，显示加载。
$('#do-update').addEventListener('click', async () => {
  const btn = $('#do-update');
  const status = $('#update-status');
  if (btn) { btn.disabled = true; btn.classList.add('loading'); }
  if (status) status.textContent = t('about.downloading');
  try {
    await invoke('install_update');
  } catch (e) {
    if (status) status.textContent = typeof e === 'string' ? e : t('about.updateFailed');
    if (btn) { btn.disabled = false; btn.classList.remove('loading'); }
  }
});
function filterCollection() {
  state.filter = $('#search').value;
  state.platform = $('#vault-platform').value;
  renderList();
  if (state.selectedId && !$$('.entry').some(row => row.dataset.entryKey === state.selectedId)) closeDetail();
}
$('#search').addEventListener('input', filterCollection);
$('#vault-platform').addEventListener('change', filterCollection);
$('#capture').addEventListener('click', async () => {
  captureErrorShown = false;
  const ok = await action('capture_clipboard', {}, 'status.captured');
  if (!ok && !captureErrorShown) setError('err.generic');
});
$('#monitoring').addEventListener('change', async (e) => {
  const input = e.target;
  input.disabled = true;
  await action('set_monitoring', { enabled: input.checked }, input.checked ? 'status.monitorOn' : 'status.monitorOff');
  input.checked = Boolean(state.snapshot?.monitoring);
  input.disabled = false;
});
$('#clear').addEventListener('click', async () => {
  if (await confirm('confirm.clear.title', 'confirm.clear.copy', 'confirm.clear.label')) {
    await action('clear_history', {}, 'status.historyCleared');
  }
});
$('#lock').addEventListener('click', async () => {
  try { await invoke('lock_app'); lockView(); }
  catch { setError('err.lock'); }
});
$('#unlock').addEventListener('click', async () => {
  try {
    await invoke('unlock_app');
    state.locked = false;
    $('#locked').close();
    setFormError('#unlock-error', null);
    await refresh();
    if (!state.locked && state.view === 'account') renderAccount();
    if (!state.locked && workspaceViews.has(state.view)) await openWorkspace(state.view);
    setStatus('status.ready');
    // 锁定期间到达的 Agent 录入请求，现在补弹。
    tryOpenAgentRequest();
  } catch { setFormError('#unlock-error', 'locked.err'); }
});

// Collapsible toolbar (chrome, not content).
$('#collapse-toolbar').addEventListener('click', () => {
  const collapsed = $('main').classList.toggle('collapsed');
  const button = $('#collapse-toolbar');
  button.setAttribute('aria-expanded', String(!collapsed));
  button.dataset.i18nAria = collapsed ? 'toolbar.expand' : 'toolbar.collapse';
  button.setAttribute('aria-label', t(collapsed ? 'toolbar.expand' : 'toolbar.collapse'));
  writePref('ky.toolbarCollapsed', collapsed ? '1' : '0');
});

// Language switch: mirror to localStorage + backend (set_ui_language), then
// re-render every dynamic string in the new language.
$('#language').addEventListener('change', (e) => {
  applyLanguage(e.target.value);
});

onLanguageChanged(() => {
  relocalizeDynamic();
  if ($('#credential').open) updateCredentialHint();
  $('#collapse-toolbar').setAttribute(
    'aria-label',
    t($('main').classList.contains('collapsed') ? 'toolbar.expand' : 'toolbar.collapse')
  );
});

document.addEventListener('keydown', (e) => {
  if (isComposing(e)) return;
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f' && !document.querySelector('dialog[open]') && state.view !== 'settings') {
    e.preventDefault();
    const input = workspaceViews.has(state.view) ? $('#workspace input[type=search]') : $('#search');
    input?.focus();
  }
  if (e.key === 'Escape' && state.selectedId && !document.querySelector('dialog[open]')) {
    closeDetail();
  }
});

// Init language: localStorage mirror first (instant paint), backend snapshot
// is the source of truth and reconciles right after (set_ui_language).
applyLanguage(readPref(LANG_KEY, null) || 'zh-CN', { persist: true, notifyBackend: false });
if (readPref('ky.toolbarCollapsed', '0') === '1') {
  $('main').classList.add('collapsed');
  $('#collapse-toolbar').setAttribute('aria-expanded', 'false');
}
setView('home');
renderHome(state.snapshot);

if (desktop) {
  let lastTouch = 0;
  const touchActivity = () => {
    if (state.locked) return;
    const now = Date.now();
    if (now - lastTouch < 15000) return;
    lastTouch = now;
    invoke('touch_activity', {}).catch(() => {});
  };
  document.addEventListener('keydown', touchActivity);
  document.addEventListener('pointerdown', touchActivity);
  Promise.all([
    listen('history-changed', () => { if (!document.hidden) refresh(); }),
    listen('app-locked', lockView),
    listen('capture-error', () => {
      // Show a real error instead of silently pretending the capture worked.
      // The backend pauses monitoring when this fires.
      captureErrorShown = true;
      refresh().then(() => setError('err.capture'));
    })
  ]).catch(() => setError('err.live'));
  document.addEventListener('visibilitychange', () => { if (!document.hidden && !state.locked) refresh(); });
  refresh().then(async () => {
    if (state.locked) return;
    const snapshot = await invoke('snapshot').catch(() => null);
    if (snapshot && snapshot.lang) syncLanguageFromSnapshot(snapshot);
  });
} else {
  renderNoBridge();
  setStatus('status.openKy');
  $$('button:not(.nav-item), #monitoring, #language').forEach((node) => { node.disabled = true; });
}
