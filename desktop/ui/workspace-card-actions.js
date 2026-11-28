import { writeCardBlob } from './workspace-files.js';
import { el, invoke } from './core.js';
import { getLang } from './i18n.js';
import { platformMark } from './platform-icon.js';
import { renderShareCard } from './share-card.js';

export function pasteDestinations(label) {
  const hint = el('div', 'wk-paste-hint');
  hint.append(label('span', 'pasteIn'));
  for (const [id, key] of [['wechat', 'wechat'], ['x', 'x']]) {
    const name = el('span', 'wk-destination');
    name.append(platformMark(id), label('span', key));
    hint.append(name);
  }
  return hint;
}

// The snapshot is read from the shares collection. No draft identifiers,
// author details, or labels for the paste destinations go into its pixels.
export async function writeShareCard(snapshot, copy, active) {
  const {title, body, image} = snapshot;
  const blob = await renderShareCard({title, body, image}, getLang());
  return writeCardBlob(blob, copy, active, invoke);
}
