// Ky desktop UI wiring. ES modules; helpers in core.js, rendering in
// render.js, strings in i18n.js, language glue in lang.js.
// Top-level await keeps __TAURI__ (mocked before load in tests) authoritative
// by the time the desktop flag is computed.
//
// Rule: every runtime string is passed as an i18n KEY (+ optional vars), never
// as finished text. On a language switch the keys are re-rendered, so no stale
// copy survives anywhere.

const { state, $, $$, setStatus, setError, setFormError, setConfirmText, invoke, listen, isComposing, readPref, writePref } = await import('./core.js');
const { setView, renderList, renderDetail, renderNoBridge, relocalizeDynamic, closeDetail } = await import('./render.js');
const { t, applyLanguage, syncLanguageFromSnapshot, currentLang, LANG_KEY, onLanguageChanged } = await import('./lang.js');

let epoch = 0;
let reviewed = null;
let busy = false;
let captureErrorShown = false;
const desktop = Boolean(window.__TAURI__?.core);

function wipeForms() {
  $('#text-form').reset();
  $('#secret-form').reset();
  $('#agent-request-form').reset();
  $('#redaction-text').textContent = '';
  $('#redaction-preview').hidden = true;
  reviewed = null;
}

function lockView() {
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
    renderResult(snapshot);
  } catch {
    setError('err.load');
    setStatus('status.unavailable');
  }
}
function renderResult(snapshot) {
  state.snapshot = snapshot;
  $('#monitoring').checked = Boolean(snapshot.monitoring);
  $('#language').value = currentLang();
  renderList();
  renderDetail();
  if (!state.locked) setStatus('status.ready');
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

function openEditor(entry) {
  wipeForms();
  $('#snippet-name').required = state.view === 'snippets' || Boolean(entry);
  $('#snippet-name').value = entry?.snippet || '';
  $('#text-value').value = entry?.text || '';
  $('#text-submit').textContent = t('action.review');
  setFormError('#text-error', null);
  $('#editor').showModal();
}
$('#add').addEventListener('click', () => {
  if (state.view !== 'vault') return openEditor();
  wipeForms();
  setFormError('#secret-error', null);
  $('#credential').showModal();
});
$$('.close').forEach((button) => button.addEventListener('click', () => button.closest('dialog').close()));
for (const id of ['editor', 'credential']) $('#' + id).addEventListener('close', wipeForms);
function resetReview() {
  reviewed = null;
  $('#redaction-preview').hidden = true;
  $('#redaction-text').textContent = '';
  $('#text-submit').textContent = t('action.review');
}
$('#text-value').addEventListener('input', resetReview);
$('#snippet-name').addEventListener('input', resetReview);
$('#text-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  if (busy || state.locked) return;
  const text = $('#text-value').value;
  const snippet = $('#snippet-name').value.trim() || null;
  setFormError('#text-error', null);
  if (reviewed?.source === text) {
    if (await action('save_text', { text: reviewed.text, snippet }, 'status.textSaved', '#text-error')) $('#editor').close();
    return;
  }
  busy = true;
  const generation = epoch;
  $('#text-submit').disabled = true;
  try {
    const preview = await invoke('preview_text', { text });
    if (generation !== epoch || !$('#editor').open || $('#text-value').value !== text) return;
    reviewed = { source: text, text: preview.text };
    $('#redaction-text').textContent = preview.text;
    $('#redaction-note').textContent = preview.redacted ? t('editor.note.detected') : t('editor.note.clean');
    $('#redaction-preview').hidden = false;
    $('#text-submit').textContent = t('action.saveReviewed');
  } catch { setFormError('#text-error', 'editor.err.review'); }
  finally { busy = false; $('#text-submit').disabled = false; }
});
$('#add').addEventListener('click', () => {
  if (state.view !== 'vault') return openEditor();
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
  const select = $('#secret-platform');
  if (!select.options.length) {
    for (const [id, label] of PLATFORMS) {
      const opt = document.createElement('option');
      opt.value = id;
      opt.textContent = label;
      select.appendChild(opt);
    }
  }
  $('#cred-request-note').hidden = true;
  $('#cred-verify-hint').textContent = t('cred.value.hint');
}

$('#secret-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  if (busy || state.locked) return;
  const platform = $('#secret-platform').value;
  const name = $('#secret-name').value.trim().toLowerCase();
  const account = $('#secret-account').value.trim();
  const alias = $('#secret-alias').value.trim().toLowerCase();
  const value = $('#secret-value').value;
  setFormError('#secret-error', null);
  if (!/^[a-z0-9][a-z0-9._-]*$/.test(platform) || !/^[a-z0-9][a-z0-9._-]*$/.test(name)) {
    return setFormError('#secret-error', 'cred.err.name');
  }
  const reference = `secret://${platform}/${name}`;
  try {
    const snapshot = await invoke('snapshot');
    if (state.locked || snapshot.locked) return lockView();
    if (snapshot.secrets.some((s) => s.reference === reference)) {
      const ok = await confirm('confirm.replace.title', 'confirm.replace.copy', 'confirm.replace.label', { ref: reference });
      if (!ok) return;
    }
  } catch { setFormError('#secret-error', 'cred.err.check'); }
  busy = true;
  try {
    await invoke('save_secret_with_meta', {
      reference,
      value,
      kind: null,
      account: account || null,
      alias: alias || null,
    });
    setStatus('status.credSaved');
    $('#credential').close();
    await refresh();
  } catch {
    setFormError('#secret-error', 'err.generic');
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
  if (busy || state.locked || !agentRequest) return;
  const value = $('#agent-request-value').value;
  setFormError('#agent-request-error', null);
  busy = true;
  try {
    await invoke('fulfill_request', { id: agentRequest.id, value });
    setStatus('status.reqSaved');
    $('#agent-request').close();
    agentRequest = null;
    await refresh();
  } catch {
    // 校验失败的信息在后端消息里，这里给通用提示；请求仍在待处理。
    setFormError('#agent-request-error', 'req.err.invalid');
  } finally { busy = false; }
});
$('#agent-request-discard').addEventListener('click', async () => {
  if (!agentRequest) return;
  try { await invoke('discard_request', { id: agentRequest.id }); } catch {}
  $('#agent-request').close();
  agentRequest = null;
});
for (const id of ['agent-request']) $('#' + id).addEventListener('close', () => { agentRequest = null; });

// Agent 通过 `ky request declare --open` 唤起 Ky.app 时，后端把 id 转成事件。
// 锁定时先记下，解锁后兜底弹出 —— 不能因为“当时锁着”就丢掉 Agent 的请求。
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
}

$('#detail').addEventListener('click', async (e) => {
  const command = e.target.closest('[data-action]')?.dataset.action;
  if (!command) return;
  if (command === 'close-detail') return closeDetail();
  const entry = state.entry;
  if (!entry || state.locked) return;
  const { id, reference } = entry;
  if (command === 'save-as-snippet' || command === 'edit-snippet') return openEditor(entry);
  if (command === 'copy-entry') return action('copy_entry', { id }, 'status.entryCopied');
  if (command === 'copy-reference') return action('copy_reference', { reference }, 'status.refCopied');
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
  if (command === 'restore-entry' && await confirm('confirm.restore.title', 'confirm.restore.copy', 'confirm.restore.label')) {
    await action('restore_entry', { id }, 'status.versionRestored');
  }
});
$$('.nav-item').forEach((button) => button.addEventListener('click', () => setView(button.dataset.view)));
$('#search').addEventListener('input', (e) => { state.filter = e.target.value; renderList(); });
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
  $('#collapse-toolbar').setAttribute(
    'aria-label',
    t($('main').classList.contains('collapsed') ? 'toolbar.expand' : 'toolbar.collapse')
  );
});

document.addEventListener('keydown', (e) => {
  if (isComposing(e)) return;
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f' && !document.querySelector('dialog[open]') && state.view !== 'settings') {
    e.preventDefault();
    $('#search').focus();
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
setView('history');

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
