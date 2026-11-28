import { $, el, formField, invoke, state } from './core.js';
import { onLanguageChanged } from './lang.js';
import { notifyToast } from './toast.js';
import { writeShareCard, pasteDestinations } from './workspace-card-actions.js';
import { w } from './workspace-copy.js';
import { offlinePage, parseFeedback, originalFromFile, sharePng, validImage, validOriginal } from './workspace-files.js';

const views = ['private', 'drafts', 'shares', 'feedback'];
let current;
let view;
let confirmLeave;
const searches = new Map();

function label(tag, key, cls) {
  const node = el(tag, cls, w(key));
  node.dataset.workspaceLabel = key;
  return node;
}
onLanguageChanged(() => {
  $('#workspace')?.querySelectorAll('[data-workspace-label]').forEach(node => {
    node.textContent = w(node.dataset.workspaceLabel);
  });
});

export function workspaceHasPendingWork() { return Boolean(current?.busy || current?.dirty); }

export function canLeaveWorkspace() {
  if (!current || state.locked) return true;
  if (current.busy) return false;
  if (!current.dirty) return true;
  return confirmAction('discard', 'discardAction');
}

function confirmAction(question, action) {
  if (confirmLeave) return confirmLeave.promise;
  const dialog = el('dialog', 'wk-confirm');
  const yes = label('button', action, 'primary');
  const no = label('button', 'cancel');
  dialog.setAttribute('aria-label', w(question));
  dialog.append(label('p', question), no, yes);
  $('#workspace').append(dialog);
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  const finish = answer => {
    dialog.close();
    dialog.remove();
    confirmLeave = null;
    resolve(answer);
  };
  confirmLeave = { promise, finish };
  yes.onclick = () => finish(true);
  no.onclick = () => finish(false);
  dialog.oncancel = event => { event.preventDefault(); finish(false); };
  dialog.showModal();
  no.focus();
  return promise;
}

export function closeWorkspace() {
  confirmLeave?.finish(false);
  current = null;
  view = null;
  searches.clear();
  const root = $('#workspace');
  if (!root) return;
  root.replaceChildren();
  root.hidden = true;
}

export async function openWorkspace(next, id, privateSourceId) {
  closeWorkspace();
  if (state.locked || !views.includes(next)) return;
  view = next;
  $('#workspace').hidden = false;
  const fromPrivate = next === 'drafts' && privateSourceId;
  if (!id && !fromPrivate) return list();
  const page = screen();
  page.panel.append(button('back', () => list()));
  message(page, 'loading');
  await run(page, async () => {
    const record = fromPrivate ? await read('private', privateSourceId) : await read(next, id);
    if (!live(page)) return;
    if (fromPrivate) editor('drafts', undefined, record);
    else detail(next, record);
  });
}

function screen() {
  const root = $('#workspace');
  const panel = el('fieldset', 'wk-panel');
  const status = el('p', 'wk-status');
  status.setAttribute('role', 'status');
  const page = { panel, status, busy: false, dirty: false };
  current = page;
  root.replaceChildren(panel, status);
  return page;
}

const live = page => current === page && !state.locked;
function message(page, key) {
  if (!live(page)) return;
  page.status.replaceChildren(...(key ? [label('span', key)] : []));
}
async function run(page, action, failure = 'error') {
  if (!live(page) || page.busy) return;
  page.busy = page.panel.disabled = true;
  message(page);
  try { await action(); }
  catch { message(page, failure); }
  finally { page.busy = page.panel.disabled = false; }
}
function button(key, action, primary = false) {
  const btn = label('button', key, primary ? 'primary' : '');
  btn.type = 'button';
  btn.onclick = action;
  return btn;
}
function bar(...children) {
  const row = el('div', 'wk-actions');
  row.append(...children);
  return row;
}
function field(key, control) {
  return formField(label('span', key), control);
}
function content(parent, record) {
  if (record.body) parent.append(el('p', 'wk-body', record.body));
  if (validOriginal(record.image)) {
    const img = el('img', 'wk-image');
    img.alt = w('image');
    img.src = record.image;
    parent.append(img);
  }
}
function shareCard(record, heading) {
  const article = el('article', 'wk-share');
  const copy = el('div', 'wk-share-copy');
  copy.append(heading);
  if (record.body) copy.append(el('p', 'wk-body', record.body));
  article.append(copy);
  if (validImage(record.image)) {
    const photo = el('img', 'wk-share-photo');
    photo.src = record.image;
    photo.alt = w('image');
    article.append(photo);
  }
  return article;
}
function shareSurface(record, heading, page, getSaved, note) {
  const section = el('section', 'wk-share-surface');
  section.append(shareActions(page, getSaved), label('p', note, 'wk-muted'), shareCard(record, heading));
  return section;
}
function read(kind, id) { return invoke('workspace_read', {kind, id}); }
function save(kind, record) {
  return invoke('workspace_save', {kind, id: record.id ?? null, title: record.title, body: record.body, image: record.image || ''});
}
async function list(pickDraft = false) {
  const page = screen();
  const kind = pickDraft ? 'drafts' : view;
  const toolbar = el('div', 'wk-actions');
  if (pickDraft) toolbar.append(button('back', () => list()), label('h2', 'chooseDraft'));
  else if (kind === 'shares') toolbar.append(button('chooseDraft', () => list(true), true));
  else if (kind === 'feedback') toolbar.append(importControl(page));
  else {
    toolbar.classList.add('wk-library-intro');
    toolbar.append(label('p', kind === 'private' ? 'privateHint' : 'draftHint'),
      button(kind === 'private' ? 'newPrivate' : 'newDraft', () => editor(kind), true));
  }
  const query = el('input');
  query.type = 'search';
  const searchKey = pickDraft ? 'pickDraft' : kind;
  query.value = searches.get(searchKey) || '';
  const rows = el('div', kind === 'shares' ? 'wk-list wk-share-list' : 'wk-list');
  const library = !pickDraft && (kind === 'private' || kind === 'drafts');
  if (library) {
    page.panel.classList.add('wk-library');
    rows.classList.add('wk-library-rows');
  }
  const searchLabel = library ? (kind === 'private' ? 'privateSearch' : 'draftSearch') : 'search';
  const searchField = field(searchLabel, query);
  if (library) {
    const box = el('span', 'wk-library-search');
    const icon = el('span', 'search-icon');
    icon.setAttribute('aria-hidden', 'true');
    box.append(icon, query);
    searchField.append(box);
  }
  page.panel.append(toolbar, searchField, rows);
  message(page, 'loading');
  try {
    const entries = await invoke('workspace_list', {kind});
    if (!live(page)) return;
    const render = () => {
      rows.replaceChildren();
      for (const entry of entries.filter(r => r.title.toLowerCase().includes(query.value.trim().toLowerCase()))) {
        const row = el('button', 'wk-row', entry.title);
        row.onclick = () => run(page, async () => {
          const record = await read(kind, entry.id);
          if (live(page)) pickDraft ? selectContent(record) : detail(kind, record);
        });
        rows.append(row);
      }
      if (!rows.childElementCount) {
        const filtered = Boolean(query.value.trim());
        rows.append(label('p', filtered ? 'noMatches' : 'empty_' + kind, 'wk-muted'));
        if (filtered) rows.append(button('clearSearch', () => {
          query.value = '';
          searches.delete(searchKey);
          render();
          query.focus();
        }));
      }
    };
    query.oninput = () => { searches.set(searchKey, query.value); render(); };
    render();
    message(page);
    if (document.activeElement === document.body) query.focus();
  } catch {
    if (live(page)) {
      message(page, 'error');
      rows.append(button('retry', () => list(pickDraft)));
    }
  }
}

function toast(key, kind = 'success') {
  notifyToast(() => w(key), {kind, id: 'workspace-' + key});
}

function shareActions(page, getSaved) {
  function action(key, perform) {
    return button(key, () => run(page, async () => {
      try {
        const snapshot = await getSaved();
        if (live(page)) await perform(snapshot);
      } catch (err) {
        if (!live(page)) return;
        message(page);
        toast(err?.message === 'cardTooLong' || err === 'cardTooLong' ? 'cardTooLong' : 'shareFailed', 'error');
      }
    }), key === 'copyCard');
  }
  async function png(snapshot, copy) {
    message(page, 'drawing');
    const done = await writeShareCard(snapshot, copy, () => live(page));
    if (!live(page)) return;
    message(page);
    if (done) toast(copy ? 'cardCopied' : 'cardSaved');
  }
  const controls = el('div', 'wk-share-actions');
  const html = action('export', async snapshot => {
    const saved = await invoke('export_workspace_file', {name: 'ky-share.html', content: offlinePage(snapshot)});
    if (live(page) && saved) toast('exported');
  });
  html.classList.add('quiet');
  controls.append(bar(action('copyCard', snapshot => png(snapshot, true)), action('savePng', snapshot => png(snapshot, false)), html), pasteDestinations(label));
  return controls;
}

function navigateWorkspace(detail) {
  window.dispatchEvent(new CustomEvent('workspace-navigate', {detail}));
}

function detail(kind, record) {
  const page = screen();
  const back = {private: 'returnPrivate', drafts: 'returnDrafts'};
  const actions = bar(button(back[kind] || 'hide', () => list()));
  const heading = el('h2', '', record.title);
  heading.tabIndex = -1;
  page.panel.append(actions, kind === 'shares' ? shareSurface(record, heading, page, () => record, 'plain') : heading);
  heading.focus();
  if (kind === 'feedback') {
    try {
      const feedback = JSON.parse(record.body);
      content(page.panel, {body: feedback.comment});
      actions.append(button('original', () => run(page, async () => {
        const original = await read('shares', feedback.shareId);
        if (live(page)) detail('shares', original);
      }, 'missingShare')));
    } catch { message(page, 'invalidFeedback'); }
    return;
  }
  if (kind === 'shares') return;
  content(page.panel, record);
  actions.append(button(kind === 'drafts' ? 'continueEdit' : 'edit', () => editor(kind, record)), button('delete', () => run(page, async () => {
    if (!await confirmAction('deleteQuestion', 'delete') || !live(page)) return;
    await invoke('workspace_delete', {kind, id: record.id});
    if (live(page)) await list();
  })));
  if (kind === 'private') actions.append(button('useAsDraft', () =>
    navigateWorkspace({view: 'drafts', privateSourceId: record.id}), true));
  if (kind === 'drafts') actions.append(button('choose', () => selectContent(record), true));
}

function editor(kind, existing, privateSource) {
  const page = screen();
  const title = el('input');
  const body = el('textarea');
  const initial = existing || privateSource;
  title.value = initial?.title || '';
  body.value = initial?.body || '';
  let image = initial?.image || null;
  const preview = el('div');
  const file = el('input');
  file.type = 'file';
  file.accept = 'image/png,image/jpeg,image/webp';
  const update = () => {
    preview.replaceChildren();
    if (image) {
      content(preview, {image});
      preview.append(button('remove', () => { image = null; page.dirty = true; update(); }));
    }
  };
  const attach = source => run(page, async () => {
    const result = await originalFromFile(source);
    if (live(page)) { image = result; page.dirty = true; update(); }
  }, 'badImage');
  file.onchange = () => {
    const source = file.files[0];
    file.value = '';
    if (source) attach(source);
  };
  page.panel.ondragover = event => { if (event.dataTransfer.types.includes('Files')) event.preventDefault(); };
  page.panel.ondrop = event => {
    event.preventDefault();
    if (event.dataTransfer.files.length) attach(event.dataTransfer.files[0]);
  };
  title.oninput = body.oninput = () => { page.dirty = true; };
  const cancel = button('cancel', async () => {
    if (!await canLeaveWorkspace() || !live(page)) return;
    page.dirty = false;
    if (privateSource) navigateWorkspace({view: 'private', id: privateSource.id});
    else if (existing) detail(kind, existing);
    else list();
  });
  const submit = button('save', () => run(page, async () => {
    const record = { id: existing?.id, title: title.value.trim(), body: body.value, image };
    if (!record.title || (!record.body.trim() && !image)) return message(page, 'invalid');
    if (new TextEncoder().encode(record.title).length > 512 || /[\u0000-\u001f\u007f-\u009f]/u.test(record.title) || new TextEncoder().encode(record.body).length > 1024 * 1024) return message(page, 'size');
    record.id = await save(kind, record);
    if (live(page)) {
      const filter = searches.get(kind)?.trim().toLowerCase();
      if (filter && !record.title.toLowerCase().includes(filter)) searches.delete(kind);
      detail(kind, record);
    }
  }), true);
  if (privateSource) page.panel.append(label('p', 'draftFromPrivate', 'wk-muted'));
  page.panel.append(field('title', title), field('body', body), label('p', 'imageHint', 'wk-muted'), field('attach', file), preview, bar(cancel, submit));
  update();
  title.focus();
}

function selectContent(draft, selected = new Set()) {
  const page = screen();
  const paragraphs = draft.body.split(/\n\s*\n/).filter(p => p.trim());
  page.panel.append(el('h2', '', draft.title));
  function option(id, text, image) {
    const wrapper = el('label', 'wk-option');
    const checkbox = el('input');
    checkbox.type = 'checkbox';
    checkbox.checked = selected.has(id);
    checkbox.onchange = () => checkbox.checked ? selected.add(id) : selected.delete(id);
    wrapper.append(checkbox);
    const sample = el('span', 'wk-body', text);
    if (image) content(sample, {image});
    wrapper.append(sample);
    page.panel.append(wrapper);
  }
  paragraphs.forEach((p, i) => option(i, p));
  if (validOriginal(draft.image)) {
    page.panel.append(label('p', 'shareImageHint', 'wk-muted'));
    option('image', w('image'), draft.image);
  }
  page.panel.append(bar(button('back', () => view === 'drafts' ? detail('drafts', draft) : list(true)), button('preview', () => run(page, async () => {
    if (!selected.size) return message(page, 'pick');
    const snapshot = {title: draft.title, body: paragraphs.filter((_, i) => selected.has(i)).join('\n\n'),
      image: selected.has('image') ? await sharePng(draft.image) : ''};
    if (!live(page)) return;
    const preview = screen();
    let id;
    async function savedSnapshot() {
      // Preserve the version id on cancellation or retry after a successful save.
      if (!id) id = await save('shares', snapshot);
      if (live(preview)) return read('shares', id);
    }
    preview.panel.append(shareSurface(snapshot, el('h2', '', snapshot.title), preview, savedSnapshot, 'autoSnapshot'),
      bar(button('back', () => selectContent(draft, selected)), button('snapshot', () => run(preview, async () => {
        const saved = await savedSnapshot();
        if (!live(preview)) return;
        detail('shares', saved);
        if (view !== 'shares') navigateWorkspace({view: 'shares', id});
      }))));
  }, 'badImage'), true)));
}

function importControl(page) {
  const file = el('input');
  file.type = 'file';
  file.accept = '.json,application/json';
  file.onchange = () => {
    const source = file.files[0];
    file.value = '';
    if (!source) return;
    run(page, async () => {
      let feedback;
      try { feedback = await parseFeedback(source); }
      catch { return message(page, 'invalidFeedback'); }
      if (!live(page)) return;
      const shares = await invoke('workspace_list', {kind: 'shares'});
      if (!live(page)) return;
      const share = shares.find(row => row.id === feedback.shareId);
      if (!share) return message(page, 'missingShare');
      const record = {title: share.title, body: JSON.stringify(feedback), image: null};
      record.id = await save('feedback', record);
      if (live(page)) { detail('feedback', record); toast('feedbackImported'); }
    });
  };
  return field('import', file);
}
