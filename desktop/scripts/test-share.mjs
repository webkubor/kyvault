import assert from 'node:assert/strict';
import { offlinePage, parseFeedback } from '../ui/workspace-files.js';

// A recipient needs only the selected snapshot and its stable reply address.
const snapshot = { id: 'e4298f93-6bdc-4384-a991-f5cfd639ad2f', title: 'A & B', body: 'A < B', image: '' };
const page = offlinePage(snapshot);
assert.ok(page.includes('A &amp; B'));
assert.ok(page.includes('A &lt; B'));
assert.ok(page.includes(`data-share="${snapshot.id}"`));
assert.ok(page.includes("default-src 'none'"));
const response = {
  format: 'ky-feedback-1', shareId: snapshot.id, comment: ' Great idea. '
};
const file = data => new Blob([JSON.stringify(data)], {type: 'application/json'});
const parsed = await parseFeedback(file(response));
assert.equal(parsed.shareId, snapshot.id);
assert.equal(parsed.comment, 'Great idea.');
await assert.rejects(parseFeedback(file({...response, format: 'unknown'})));
await assert.rejects(parseFeedback(file({...response, comment: ' '})));
console.log('离线分享文本、版本关联和反馈格式验证通过');
