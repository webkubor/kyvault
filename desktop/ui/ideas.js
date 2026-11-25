// 灵感库视图。灵感以明文 Markdown 存在本机（后端 InspirationStore），
// 与密钥库、剪贴板历史完全分开；前端只缓存当前列表，不写 localStorage。
// 记录要低摩擦：正文在前，标题可省（后端取正文首行）。

import { state, $, el, showEl, invoke, formatWhen, setError, setStatus, setFormError, messages } from './core.js';
import { t } from './lang.js';

let ideas = null;      // null = 尚未加载；锁定时清回 null
let generation = 0;    // 丢弃过期的加载结果
let loading = false;
let noBridge = false;
let editing = null;
let saving = false;

const keyOf = (idea) => `idea:${idea.id}`;

// 隐私锁：丢掉缓存的列表和未保存的编辑内容（弹窗本身由 lockView 统一关闭）。
export function resetIdeas() {
  generation++;
  ideas = null;
  loading = false;
  editing = null;
  $('#idea-form').reset();
}

export async function loadIdeas() {
  const gen = ++generation;
  loading = true;
  try {
    const list = await invoke('list_ideas');
    if (gen !== generation || state.locked) return;
    ideas = Array.isArray(list) ? list : [];
    if (messages.error === 'err.ideas.load') setError(null);
  } catch (err) {
    if (gen !== generation) return;
    ideas = [];
    noBridge = err instanceof Error && err.message === 'no-bridge';
    if (!noBridge && !state.locked) setError('err.ideas.load');
  } finally {
    if (gen === generation) loading = false;
  }
  if (state.view === 'ideas') { renderIdeaList(); renderIdeaDetail(); }
}

function matches(idea, q) {
  return [idea.title, idea.body || '', ...(idea.tags || [])].some((s) => s.toLowerCase().includes(q));
}

function setEmpty(titleKey, copyKey) {
  $('#empty-title').textContent = t(titleKey);
  $('#empty-copy').textContent = t(copyKey);
}

export function renderIdeaList() {
  const list = $('#entries');
  list.replaceChildren();
  if (ideas === null) {
    showEl($('#empty'), true);
    setEmpty('loading.title', 'loading.copy');
    if (!loading && !state.locked) loadIdeas();
    return;
  }
  const q = state.filter.trim().toLowerCase();
  const shown = q ? ideas.filter((i) => matches(i, q)) : ideas;
  showEl($('#empty'), shown.length === 0);
  if (noBridge) return setEmpty('desktopOnly.title', 'desktopOnly.copy');
  if (!ideas.length) return setEmpty('empty.ideas.title', 'empty.ideas.copy');
  if (!shown.length) return setEmpty('empty.nomatch.title', 'empty.nomatch.copy');
  shown.forEach((idea) => list.appendChild(ideaRow(idea)));
}

function ideaRow(idea) {
  const wrap = el('div', 'entry-wrap');
  const btn = el('button', 'entry');
  btn.type = 'button';
  btn.appendChild(el('span', 'entry-title', idea.title));
  const meta = el('span', 'entry-meta');
  meta.appendChild(el('span', '', formatWhen(idea.updated_at)));
  (idea.tags || []).slice(0, 3).forEach((tag) => meta.appendChild(el('span', 'badge', `#${tag}`)));
  btn.appendChild(meta);
  const sub = (idea.body || '').replace(/\s+/g, ' ').trim();
  if (sub && sub !== idea.title) btn.appendChild(el('span', 'entry-subtitle', sub));
  btn.dataset.entryKey = keyOf(idea);
  btn.setAttribute('aria-pressed', String(keyOf(idea) === state.selectedId));
  btn.addEventListener('click', () => {
    state.selectedId = keyOf(idea);
    renderIdeaList();
    renderIdeaDetail();
  });
  wrap.appendChild(btn);
  return wrap;
}

export function renderIdeaDetail() {
  const pane = $('#detail');
  const idea = (ideas || []).find((i) => keyOf(i) === state.selectedId) || null;
  state.entry = idea;
  pane.replaceChildren();
  if (!idea) return showEl(pane, false);

  const head = el('div', 'detail-head');
  const back = el('button', 'icon');
  back.type = 'button';
  back.dataset.action = 'close-detail';
  back.setAttribute('aria-label', t('action.back'));
  back.appendChild(el('span', 'back-icon', ''));
  head.appendChild(back);
  head.appendChild(el('h2', null, idea.title));
  pane.appendChild(head);

  const note = el('p', 'detail-note', `${t('idea.detail.meta', { time: formatWhen(idea.updated_at) })} · `);
  note.appendChild(el('span', 'badge', t('badge.plain')));
  (idea.tags || []).forEach((tag) => { note.append(' '); note.appendChild(el('span', 'badge', `#${tag}`)); });
  pane.appendChild(note);
  pane.appendChild(el('div', 'detail-body', idea.body || t('idea.body.empty')));

  const actions = el('div', 'actions');
  const edit = el('button', 'primary', t('action.edit'));
  edit.type = 'button';
  edit.dataset.action = 'edit-idea';
  actions.appendChild(edit);
  const open = el('button', '', t('idea.open'));
  open.type = 'button';
  open.addEventListener('click', () => invoke('open_idea_file', { id: idea.id }).catch(() => setError('err.generic')));
  actions.appendChild(open);
  const remove = el('button', 'quiet danger', t('action.remove'));
  remove.type = 'button';
  remove.dataset.action = 'delete-idea';
  actions.appendChild(remove);
  pane.appendChild(actions);
  showEl(pane, true);
}

function renderEditorTitle() {
  $('#idea-editor-title').textContent = t(editing ? 'idea.editor.edit' : 'idea.editor.new');
}

export function relocalizeIdeas() {
  if ($('#idea-editor').open) renderEditorTitle();
}

export function openIdeaEditor(idea) {
  if (state.locked) return;
  $('#idea-form').reset();
  editing = idea || null;
  $('#idea-body').value = idea?.body || '';
  $('#idea-title').value = idea?.title || '';
  $('#idea-tags').value = (idea?.tags || []).join(', ');
  setFormError('#idea-error', null);
  renderEditorTitle();
  $('#idea-editor').showModal();
  $('#idea-body').focus();
}

export async function deleteIdea(id) {
  if (state.locked) return;
  try {
    await invoke('delete_idea', { id });
    state.selectedId = null;
    state.entry = null;
    setStatus('status.ideaDeleted');
    await loadIdeas();
  } catch {
    setError('err.generic');
  }
}

$('#idea-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  if (saving || state.locked) return;
  saving = true;
  setFormError('#idea-error', null);
  try {
    const saved = await invoke('save_idea', {
      id: editing?.id || null,
      title: $('#idea-title').value,
      body: $('#idea-body').value,
      tags: $('#idea-tags').value,
    });
    $('#idea-editor').close();
    setStatus('status.ideaSaved');
    if (saved?.id) state.selectedId = `idea:${saved.id}`;
    await loadIdeas();
  } catch (err) {
    const detail = typeof err === 'string' ? err : (err && err.message) || '';
    setFormError('#idea-error', detail ? 'err.backend' : 'err.generic', detail ? { detail } : undefined);
  } finally {
    saving = false;
  }
});
$('#idea-editor').addEventListener('close', () => {
  editing = null;
  $('#idea-form').reset();
  setFormError('#idea-error', null);
});
// ⌘/Ctrl+Enter 直接保存：记完即走。
$('#idea-form').addEventListener('keydown', (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === 'Enter' && !e.isComposing) {
    e.preventDefault();
    $('#idea-form').requestSubmit();
  }
});

$('#open-ideas-folder').addEventListener('click', () => {
  invoke('open_ideas_folder').catch(() => setError('err.generic'));
});
$('#capture-idea').addEventListener('click', async () => {
  try {
    await invoke('capture_idea_clipboard');
    setStatus('status.ideaCaptured');
    await loadIdeas();
  } catch (err) {
    setError('err.backend', { detail: typeof err === 'string' ? err : t('err.generic') });
  }
});
async function savePastedImage(file) {
  if (!file || !file.type.startsWith('image/') || state.locked) return;
  try {
    if (file.size > 8 * 1024 * 1024) throw new Error('image too large');
    // Normalize browser-supported image formats to PNG before sending to Rust.
    const bitmap = await createImageBitmap(file);
    if (bitmap.width > 12000 || bitmap.height > 12000 || bitmap.width * bitmap.height > 16000000) {
      bitmap.close();
      throw new Error('image too large');
    }
    const canvas = document.createElement('canvas');
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    canvas.getContext('2d').drawImage(bitmap, 0, 0);
    bitmap.close();
    const png = await new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
    if (!png || png.size > 8 * 1024 * 1024) throw new Error('image too large');
    const data = await new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result).split(',')[1]);
      reader.onerror = reject;
      reader.readAsDataURL(png);
    });
    await invoke('save_idea_image', { data });
    setStatus('status.ideaCaptured');
    await loadIdeas();
  } catch (err) {
    setFormError('#idea-error', 'err.backend', { detail: typeof err === 'string' ? err : t('err.generic') });
  }
}
$('#idea-body').addEventListener('paste', (event) => {
  const image = [...event.clipboardData.files].find((file) => file.type.startsWith('image/'));
  if (image) { event.preventDefault(); savePastedImage(image); }
});
$('#idea-body').addEventListener('dragover', (event) => { if (event.dataTransfer.types.includes('Files')) event.preventDefault(); });
$('#idea-body').addEventListener('drop', (event) => {
  const image = [...event.dataTransfer.files].find((file) => file.type.startsWith('image/'));
  if (image) { event.preventDefault(); savePastedImage(image); }
});
