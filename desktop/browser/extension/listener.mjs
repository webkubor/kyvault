// Chromium serializes this function into the isolated world of the top frame.
export function attachCopy(expectedHost) {
  if (location.host !== expectedHost || chrome.extension.inIncognitoContext ||
      globalThis.kyFlashAttached) return;
  const key = 'auto:' + expectedHost;
  function editable(node) {
    const el = node?.nodeType === 3 ? node.parentElement : node;
    return el?.isContentEditable || el?.closest?.('input,textarea,[role="textbox"]');
  }
  function copy(event) {
    if (!event.isTrusted || document.designMode === 'on' ||
        event.composedPath().some(editable) || editable(document.activeElement)) return;
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed || editable(selection.anchorNode) ||
        editable(selection.focusNode)) return;
    const text = selection.toString();
    if (text.trim()) chrome.runtime.sendMessage({type: 'copy', text}).catch(detach);
  }
  function detach() {
    document.removeEventListener('copy', copy, true);
    chrome.storage.onChanged.removeListener(changed);
    if (globalThis.kyFlashAttached === copy) delete globalThis.kyFlashAttached;
  }
  function changed(changes, area) {
    if (area === 'local' && changes[key] && changes[key].newValue !== true) detach();
  }
  globalThis.kyFlashAttached = copy;
  chrome.storage.onChanged.addListener(changed);
  return chrome.storage.local.get(key).then(state => {
    if (state[key] && globalThis.kyFlashAttached === copy) document.addEventListener('copy', copy, true);
    else detach();
  });
}
