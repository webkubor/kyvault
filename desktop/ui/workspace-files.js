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
<style nonce="${nonce}">:root{color-scheme:light dark}body{max-width:42rem;margin:3rem auto;padding:0 1rem;font:1rem/1.6 system-ui;overflow-wrap:anywhere}article{white-space:pre-wrap}img,textarea{max-width:100%}textarea{display:block;width:100%;box-sizing:border-box;margin:.5rem 0}form{margin:2rem 0}button{padding:.5rem 1rem}</style>
</head><body data-share="${escape(share.id)}"><main><h1>${escape(share.title)}</h1><article>${escape(share.body)}</article>
${validImage(share.image) ? '<img alt="' + w('image') + '" src="' + share.image + '">' : ''}
<form><h2>${w('feedback')}</h2><label>${w('send')}<textarea required rows="6" maxlength="12000"></textarea></label><button>${w('download')}</button></form>
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
