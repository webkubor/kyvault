// List + preview rendering for the Ky desktop UI. All user-visible strings
// come from the i18n dictionary via t().

import { state, $, $$, el, showEl, invoke, formatWhen, firstLine, setError, relocalizeMessages } from './core.js';
import { t } from './lang.js';

export const VIEW_META = {
  history: 'view.history',
  vault: 'view.vault',
  snippets: 'view.snippets',
  settings: 'view.settings',
};

// The view heading + description derive from the view, so they can be
// re-rendered whenever the language changes.
export function renderViewHeading() {
  const key = VIEW_META[state.view] || 'view.history';
  $('#view-title').textContent = t(`${key}.title`);
  $('#view-description').textContent = t(`${key}.desc`);
}

export function renderAddLabel() {
  const add = $('#add');
  if (state.view === 'vault') add.textContent = t('action.add.credential');
  else if (state.view === 'snippets') add.textContent = t('action.add.snippet');
  else add.textContent = t('action.add.text');
}

export function renderMonitorState() {
  const on = Boolean(state.snapshot && state.snapshot.monitoring);
  $('#monitor-state').textContent = on ? t('monitor.on') : t('monitor.off');
  const box = $('#monitoring');
  if (box && state.snapshot) box.checked = on;
}

// Re-render every dynamic surface that can carry language-dependent text.
export function relocalizeDynamic() {
  relocalizeMessages();
  renderViewHeading();
  renderAddLabel();
  renderMonitorState();
  renderList();
  renderPreview();
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

function setEmptyState(title, copy) {
  $('#empty-title').textContent = title;
  $('#empty-copy').textContent = copy;
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
  $('#count').textContent = entries.length;
  $('#list-label').textContent = isSecretView() ? t('list.label.refs') : t('list.label.text');

  const hasAny = entries.length > 0;
  const hasMatch = filtered.length > 0;
  showEl($('#empty'), !hasMatch);

  if (!state.snapshot) {
    setEmptyState(t('loading.title'), t('loading.copy'));
    return;
  }
  if (!hasAny) {
    setEmptyState(emptyTitle(), emptyCopy());
    return;
  }
  if (!hasMatch) {
    setEmptyState(t('empty.nomatch.title'), t('empty.nomatch.copy'));
    return;
  }
  filtered.forEach((entry) => list.appendChild(renderEntry(entry)));
}

function renderEntry(entry) {
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
    if (entry.redacted) meta.appendChild(el('span', 'badge', t('badge.redacted')));
    if (entry.snippet) meta.appendChild(el('span', 'badge', `v${entry.version || 1}`));
    btn.appendChild(meta);
    btn.appendChild(el('span', 'entry-subtitle', entrySubtitle(entry)));
  }
  btn.dataset.entryKey = entryKey(entry);
  btn.setAttribute('aria-pressed', String(entryKey(entry) === state.selectedId));
  btn.addEventListener('click', () => select(entry));
  return btn;
}

export function setView(view) {
  state.view = view;
  state.selectedId = null;
  state.filter = '';
  $('#search').value = '';
  showEl($('#add'), view !== 'settings');
  showEl($('#capture'), view === 'history');
  showEl($('#search'), view !== 'settings');
  showEl($('#library'), view !== 'settings');
  showEl($('#settings'), view === 'settings');
  $$('.nav-item').forEach((b) =>
    b.setAttribute('aria-current', b.dataset.view === view ? 'page' : 'false')
  );
  renderViewHeading();
  renderAddLabel();
  renderList();
  renderPreview();
}

function select(entry) {
  state.selectedId = entryKey(entry);
  $$('.entry').forEach((b) =>
    b.setAttribute('aria-pressed', String(b.dataset.entryKey === state.selectedId))
  );
  renderList();
  renderPreview();
}

function selectedEntry() {
  if (!state.snapshot) return null;
  return currentEntries().find((e) => entryKey(e) === state.selectedId) || null;
}

/* ---------- preview ---------- */

export function renderPreview() {
  const pane = $('#preview');
  if (state.view === 'settings' || !state.selectedId) {
    pane.replaceChildren();
    renderPlaceholder();
    return;
  }
  const entry = selectedEntry();
  if (!entry) {
    pane.replaceChildren();
    renderPlaceholder();
    return;
  }
  state.entry = entry;
  if (isSecretView()) {
    renderSecretPreview(entry);
    return;
  }

  pane.replaceChildren();
  const heading = el('h2', 'entry-title', entryTitle(entry));
  heading.style.whiteSpace = 'normal';
  pane.appendChild(heading);

  const meta = el('p', 'entry-subtitle', t('preview.saved', { time: formatWhen(entry.created_at) }));
  pane.appendChild(meta);

  const code = el('pre', null, '…');
  pane.appendChild(code);

  const actions = el('div', 'actions');
  const copyBtn = el('button', '', t('action.copy'));
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
  pane.appendChild(actions);

  showEl($('#add'), false);
  loadPreviewText(entry, code);
}

function renderSecretPreview(entry) {
  const pane = $('#preview');
  pane.replaceChildren();
  const heading = el('h2', null, entry.reference);
  heading.style.overflowWrap = 'anywhere';
  pane.appendChild(heading);

  const meta = el('p', 'entry-subtitle', t('preview.credMeta', { platform: entry.platform || 'vault' }));
  pane.appendChild(meta);

  pane.appendChild(el('p', 'muted', t('preview.secret.note')));

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
  pane.appendChild(actions);

  showEl($('#add'), false);
}

async function loadPreviewText(entry, code) {
  try {
    const res = await invoke('preview_text', { text: entry.text });
    code.textContent = res && typeof res.text === 'string' ? res.text : entry.text;
  } catch (err) {
    if (err && err.message === 'no-bridge') {
      code.textContent = entry.text;
      return;
    }
    code.textContent = entry.text;
    setError('err.generic');
  }
}

function renderPlaceholder() {
  const pane = $('#preview');
  const box = el('div', 'preview-placeholder');
  box.appendChild(el('span', 'eyebrow', t('ph.eyebrow')));
  const body = el('p');
  body.append(t('ph.line1'), document.createElement('br'), t('ph.line2'));
  box.appendChild(body);
  box.appendChild(el('span', '', isSecretView() ? t('ph.hint.vault') : t('ph.hint.text')));
  pane.replaceChildren(box);
}

export function renderNoBridge() {
  state.snapshot = null;
  renderList();
  $('#empty-title').textContent = t('desktopOnly.title');
  $('#empty-copy').textContent = t('desktopOnly.copy');
  const pane = $('#preview');
  const box = el('div', 'preview-placeholder');
  box.appendChild(el('span', 'eyebrow', 'KY'));
  box.appendChild(el('p', null, t('desktopOnly.title')));
  box.appendChild(el('span', '', t('desktopOnly.copy')));
  pane.replaceChildren(box);
}
