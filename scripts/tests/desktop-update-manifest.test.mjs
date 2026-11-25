import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { manifest } from '../desktop-update-manifest.mjs';

test('manifest binds a signed bundle to the desktop product line', () => {
  const root = mkdtempSync(join(tmpdir(), 'ky-update-manifest-'));
  try {
    const bundle = join(root, 'Ky.app.tar.gz');
    writeFileSync(bundle, 'fixture only');
    const options = { version: '0.3.0', platform: 'darwin-aarch64', bundle,
      signature: Buffer.from('untrusted comment: fixture\nfixture\ntrusted comment: fixture\nfixture').toString('base64'),
      notes: 'Fixture release', date: '2026-11-25T00:00:00Z' };
    const result = manifest(options);
    assert.equal(result.platforms['darwin-aarch64'].url, 'https://github.com/webkubor/kyvault/releases/download/desktop-v0.3.0/Ky.app.tar.gz');
    for (const bad of [{ signature: '' }, { signature: Buffer.from('not a signature').toString('base64') },
      { version: '../../main' }, { platform: 'linux' }, { platform: 'darwin-x86_64' }, { bundle: join(root, 'missing.tar.gz') }, { date: 'invalid' }]) {
      assert.throws(() => manifest({ ...options, ...bad }));
    }
  } finally { rmSync(root, { recursive: true, force: true }); }
});
