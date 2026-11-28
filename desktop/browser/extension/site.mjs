export function webpage(raw) {
  try {
    const url = new URL(raw);
    if (['http:', 'https:'].includes(url.protocol)) return url;
  } catch {}
  return null;
}

export const siteKey = url => 'auto:' + url.host;
export const permission = url => ({origins: ['*://' + url.hostname + '/*']});

export async function enabled(url) {
  const key = siteKey(url);
  return (await chrome.storage.local.get(key))[key] === true &&
    await chrome.permissions.contains(permission(url));
}
