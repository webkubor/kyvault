// Pass fixed UI copy (or a function returning its current translation), never vault data.
import { getLang } from './i18n.js';

const items = new Map();
const limit = 3;
let announced;
const text = (zh, en) => getLang() === 'en' ? en : zh;
const host = () => document.getElementById('toasts');

function place() {
  const node = host();
  if (!node) return;
  const modal = document.activeElement?.closest('dialog:modal') || [...document.querySelectorAll('dialog:modal')].at(-1);
  const parent = modal || document.body;
  const live = document.getElementById('toast-live');
  if (node.parentElement !== parent && live) parent.append(node, live);
}
document.addEventListener('close', place, true);

function remove(item) {
  clearTimeout(item.timer);
  const focused = item.node.contains(document.activeElement);
  item.node.remove();
  items.delete(item.id);
  if (announced === item) {
    document.getElementById('toast-live').textContent = '';
    announced = null;
  }
  if (focused) {
    const next = host()?.querySelector('button');
    const target = item.origin?.isConnected && !item.origin.disabled ? item.origin : next;
    target?.focus({preventScroll:true});
  }
}

function schedule(item) {
  clearTimeout(item.timer);
  if (item.kind !== 'success' || item.node.matches(':hover, :focus-within')) return;
  item.timer = setTimeout(() => remove(item), 5000);
}

function paint(item) {
  const value = typeof item.message === 'function' ? item.message() : item.message;
  item.caption.textContent = value;
  item.node.dataset.kind = item.kind;
  item.symbol.textContent = {success:'✓', error:'!', pending:'…'}[item.kind];
  item.close.setAttribute('aria-label', text('关闭通知', 'Dismiss notification'));
  item.retryButton.textContent = text('重试', 'Retry');
  item.retryButton.hidden = item.kind !== 'error' || typeof item.retry !== 'function';
  return value;
}

function build(id) {
  const node = document.createElement('div');
  node.className = 'ky-toast';
  const symbol = document.createElement('span');
  symbol.className = 'toast-symbol';
  symbol.setAttribute('aria-hidden', 'true');
  const caption = document.createElement('span');
  caption.className = 'toast-caption';
  const retryButton = document.createElement('button');
  const close = document.createElement('button');
  close.className = 'toast-close';
  close.textContent = '×';
  close.type = retryButton.type = 'button';
  node.append(symbol, caption, retryButton, close);
  const item = {id,node,symbol,caption,close,retryButton,origin:document.activeElement};
  close.onclick = () => remove(item);
  retryButton.onclick = () => {
    const retry = item.retry;
    remove(item);
    retry();
  };
  node.onkeydown = e => {
    if (e.key !== 'Escape') return;
    e.stopPropagation();
    remove(item);
  };
  node.onmouseenter = node.onfocusin = () => clearTimeout(item.timer);
  node.onmouseleave = () => schedule(item);
  node.onfocusout = () => queueMicrotask(() => { if (node.isConnected) schedule(item); });
  return item;
}

export function notifyToast(message, {kind='success', id=message, announce=true, retry}={}) {
  if (!host() || document.querySelector('#locked[open]')) return;
  let item = items.get(id);
  if (kind === 'cancel' || kind === 'cancelled') {
    if (item) remove(item);
    return;
  }
  if (!message || !['success','error','pending'].includes(kind)) return;
  place();
  if (!item) {
    if (items.size >= limit) {
      const oldest = [...items.values()].find(entry => !entry.node.contains(document.activeElement));
      if (oldest) remove(oldest);
    }
    item = build(id);
    items.set(id,item);
    host().append(item.node);
  }
  Object.assign(item, {kind,message,retry});
  const value = paint(item);
  // Existing form alerts, the footer and the account status keep their own announcer.
  if (announce) {
    const live = document.getElementById('toast-live');
    if (live) {
      if (live.textContent !== value) live.textContent = value;
      announced = item;
    }
  }
  schedule(item);
}

export function refreshToasts() {
  for (const item of items.values()) paint(item);
}

export function clearToasts() {
  for (const item of items.values()) {
    clearTimeout(item.timer);
    item.node.remove();
  }
  items.clear();
  announced = null;
  const live = document.getElementById('toast-live');
  if (live) live.textContent = '';
}
