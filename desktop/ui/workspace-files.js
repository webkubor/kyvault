import { w } from './workspace-copy.js';
import { getLang } from './i18n.js';

const prefix = 'data:image/png;base64,';
const maxPng = 1024 * 1024;
export function validImage(value) {
  return typeof value === 'string' && value.length <= prefix.length + 4 * Math.ceil(maxPng / 3)
    && ((value.length - prefix.length) / 4 * 3 - (value.endsWith('==') ? 2 : value.endsWith('=') ? 1 : 0)) <= maxPng
    && value.startsWith(prefix + 'iVBORw0KGgo') && /^[A-Za-z0-9+/]+={0,2}$/.test(value.slice(prefix.length));
}

const maxOriginal = 8 * 1024 * 1024;
const originalTypes = new Set(['image/png', 'image/jpeg', 'image/webp']);

export function validOriginal(value) {
  if (typeof value !== 'string' || value.length > 32 + 4 * Math.ceil(maxOriginal / 3)) return false;
  return /^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(value);
}

export async function originalFromFile(file) {
  if (!file.size || file.size > maxOriginal || !originalTypes.has(file.type)) throw Error('badImage');
  const header = new Uint8Array(await file.slice(0, 12).arrayBuffer());
  const ascii = String.fromCharCode(...header);
  const mime = header[0] === 255 && header[1] === 216 && header[2] === 255 ? 'image/jpeg'
    : ascii.startsWith('RIFF') && ascii.endsWith('WEBP') ? 'image/webp'
    : ascii.startsWith('\x89PNG\r\n\x1a\n') ? 'image/png' : '';
  if (mime !== file.type) throw Error('badImage');
  const url = await new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = reader.onabort = () => reject(Error('badImage'));
    reader.readAsDataURL(file);
  });
  const source = new Image();
  try {
    await new Promise((resolve, reject) => {
      source.onload = resolve;
      source.onerror = () => reject(Error('badImage'));
      source.src = url;
    });
    const {naturalWidth: width, naturalHeight: height} = source;
    if (!width || !height || Math.max(width, height) > 16384 || width * height > 40_000_000) throw Error('badImage');
    await source.decode();
    return url;
  } finally { source.removeAttribute('src'); }
}

export async function sharePng(url) {
  if (!validOriginal(url)) throw Error('badImage');
  const source = new Image();
  const canvas = document.createElement('canvas');
  try {
    source.src = url;
    await source.decode();
    let scale = Math.min(1, 1600 / Math.max(source.naturalWidth, source.naturalHeight));
    while (true) {
      canvas.width = Math.max(1, Math.round(source.naturalWidth * scale));
      canvas.height = Math.max(1, Math.round(source.naturalHeight * scale));
      canvas.getContext('2d').drawImage(source, 0, 0, canvas.width, canvas.height);
      const data = canvas.toDataURL('image/png');
      if (validImage(data)) return data;
      if (Math.max(canvas.width, canvas.height) <= 64) break;
      scale *= 0.7;
    }
    throw Error('badImage');
  } finally {
    source.removeAttribute('src');
    canvas.width = canvas.height = 0;
  }
}

const escape = (s) => String(s).replace(/[&<>"']/g, c => ({'&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;'})[c]);

// This function runs only in the standalone recipient file. Share data is
// encoded in HTML text/attributes, never interpolated into executable code.
function recipient() {
  document.querySelector('form').addEventListener('submit', event => {
    event.preventDefault();
    const field = document.querySelector('textarea');
    const comment = field.value.trim();
    if (!comment) { field.focus(); return; }
    const feedback = {format: 'ky-feedback-1', shareId: document.body.dataset.share, comment};
    const url = URL.createObjectURL(new Blob([JSON.stringify(feedback)], {type: 'application/json'}));
    const link = document.createElement('a');
    link.href = url;
    link.download = 'ky-feedback.json';
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 30000);
  });
}

export function offlinePage(share) {
  const nonce = crypto.randomUUID().replaceAll('-', '');
  return `<!doctype html><html lang="${getLang()}"><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; script-src 'nonce-${nonce}'; style-src 'nonce-${nonce}'; base-uri 'none'; form-action 'none'">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>${escape(share.title)}</title>
<style nonce="${nonce}">
:root {color-scheme:light dark;--paper:#f7faf8;--bg:#eef4ef;--ink:#1b2d24;--muted:#52655a;--line:#d4e0d8;--accent:#086744;--on-accent:white}
@media(prefers-color-scheme:dark){:root{--paper:#182219;--bg:#0d1410;--ink:#e6efe9;--muted:#a0b0a6;--line:#354338;--accent:#66f0b5;--on-accent:#06281a}}
* {box-sizing:border-box}
body {margin:0;background:var(--bg);color:var(--ink);font:1rem/1.65 system-ui,sans-serif;overflow-wrap:anywhere}
main {width:min(100%,44rem);padding:clamp(.875rem,4vw,2rem);margin:clamp(.5rem,4vh,3rem) auto}
article {background:var(--paper);border:1px solid var(--line);border-radius:1rem}
article img {display:block;width:auto;height:auto;max-width:100%;max-height:65vh;object-fit:contain;margin:0 auto 1.25rem;max-width:calc(100% - 2.5rem)}
.share-copy {padding:clamp(1.25rem,5vw,2.5rem)}
.share-copy::before {content:'';display:block;width:2rem;height:3px;background:var(--accent);margin-bottom:1rem}
h1 {margin:0;font-size:clamp(1.5rem,4vw,2.1rem);line-height:1.25;letter-spacing:-.025em}
.share-body {white-space:pre-wrap;margin:1rem 0 0;line-height:1.75}
form {margin:2rem 0 0;padding-inline:clamp(.25rem,3vw,1rem)}
h2 {margin:0 0 .375rem;font-size:1.12rem}
label {color:var(--muted);font-size:.92rem}
textarea {display:block;width:100%;resize:vertical;margin:.75rem 0;padding:.75rem;background:var(--paper);color:var(--ink);border:1px solid var(--line);border-radius:.5rem;font:inherit}
button {border:0;border-radius:.5rem;padding:.65rem 1rem;background:var(--accent);color:var(--on-accent);font:600 .94rem system-ui;cursor:pointer}
:focus-visible {outline:2px solid var(--accent);outline-offset:2px}
</style>
</head><body data-share="${escape(share.id)}"><main><article>
<div class="share-copy"><h1>${escape(share.title)}</h1>${share.body ? '<p class="share-body">' + escape(share.body) + '</p>' : ''}</div>
${validImage(share.image) ? '<img alt="' + w('image') + '" src="' + share.image + '">' : ''}</article>
<form><h2>${w('feedback')}</h2><label>${w('send')}<textarea required rows="4" maxlength="12000"></textarea></label><button>${w('download')}</button></form>
</main><script nonce="${nonce}">(${recipient.toString()})();</script></body></html>`;
}

export async function parseFeedback(file) {
  if (!file || file.size > 128 * 1024) throw Error('invalidFeedback');
  let data;
  try { data = JSON.parse(await file.text()); } catch { throw Error('invalidFeedback'); }
  if (data?.format !== 'ky-feedback-1' || typeof data.shareId !== 'string' || !data.shareId || data.shareId.length > 256
    || typeof data.comment !== 'string' || !data.comment.trim() || data.comment.length > 12000) throw Error('invalidFeedback');
  return {shareId: data.shareId, comment: data.comment.trim()};
}

export async function writeCardBlob(blob, copy, active, invoke) {
  if (!active()) return false;
  if (!(blob instanceof Blob) || !blob.size || blob.type !== 'image/png') throw Error('invalidCard');
  const data = await new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = reader.onabort = () => reject(Error('invalidCard'));
    reader.readAsDataURL(blob);
  });
  if (!active()) return false;
  if (!copy) return invoke('export_share_png', {name: 'ky-card.png', data});
  await invoke('copy_share_png', {data});
  return true;
}
