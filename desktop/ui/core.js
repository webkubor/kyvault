"use strict";

// Shared bridge + DOM helpers for the Ky desktop UI.
// No dependency on Tauri at import time — invoke/listen read window.__TAURI__
// lazily so tests can inject a mock before load (via CDP) without errors.
// No secrets ever pass through storage here; localStorage holds only the
// UI language + toolbar preference.
//
// Every runtime string is stored as an i18n KEY (plus optional vars), never as
// finished text. That is what lets a language switch re-render the status
// line, error lines and open dialogs without leaving stale copy behind.

import { t } from './i18n.js';

export const state = {
  view: "history",
  snapshot: null,
  selectedId: null,
  filter: "",
  entry: null,
  locked: false,
  lang: "zh-CN",
};

export const $ = (s, r = document) => r.querySelector(s);
export const $$ = (s, r = document) => Array.from(r.querySelectorAll(s));

export function el(tag, cls, text) {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

export function showEl(node, show) {
  node.hidden = !show;
}

export function invoke(cmd, args = {}) {
  const core = globalThis.__TAURI__ && globalThis.__TAURI__.core;
  if (!core || typeof core.invoke !== "function") return Promise.reject(new Error("no-bridge"));
  try { return Promise.resolve(core.invoke(cmd, args)); }
  catch (err) { return Promise.reject(err); }
}

export function listen(event, handler) {
  const api =
    globalThis.__TAURI__ && globalThis.__TAURI__.event
      ? globalThis.__TAURI__.event
      : null;
  if (!api || typeof api.listen !== "function") {
    return Promise.resolve(() => {});
  }
  return api.listen(event, handler);
}

export function formatWhen(value) {
  if (!value) return "";
  const ms = typeof value === "number" ? value * 1000 : Date.parse(value);
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return "";
  const now = new Date();
  const sameYear = d.getFullYear() === now.getFullYear();
  const opts = sameYear
    ? { month: "short", day: "numeric" }
    : { month: "short", day: "numeric", year: "numeric" };
  const date = d.toLocaleDateString(undefined, opts);
  const hm = d.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
  return `${date} · ${hm}`;
}

export function firstLine(text) {
  const idx = (text || "").indexOf("\n");
  return idx === -1 ? (text || "").trim() : text.slice(0, idx).trim();
}

export function isComposing(e) {
  // keyCode 229 is the legacy-but-reliable "still composing" IME signal.
  return e.isComposing || e.keyCode === 229;
}

/* ---------- localizable runtime messages ---------- */

// status line, error banner, per-form errors, and the open confirm dialog.
export const messages = {
  status: null,
  statusVars: null,
  error: null,
  errorVars: null,
  formErrors: new Map(),
  confirm: null,
};

export function setStatus(key, vars) {
  messages.status = key || null;
  messages.statusVars = vars || null;
  $("#status").textContent = key ? (vars ? t(key, vars) : t(key)) : "";
}

export function setError(key, vars) {
  messages.error = key || null;
  messages.errorVars = vars || null;
  const node = $("#error");
  if (key) {
    node.textContent = vars ? t(key, vars) : t(key);
    showEl(node, true);
  } else {
    showEl(node, false);
  }
}

// Per-form error (e.g. #text-error, #secret-error, #unlock-error).
export function setFormError(id, key, vars) {
  const node = $(id);
  if (key) {
    messages.formErrors.set(id, { key, vars: vars || null });
    node.textContent = vars ? t(key, vars) : t(key);
    showEl(node, true);
  } else {
    messages.formErrors.delete(id);
    node.textContent = "";
    showEl(node, false);
  }
}

// The confirmation dialog keeps its keys so it can be re-rendered on switch.
export function setConfirmText({ titleKey, copyKey, labelKey, vars }) {
  messages.confirm = { titleKey, copyKey, labelKey, vars: vars || null };
  $("#confirm-title").textContent = t(titleKey, vars);
  $("#confirm-copy").textContent = t(copyKey, vars);
  $("#confirm-action").textContent = labelKey ? t(labelKey) : t('action.cancel');
}

// Re-apply the current language to every runtime message.
export function relocalizeMessages() {
  if (messages.status) {
    $("#status").textContent = messages.statusVars
      ? t(messages.status, messages.statusVars)
      : t(messages.status);
  }
  if (messages.error) {
    $("#error").textContent = messages.errorVars
      ? t(messages.error, messages.errorVars)
      : t(messages.error);
  }
  messages.formErrors.forEach((entry, id) => {
    const node = $(id);
    if (!node) return;
    node.textContent = entry.vars ? t(entry.key, entry.vars) : t(entry.key);
  });
  if (messages.confirm && $("#confirm").open) {
    const c = messages.confirm;
    $("#confirm-title").textContent = t(c.titleKey, c.vars);
    $("#confirm-copy").textContent = t(c.copyKey, c.vars);
    $("#confirm-action").textContent = c.labelKey ? t(c.labelKey) : t('action.cancel');
  }
}

export function readPref(key, fallback) {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v;
  } catch {
    return fallback;
  }
}

export function writePref(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* storage unavailable — pref stays session-only */
  }
}
