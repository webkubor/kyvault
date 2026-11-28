import assert from 'node:assert/strict';
import { offlinePage, parseFeedback, writeCardBlob } from '../ui/workspace-files.js';

// A recipient needs only the selected snapshot and its stable reply address.
const snapshot = { id: 'e4298f93-6bdc-4384-a991-f5cfd639ad2f', title: 'A & B', body: 'A < B', image: '' };
const page = offlinePage(snapshot);
assert.ok(page.includes('A &amp; B'));
assert.ok(page.includes('A &lt; B'));
assert.ok(page.includes(`data-share="${snapshot.id}"`));
assert.ok(page.includes("default-src 'none'"));
assert.doesNotMatch(page, /<img\b/i);

const png = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=';
const illustrated = offlinePage({...snapshot, image: png});
assert.ok(illustrated.includes('src="' + png + '"'));
assert.match(illustrated, /<article>[\s\S]*A &lt; B[\s\S]*<img [^>]*>[\s\S]*<\/article>/);
const imageOnly = offlinePage({...snapshot, image: png, body: ''});
assert.doesNotMatch(imageOnly, /<p class="share-body">/);
const unsupported = offlinePage({...snapshot, image: 'https://example.invalid/image.png'});
assert.doesNotMatch(unsupported, /<img\b|example\.invalid/);
const markup = offlinePage({...snapshot, title: '<em>Title</em>', body: '<aside>Text</aside>'});
assert.ok(markup.includes('&lt;em&gt;Title&lt;/em&gt;'));
assert.ok(markup.includes('&lt;aside&gt;Text&lt;/aside&gt;'));
const response = {
  format: 'ky-feedback-1', shareId: snapshot.id, comment: ' Great idea. '
};
const file = data => new Blob([JSON.stringify(data)], {type: 'application/json'});
const parsed = await parseFeedback(file(response));
assert.equal(parsed.shareId, snapshot.id);
assert.equal(parsed.comment, 'Great idea.');
await assert.rejects(parseFeedback(file({...response, format: 'unknown'})));
await assert.rejects(parseFeedback(file({...response, comment: ' '})));
console.log('纯文字、图文、纯图、离线资源、版本关联和反馈格式验证通过');

// Node supplies Blob but not the WebView's FileReader. Exercise the same
// Blob-to-IPC path used after rendering, including the native parser's prefix.
const blob = new Blob([Buffer.from(png.split(',')[1], 'base64')], {type: 'image/png'});
globalThis.FileReader = class {
  async readAsDataURL(file) {
    this.result = 'data:' + file.type + ';base64,' + Buffer.from(await file.arrayBuffer()).toString('base64');
    this.onload();
  }
};
const calls = [];
function invoke(command, args) {
  calls.push([command, args]);
  assert.equal(args.data, png);
  return command === 'export_share_png' ? false : undefined;
}
try {
  assert.equal(await writeCardBlob(blob, true, () => true, invoke), true);
  assert.equal(await writeCardBlob(blob, false, () => true, invoke), false);
  let liveChecks = 0;
  assert.equal(await writeCardBlob(blob, true, () => ++liveChecks === 1, invoke), false);
  assert.deepEqual(calls, [
    ['copy_share_png', {data: png}],
    ['export_share_png', {name: 'ky-card.png', data: png}],
  ]);
} finally {
  delete globalThis.FileReader;
}
console.log('复制与保存均传完整 PNG data URL，锁屏后无 IPC 调用');
