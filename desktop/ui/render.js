// List + detail rendering for the Ky desktop UI. All user-visible strings
// come from the i18n dictionary via t().
//
// Narrow windows use a detail sheet; the wide vault view keeps the list visible.

import { state, $, $$, el, showEl, invoke, formatWhen, firstLine, setError, setStatus, relocalizeMessages } from './core.js';
import { t } from './lang.js';

import { platformMark } from './platform-icon.js';
import { credentialTitle, matchesCredential, platformLabel } from './vault.js';

export const VIEW_META = {
  history: 'view.history',
  vault: 'view.vault',
  settings: 'view.settings',
};

// The view heading derives from the view, so it can be re-rendered whenever
// the language changes.
export function renderViewHeading() {
  const key = VIEW_META[state.view] || 'view.history';
  const search = $('#search');
  search.placeholder = t(isSecretView() ? 'vault.search' : 'search.placeholder');
  search.setAttribute('aria-label', search.placeholder);
  $('#view-title').textContent = VIEW_META[state.view] ? t(`${key}.title`) : t('nav.' + state.view);
  for (const item of $$('.nav-item')) item.title = t('nav.' + item.dataset.view);
}

export function renderAddLabel() {
  const add = $('#add');
  add.hidden = state.view !== 'vault';
  if (!add.hidden) add.textContent = t('action.add.credential');
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
    case 'vault':
      return state.snapshot.secrets || [];
    default:
      return [];
  }
}

export function entryKey(entry) {
  return isSecretView() ? `ref:${entry.reference}` : `id:${entry.id}`;
}

/// 字节数 → 人类可读（"1.2 MB"）。图片条目的体积标注。
function formatSize(bytes) {
  if (!bytes) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function entryTitle(e) {
  if (e.image) return t('entry.imageAlt');
  if (e.snippet) return e.snippet;
  return firstLine(e.text) || (e.redacted ? t('entry.redacted') : t('entry.text'));
}

function entrySubtitle(e) {
  return (e.text || '').replace(/\s+/g, ' ').trim() || '';
}

function matched(entries) {
  if (isSecretView() && state.platform) entries = entries.filter(entry => entry.platform === state.platform);
  const q = state.filter.trim().toLowerCase();
  if (!q) return entries;
  if (isSecretView()) return entries.filter(entry => matchesCredential(entry, q));
  return entries.filter((e) => {
    const title = entryTitle(e);
    const subtitle = entrySubtitle(e);
    return title.toLowerCase().includes(q) || subtitle.toLowerCase().includes(q);
  });
}

function emptyTitle() {
  switch (state.view) {
    case 'history': return t('empty.history.title');
    case 'vault': return t('empty.vault.title');
    default: return t('empty.nomatch.title');
  }
}

function emptyCopy() {
  switch (state.view) {
    case 'history': return t('empty.history.copy');
    case 'vault': return t('empty.vault.copy');
    default: return t('empty.nomatch.copy');
  }
}

export function renderList() {
  const list = $('#entries');
  const entries = currentEntries();
  const filter = $('#vault-platform');
  showEl($('#vault-platform-row'), isSecretView() && entries.length > 0);
  if (isSecretView()) {
    const ids = [...new Set(entries.map(entry => entry.platform))].sort();
    const label = id => platformLabel(id, state.lang);
    const options = [new Option(t('vault.all'), ''), ...ids.map(id => {
      const option = new Option(label(id), id);
      option.dataset.platform = id;
      return option;
    })];
    filter.replaceChildren(...options);
    filter.value = state.platform || '';
    if (filter.selectedIndex < 0) state.platform = filter.value = '';
  }
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
    btn.appendChild(platformMark(entry.platform));
    btn.appendChild(el('span', 'entry-title', credentialTitle(entry, state.lang)));
    const meta = el('span', 'entry-meta');
    meta.appendChild(el('span', '', [entry.account, ...(entry.aliases || []), entry.name].filter(Boolean).join(' · ')));
    btn.appendChild(meta);
  } else if (entry.image) {
    // 图片条目：缩略图 + 尺寸信息；点击复制（与文本一致，主动作在监听器里）。
    const thumb = el('img', 'entry-thumb');
    thumb.alt = t('entry.imageAlt');
    thumb.loading = 'lazy';
    invoke('get_entry_image', { id: entry.id })
      .then((res) => {
        if (res && typeof res.png === 'string') thumb.src = `data:image/png;base64,${res.png}`;
      })
      .catch(() => {});
    btn.appendChild(thumb);
    const meta = el('span', 'entry-meta');
    meta.appendChild(el('span', '', `${entry.image.width}×${entry.image.height}`));
    meta.appendChild(el('span', '', formatSize(entry.image.png_len)));
    if (entry.source) meta.appendChild(el('span', 'entry-source', t('entry.source', { app: entry.source })));
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
    if (!entry.image && !entry.redacted && state.view === 'history') {
      const save = el('button', 'quiet', t('flash.collect'));
      save.onclick = async () => {
        save.disabled = true;
        try { await invoke('flash_history', {id:entry.id}); setStatus('flash.saved'); }
        catch { setError('err.generic'); }
        finally { save.disabled = false; }
      };
      wrap.appendChild(save);
    }
    wrap.appendChild(more);
    return wrap;
  }
  wrap.appendChild(btn);
  return wrap;
}

export function setView(view) {
  state.view = view;
  const collection = view === 'history' || view === 'vault';
  const workspace = ['private','drafts','shares','feedback'].includes(view);
  showEl($('#home'), view === 'home');
  showEl($('#account'), view === 'account');
  showEl($('#toolbar'), view !== 'home');
  $('main').dataset.view = view;
  showEl($('#collection-panes'), collection);
  showEl($('#workspace'), workspace);
  state.selectedId = null;
  state.entry = null;
  state.filter = '';
  state.platform = '';
  $('#search').value = '';
  showEl($('#add'), view !== 'settings');
  showEl($('#search-row'), collection);
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
  const target = $('#detail [data-action="show-secret"]') || $('#detail .detail-head button');
  target?.focus({ preventScroll: true });
}

function selectedEntry() {
  if (!state.snapshot) return null;
  return currentEntries().find((e) => entryKey(e) === state.selectedId) || null;
}

export function closeDetail() {
  const prior = state.selectedId;
  state.selectedId = null;
  state.entry = null;
  renderList();
  renderDetail();
  const source = $$('.entry').find(button => button.dataset.entryKey === prior);
  source?.focus();
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
  const title = el('h2', null, isSecretView() ? credentialTitle(entry, state.lang) : entryTitle(entry));
  if (isSecretView()) head.appendChild(platformMark(entry.platform));
  head.appendChild(title);
  pane.appendChild(head);

  if (isSecretView()) {
    const metadata = el('dl', 'vault-fields');
    for (const [label, value] of [['vault.platform', platformLabel(entry.platform, state.lang)], ['vault.account', entry.account], ['vault.reference', entry.reference]]) {
      if (value) metadata.append(el('dt', '', t(label)), el('dd', '', value));
    }
    const field = el('div', 'vault-value');
    field.append(el('span', 'value-label', t('vault.value')), el('span', 'masked-value', '••••　••••　••••'));
    if (state.snapshot.native_reveal) {
      const reveal = el('button', 'primary', t('vault.show'));
      reveal.type = 'button';
      reveal.dataset.action = 'show-secret';
      field.append(reveal);
    }
    pane.append(field, metadata, secretActions());
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

function secretActions() {
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
