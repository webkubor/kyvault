import {webpage, enabled, permission} from './site.mjs';
import {attachCopy} from './listener.mjs';

const menu = 'ky-flash';
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => chrome.contextMenus.create({
    id: menu, title: '收进 Ky 闪记', contexts: ['selection'],
    documentUrlPatterns: ['http://*/*', 'https://*/*']
  }));
});

async function save(text, url, tabId) {
  if (!url || typeof text !== 'string' || !text.trim()) return false;
  let ok = false;
  try {
    const reply = await chrome.runtime.sendNativeMessage('com.kyvault.flash', {
      text, url: url.origin + url.pathname
    });
    ok = reply?.ok === true;
  } catch {}
  await chrome.action.setBadgeText({tabId, text: ok ? '' : '!'}).catch(() => {});
  await chrome.action.setTitle({tabId, title: ok ? 'Ky 闪记' : '收藏失败，请检查 Ky 和本机连接配置'}).catch(() => {});
  return ok;
}

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === menu && tab && !tab.incognito && !info.editable &&
      webpage(info.pageUrl)) void save(info.selectionText, webpage(info.frameUrl ?? info.pageUrl), tab.id);
});

async function attach(tab) {
  const url = webpage(tab.url);
  if (tab.incognito || !url || !await enabled(url)) return;
  await chrome.scripting.executeScript({
    target: {tabId: tab.id}, func: attachCopy, args: [url.host]
  });
}

// tabs supplies URLs only when the browser has already granted access.
chrome.tabs.onUpdated.addListener((id, change, tab) => {
  if (change.status === 'loading' || change.status === 'complete')
    void attach(tab).catch(() => {}); // Tab may have navigated or closed.
});

async function refreshTabs(host) {
  const tabs = await chrome.tabs.query({});
  await Promise.allSettled(tabs.filter(tab => webpage(tab.url)?.host === host).map(attach));
}

chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== 'local') return;
  for (const [key, change] of Object.entries(changes)) {
    if (key.startsWith('auto:') && change.newValue === true) void refreshTabs(key.slice(5));
  }
});

chrome.permissions.onRemoved.addListener(async () => {
  for (const key of Object.keys(await chrome.storage.local.get(null))) {
    if (!key.startsWith('auto:')) continue;
    const url = webpage('https://' + key.slice(5));
    if (url && !await chrome.permissions.contains(permission(url))) await chrome.storage.local.remove(key);
  }
});

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  const url = webpage(sender.url);
  if (message?.type !== 'copy' || sender.id !== chrome.runtime.id || !sender.tab ||
      sender.tab.incognito || sender.frameId !== 0 || !url || webpage(sender.tab.url)?.host !== url.host) return;
  enabled(url).then(on => on && save(message.text, url, sender.tab.id))
    .then(ok => respond({ok}), () => respond({ok: false}));
  return true;
});
