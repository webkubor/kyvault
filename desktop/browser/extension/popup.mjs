import {webpage, siteKey, permission, enabled} from './site.mjs';
const button = document.querySelector('button');
const status = document.querySelector('#status');
const [tab] = await chrome.tabs.query({active: true, currentWindow: true});
const url = webpage(tab?.url);
if (!url || tab.incognito) {
  status.textContent = '请在普通网页中打开。';
} else {
  document.querySelector('#host').textContent = url.host;
  const key = siteKey(url);
  let on;
  async function render() {
    on = await enabled(url);
    button.textContent = on ? '关闭复制自动收藏' : '启用复制自动收藏';
    button.disabled = false;
  }
  await render();
  button.onclick = async () => {
    button.disabled = true;
    status.textContent = '';
    try {
      if (on) {
        await chrome.storage.local.remove(key);
        const keys = Object.keys(await chrome.storage.local.get(null));
        if (!keys.some(k => k.startsWith('auto:') && webpage('https://' + k.slice(5))?.hostname === url.hostname))
          await chrome.permissions.remove(permission(url));
      } else if (await chrome.permissions.request(permission(url))) {
        await chrome.storage.local.set({[key]: true});
      } else {
        status.textContent = '未获得网站权限。';
      }
    } catch {
      status.textContent = '设置失败，请重新打开弹窗再试。';
    } finally {
      await render();
    }
  };
}
