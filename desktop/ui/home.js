import { el, invoke } from './core.js';
import { getLang } from './i18n.js';
import { onLanguageChanged } from './lang.js';

const copy = {
  title: ['私人工具工作台', 'Personal workbench'],
  vault: ['查密码', 'Find a password'],
  private: ['存私密图文', 'Save a private note'],
  drafts: ['分享想法', 'Share an idea'],
  feedback: ['查看反馈', 'View feedback'],
  account: ['仓库连接', 'Repository connection'],
  secrets: ['已存凭据', 'Credentials'],
  notes: ['私密收藏', 'Private entries'],
  ideas: ['想法草稿', 'Idea drafts'],
  pending: ['正在读取', 'Loading'],
  missing: ['数量不可用', 'Count unavailable'],
};
const cards = [
  ['vault', 'secrets'], ['private', 'notes'], ['drafts', 'ideas'],
];
const paths = {
  vault: 'M14 4a5 5 0 1 1-1.2 9.85L7 20H3v-4l6.15-5.8A5 5 0 0 1 14 4ZM16 7.8h.01',
  private: 'M5 3.5h14a1.5 1.5 0 0 1 1.5 1.5v14a1.5 1.5 0 0 1-1.5 1.5H5A1.5 1.5 0 0 1 3.5 19V5A1.5 1.5 0 0 1 5 3.5ZM4 16l5-5 4 4 3-3 4 4M15.5 8h.01',
  drafts: 'm3 10 18-7-6.5 18-4-7.5L3 10Zm7.5 3.5L21 3',
  feedback: 'M4 4.5h16v12H9l-5 4v-16ZM8 9h8m-8 4h5',
  account: 'M9 15l6-6m-5-2 2-2a4.2 4.2 0 0 1 6 6l-2 2m-2 4-2 2a4.2 4.2 0 0 1-6-6l2-2',
};
let page;
let revision = 0;

function icon(view) {
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  const path = document.createElementNS(ns, 'path');
  path.setAttribute('d', paths[view]);
  svg.append(path);
  return svg;
}

function label(tag, cls, key) {
  const node = el(tag, cls);
  node.dataset.homeLabel = key;
  return node;
}

function button(view, cls) {
  const node = el('button', cls);
  node.type = 'button';
  node.addEventListener('click', () => {
    window.dispatchEvent(new CustomEvent('ky-navigate', { detail: { view } }));
  });
  return node;
}

function mount(root) {
  const panel = el('div', 'ky-home-panel');
  const hero = el('header', 'ky-home-hero');
  const art = el('span', 'ky-home-art');
  art.setAttribute('aria-hidden', 'true');
  art.append(el('i'), el('i'), el('i', 'ky-home-mark'));
  hero.append(label('h1', '', 'title'), art);
  const grid = el('div', 'ky-home-grid');
  const counts = {};
  for (const [view, metric] of cards) {
    const card = button(view, 'ky-home-card');
    const top = el('span', 'ky-home-card-top');
    const symbol = el('span', 'ky-home-symbol');
    symbol.append(icon(view));
    const arrow = el('span', 'ky-home-arrow', '↗');
    arrow.setAttribute('aria-hidden', 'true');
    top.append(symbol, arrow);
    const stat = el('span', 'ky-home-stat');
    counts[view] = el('strong');
    stat.append(counts[view], label('span', '', metric));
    card.append(top, label('span', 'ky-home-action', view), stat);
    grid.append(card);
  }
  const links = el('div', 'ky-home-links');
  for (const view of ['feedback', 'account']) {
    const link = button(view, 'ky-home-link');
    link.append(icon(view), label('span', '', view));
    links.append(link);
  }
  panel.append(hero, grid, links);
  root.replaceChildren(panel);
  return { root, counts };
}

function translate() {
  if (!page) return;
  const index = getLang() === 'en' ? 1 : 0;
  for (const node of page.root.querySelectorAll('[data-home-label]')) {
    node.textContent = copy[node.dataset.homeLabel][index];
  }
  for (const node of Object.values(page.counts)) {
    if (node.dataset.status) node.setAttribute('aria-label', copy[node.dataset.status][index]);
  }
}
onLanguageChanged(translate);

function count(node, rows, pending = false) {
  if (Array.isArray(rows)) {
    node.textContent = String(rows.length);
    delete node.dataset.status;
    node.removeAttribute('aria-label');
  } else {
    node.dataset.status = pending ? 'pending' : 'missing';
    node.textContent = pending ? '…' : '—';
    node.setAttribute('aria-label', copy[node.dataset.status][getLang() === 'en' ? 1 : 0]);
  }
}

export async function renderHome(snapshot) {
  const root = document.getElementById('home');
  if (!root) return;
  const entering = page?.root !== root;
  if (entering) page = mount(root);
  root.hidden = false;
  translate();
  count(page.counts.vault, snapshot?.secrets);
  // Clipboard updates refresh the backend snapshot while the home is visible.
  // The document counts only change through workspace views; fetch on entry.
  if (!entering) return;
  const request = ++revision;
  await Promise.all(['private', 'drafts'].map(async kind => {
    const node = page.counts[kind];
    count(node, null, true);
    let rows;
    try { rows = await invoke('workspace_list', { kind }); }
    catch { /* An unavailable list has no known count. */ }
    if (request === revision) count(node, rows);
  }));
}

export function clearHome() {
  ++revision;
  const root = document.getElementById('home');
  if (root) {
    root.hidden = true;
    root.replaceChildren();
  }
  page = undefined;
}

// WKWebView can keep the page mounted while the main window is hidden.
document.addEventListener('visibilitychange', () => {
  document.documentElement.classList.toggle('ky-window-hidden', document.hidden);
});
