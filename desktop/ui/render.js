// List + detail rendering for the Ky desktop UI. All user-visible strings
// come from the i18n dictionary via t().
//
// Layout: one column. The list fills the column; selecting an entry opens a
// detail sheet that slides over it. There is no permanent second pane.

import { state, $, $$, el, showEl, invoke, formatWhen, firstLine, setError, setStatus, relocalizeMessages } from './core.js';
import { t } from './lang.js';

export const VIEW_META = {
  history: 'view.history',
  vault: 'view.vault',
  snippets: 'view.snippets',
  settings: 'view.settings',
};

// The view heading derives from the view, so it can be re-rendered whenever
// the language changes.
export function renderViewHeading() {
  const key = VIEW_META[state.view] || 'view.history';
  $('#view-title').textContent = t(`${key}.title`);
}

export function renderAddLabel() {
  const add = $('#add');
  // 粘贴历史里「新增文本」多余：这个页面只做「点条目→复制」。
  // 新增只在片段/凭证页需要。
  if (state.view === 'history') {
    add.hidden = true;
    return;
  }
  add.hidden = false;
  if (state.view === 'vault') add.textContent = t('action.add.credential');
  else add.textContent = t('action.add.snippet');
}

export function renderMonitorState() {
  const on = Boolean(state.snapshot && state.snapshot.monitoring);
  const box = $('#monitoring');
  if (box && state.snapshot) box.checked = on;
  $('#capture').hidden = state.view !== 'history';
}

// Re-render every dynamic surface that can carry language-dependent text.
export function relocalizeDynamic() {
  relocalizeMessages();
  renderViewHeading();
  renderAddLabel();
  renderMonitorState();
  renderList();
  renderDetail();
  const editor = $('#editor');
  if (editor.open) {
    $('#text-submit').textContent = $('#redaction-preview').hidden
      ? t('action.review')
      : t('action.saveReviewed');
    $('#editor-title').textContent = t('editor.title');
  }
  const credential = $('#credential');
  if (credential.open) $('#credential-title').textContent = t('cred.title');
}

export function isSecretView() {
  return state.view === 'vault';
}

export function currentEntries() {
  if (!state.snapshot) return [];
  switch (state.view) {
    case 'history':
      return state.snapshot.entries;
    case 'snippets':
      return state.snapshot.entries.filter((e) => e.snippet);
    case 'vault':
      return state.snapshot.secrets || [];
    default:
      return [];
  }
}

export function entryKey(entry) {
  return isSecretView() ? `ref:${entry.reference}` : `id:${entry.id}`;
}

export function entryTitle(e) {
  if (e.snippet) return e.snippet;
  return firstLine(e.text) || (e.redacted ? t('entry.redacted') : t('entry.text'));
}

function entrySubtitle(e) {
  return (e.text || '').replace(/\s+/g, ' ').trim() || '';
}

function matched(entries) {
  const q = state.filter.trim().toLowerCase();
  if (!q) return entries;
  return entries.filter((e) => {
    const title = isSecretView() ? e.reference : entryTitle(e);
    const subtitle = isSecretView() ? e.platform || '' : entrySubtitle(e);
    return title.toLowerCase().includes(q) || subtitle.toLowerCase().includes(q);
  });
}

function emptyTitle() {
  switch (state.view) {
    case 'history': return t('empty.history.title');
    case 'snippets': return t('empty.snippets.title');
    case 'vault': return t('empty.vault.title');
    default: return t('empty.nomatch.title');
  }
}

function emptyCopy() {
  switch (state.view) {
    case 'history': return t('empty.history.copy');
    case 'snippets': return t('empty.snippets.copy');
    case 'vault': return t('empty.vault.copy');
    default: return t('empty.nomatch.copy');
  }
}

export function renderList() {
  const list = $('#entries');
  const entries = currentEntries();
  const filtered = matched(entries);
  list.replaceChildren();
  showEl($('#empty'), filtered.length === 0);

  if (!state.snapshot) {
    $('#empty-title').textContent = t('loading.title');
    $('#empty-copy').textContent = t('loading.copy');
    return;
  }
  if (entries.length === 0) {
    $('#empty-title').textContent = emptyTitle();
    $('#empty-copy').textContent = emptyCopy();
    return;
  }
  if (filtered.length === 0) {
    $('#empty-title').textContent = t('empty.nomatch.title');
    $('#empty-copy').textContent = t('empty.nomatch.copy');
    return;
  }
  filtered.forEach((entry) => list.appendChild(renderEntry(entry)));
}

function renderEntry(entry) {
  const wrap = el('div', 'entry-wrap');
  const btn = el('button', 'entry');
  btn.type = 'button';
  if (isSecretView()) {
    btn.appendChild(el('span', 'entry-title', entry.reference));
    const meta = el('span', 'entry-meta');
    if (entry.platform) meta.appendChild(el('span', '', entry.platform));
    meta.appendChild(el('span', 'badge', t('badge.credential')));
    btn.appendChild(meta);
  } else {
    btn.appendChild(el('span', 'entry-title', entryTitle(entry)));
    const meta = el('span', 'entry-meta');
    meta.appendChild(el('span', '', formatWhen(entry.created_at)));
    if (entry.source) meta.appendChild(el('span', 'entry-source', t('entry.source', { app: entry.source })));
    if (entry.redacted) meta.appendChild(el('span', 'badge', t('badge.redacted')));
    if (entry.snippet) meta.appendChild(el('span', 'badge', `v${entry.version || 1}`));
    btn.appendChild(meta);
    const sub = entrySubtitle(entry);
    if (sub && sub !== entryTitle(entry)) btn.appendChild(el('span', 'entry-subtitle', sub));
  }
  btn.dataset.entryKey = entryKey(entry);
  btn.setAttribute('aria-pressed', String(entryKey(entry) === state.selectedId));
  if (isSecretView()) {
    // 密钥视图：点条目打开详情（显示引用、复制引用）。
    btn.addEventListener('click', () => select(entry));
  } else {
    // 历史/片段：核心动作是「点一下就复制到剪贴板」，一步到位。
    btn.addEventListener('click', async () => {
      try {
        await invoke('copy_entry', { id: entry.id });
        setStatus('status.entryCopied');
      } catch {
        setError('err.generic');
      }
    });
    // 详情（显示原文/存为片段/删除）收进二级入口，不干扰主动作。
    const more = el('button', 'entry-more');
    more.type = 'button';
    more.textContent = '⋯';
    more.setAttribute('aria-label', t('action.entryDetail'));
    more.title = t('action.entryDetail');
    more.addEventListener('click', () => select(entry));
    wrap.appendChild(btn);
    wrap.appendChild(more);
    return wrap;
  }
  wrap.appendChild(btn);
  return wrap;
}

export function setView(view) {
  state.view = view;
  state.selectedId = null;
  state.entry = null;
  state.filter = '';
  $('#search').value = '';
  showEl($('#add'), view !== 'settings');
  showEl($('#search-row'), view !== 'settings');
  showEl($('#library'), view !== 'settings');
  showEl($('#settings'), view === 'settings');
  $$('.nav-item').forEach((b) =>
    b.setAttribute('aria-current', b.dataset.view === view ? 'page' : 'false')
  );
  renderViewHeading();
  renderAddLabel();
  renderMonitorState();
  renderList();
  renderDetail();
}

function select(entry) {
  state.selectedId = entryKey(entry);
  state.entry = entry;
  renderList();
  renderDetail();
}

function selectedEntry() {
  if (!state.snapshot) return null;
  return currentEntries().find((e) => entryKey(e) === state.selectedId) || null;
}

export function closeDetail() {
  state.selectedId = null;
  state.entry = null;
  renderList();
  renderDetail();
}

/* ---------- detail sheet ---------- */

export function renderDetail() {
  const pane = $('#detail');
  const entry = selectedEntry();
  if (state.view === 'settings' || !entry) {
    pane.replaceChildren();
    showEl(pane, false);
    return;
  }
  pane.replaceChildren();

  const head = el('div', 'detail-head');
  const back = el('button', 'icon');
  back.type = 'button';
  back.dataset.action = 'close-detail';
  back.setAttribute('aria-label', t('action.back'));
  back.appendChild(el('span', 'back-icon', ''));
  head.appendChild(back);
  const title = el('h2', null, isSecretView() ? entry.reference : entryTitle(entry));
  head.appendChild(title);
  pane.appendChild(head);

  if (isSecretView()) {
    pane.appendChild(el('p', 'detail-note', t('preview.secret.note')));
    pane.appendChild(secretActions(entry));
  } else {
    const meta = el('p', 'detail-note', t('preview.saved', { time: formatWhen(entry.created_at) }));
    pane.appendChild(meta);
    const body = el('div', 'detail-body', '…');
    pane.appendChild(body);
    if (entry.redacted) {
      // 敏感条目默认打码：不主动把原文放进 DOM。用户点"显示原文"才取。
      const reveal = el('button', '', t('action.reveal'));
      reveal.type = 'button';
      reveal.dataset.action = 'reveal-entry';
      pane.appendChild(reveal);
      body.textContent = t('preview.masked');
    }
    pane.appendChild(entryActions(entry));
    if (!entry.redacted) loadDetailText(entry, body);
  }
  showEl(pane, true);
}

function secretActions(entry) {
  const actions = el('div', 'actions');
  const copyRef = el('button', '', t('action.copyReference'));
  copyRef.type = 'button';
  copyRef.dataset.action = 'copy-reference';
  actions.appendChild(copyRef);
  const copyVal = el('button', '', t('action.copySecretValue'));
  copyVal.type = 'button';
  copyVal.dataset.action = 'copy-secret';
  actions.appendChild(copyVal);
  const remove = el('button', 'quiet danger', t('action.remove'));
  remove.type = 'button';
  remove.dataset.action = 'delete-secret';
  actions.appendChild(remove);
  return actions;
}

function entryActions(entry) {
  const actions = el('div', 'actions');
  const copyBtn = el('button', 'primary', t('action.copy'));
  copyBtn.type = 'button';
  copyBtn.dataset.action = 'copy-entry';
  actions.appendChild(copyBtn);

  if (entry.snippet) {
    const editBtn = el('button', '', t('action.editVersion'));
    editBtn.type = 'button';
    editBtn.dataset.action = 'edit-snippet';
    actions.appendChild(editBtn);
    const restoreBtn = el('button', '', t('action.restore'));
    restoreBtn.type = 'button';
    restoreBtn.dataset.action = 'restore-entry';
    actions.appendChild(restoreBtn);
  } else {
    const saveBtn = el('button', '', t('action.saveAsSnippet'));
    saveBtn.type = 'button';
    saveBtn.dataset.action = 'save-as-snippet';
    actions.appendChild(saveBtn);
  }
  const removeBtn = el('button', 'quiet danger', t('action.remove'));
  removeBtn.type = 'button';
  removeBtn.dataset.action = 'delete-entry';
  actions.appendChild(removeBtn);
  return actions;
}

async function loadDetailText(entry, body, reveal = false) {
  try {
    const res = await invoke('preview_text', { text: entry.text, reveal });
    body.textContent = res && typeof res.text === 'string' ? res.text : entry.text;
  } catch {
    body.textContent = entry.text;
    setError('err.generic');
  }
}

// 导出给 app.js 的事件委托：用户点"显示原文"时调用，取回并展示原文。
export function revealEntry(entry) {
  const body = document.querySelector('#detail .detail-body');
  if (body) loadDetailText(entry, body, true);
}

export function renderNoBridge() {
  state.snapshot = null;
  renderList();
  $('#empty-title').textContent = t('desktopOnly.title');
  $('#empty-copy').textContent = t('desktopOnly.copy');
}
