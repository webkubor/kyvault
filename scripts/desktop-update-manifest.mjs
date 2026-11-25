#!/usr/bin/env node
// Generate a manifest only after a signed bundle exists; never publishes it.
import { readFileSync, existsSync } from 'node:fs';
import { resolve, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

export function manifest({ version, platform, bundle, signature, notes, date }) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error('Expected stable X.Y.Z version');
  if (platform !== 'darwin-aarch64') throw new Error('Unsupported platform');
  if (basename(bundle) !== 'Ky.app.tar.gz' || !existsSync(bundle)) throw new Error('Signed Ky.app.tar.gz bundle missing');
  const sig = signature.trim();
  if (!sig || !/^[A-Za-z0-9+/]+={0,2}$/.test(sig)) throw new Error('Updater signature missing or malformed');
  const decoded = Buffer.from(sig, 'base64').toString('utf8');
  if (!decoded.startsWith('untrusted comment:') || !decoded.includes('trusted comment:')) throw new Error('Expected Tauri minisign signature');
  if (!Number.isFinite(Date.parse(date))) throw new Error('Invalid publication date');
  return {
    version, notes, pub_date: date,
    platforms: { [platform]: {
      signature: sig,
      url: `https://github.com/webkubor/kyvault/releases/download/desktop-v${version}/Ky.app.tar.gz`,
    } },
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const { values } = parseArgs({ options: {
      bundle: { type: 'string' }, platform: { type: 'string', default: 'darwin-aarch64' },
      'notes-file': { type: 'string' },
    } });
    if (!values.bundle || !values['notes-file']) throw new Error('Usage: node scripts/desktop-update-manifest.mjs --bundle <Ky.app.tar.gz> --notes-file <release-notes.md>');
    const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
    const config = JSON.parse(readFileSync(resolve(root, 'desktop/src-tauri/tauri.conf.json'), 'utf8'));
    const cargo = readFileSync(resolve(root, 'desktop/src-tauri/Cargo.toml'), 'utf8').match(/^version\s*=\s*"([^"]+)"/m)?.[1];
    const pkg = JSON.parse(readFileSync(resolve(root, 'desktop/package.json'), 'utf8'));
    if (config.version !== cargo || config.version !== pkg.version) throw new Error('Desktop version fields disagree');
    const output = manifest({ version: config.version, platform: values.platform, bundle: values.bundle,
      signature: readFileSync(`${values.bundle}.sig`, 'utf8'), notes: readFileSync(values['notes-file'], 'utf8').trim(), date: new Date().toISOString() });
    process.stdout.write(`${JSON.stringify(output, null, 2)}\n`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
