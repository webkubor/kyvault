// i18n application layer — applies the dictionary to the DOM and keeps the
// backend in sync. Data lives in i18n.js; this file is the glue.
//
// Rules (see DESIGN.md):
//   • zh-CN is the default language; en is switchable in Settings.
//   • The backend (prefs.json) is the source of truth; localStorage
//     ['ky.lang'] is only a mirror for instant paint on the next launch.
//   • Only the language code is ever stored. No secret, no history, no
//     clipboard content ever touches localStorage.

import { $, $$, readPref, writePref, invoke, state } from './core.js';
import { DEFAULT_LANG, normalizeLang, setLang, getLang, t, translate } from './i18n.js';

export const LANG_KEY = 'ky.lang';

// Callbacks invoked after the language changes, so the dynamic surface can be
// re-rendered (kept as a hook to avoid a circular import with render.js).
const listeners = new Set();
export function onLanguageChanged(fn) {
  listeners.add(fn);
}
function notify() {
  listeners.forEach((fn) => { try { fn(getLang()); } catch { /* keep other listeners */ } });
}

function desktopBridge() {
  return Boolean(globalThis.__TAURI__ && globalThis.__TAURI__.core);
}

// Apply static data-i18n attributes: text, placeholder, aria-label.
export function applyStaticStrings() {
  $$('[data-i18n]').forEach((node) => {
    node.textContent = t(node.dataset.i18n);
  });
  $$('[data-i18n-placeholder]').forEach((node) => {
    node.placeholder = t(node.dataset.i18nPlaceholder);
  });
  $$('[data-i18n-aria]').forEach((node) => {
    node.setAttribute('aria-label', t(node.dataset.i18nAria));
  });
  document.title = t('doc.title');
}

export function applyLanguage(lang, { persist = true, notifyBackend = true } = {}) {
  const next = normalizeLang(lang);
  setLang(next);
  state.lang = next;
  document.documentElement.lang = next;
  if (persist) writePref(LANG_KEY, next);
  applyStaticStrings();
  // Re-render every runtime string (status line, errors, headings, dialogs).
  notify();
  if (notifyBackend && desktopBridge()) {
    // Backend is the source of truth; mirror the change there. Failures are
    // non-fatal — the UI language still applies locally.
    invoke('set_ui_language', { lang: next }).then(
      (snapshot) => {
        if (snapshot && snapshot.lang && snapshot.lang !== next) {
          applyLanguage(snapshot.lang, { persist: true, notifyBackend: false });
        }
      },
      () => {}
    );
  }
  return next;
}

// Called after the first snapshot. Per the product rule "on init pass
// stored": the localStorage mirror wins on launch and is pushed to the
// backend, so the same machine always opens in the language the user last
// chose here. Trade-off (documented in DESIGN.md): a language changed on a
// different install is overridden by this machine's mirror until the user
// switches language here.
export function syncLanguageFromSnapshot(snapshot) {
  const stored = readPref(LANG_KEY, null);
  if (stored) {
    return applyLanguage(stored, { persist: true, notifyBackend: true });
  }
  // No mirror yet → adopt the backend language and create the mirror.
  const backendLang = snapshot && snapshot.lang ? normalizeLang(snapshot.lang) : DEFAULT_LANG;
  return applyLanguage(backendLang, { persist: true, notifyBackend: false });
}

export function currentLang() {
  return getLang();
}

export { t, translate, DEFAULT_LANG, normalizeLang };
