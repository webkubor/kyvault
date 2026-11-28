// Apply the saved appearance before first paint, including native form controls.
(() => {
  const key = 'ky.theme';
  const options = ['system', 'light', 'dark'];
  const systemDark = matchMedia('(prefers-color-scheme: dark)');
  let mode = 'system';
  try {
    const saved = localStorage.getItem(key);
    if (options.includes(saved)) mode = saved;
  } catch { /* System mode also works with storage disabled. */ }
  function paint() {
    document.documentElement.dataset.theme = mode === 'system'
      ? (systemDark.matches ? 'dark' : 'light') : mode;
  }
  function applyNative() {
    globalThis.__TAURI__?.app?.setTheme(mode === 'system' ? null : mode)?.catch(() => {});
  }
  paint();
  systemDark.addEventListener('change', paint);
  document.addEventListener('DOMContentLoaded', () => {
    const picker = document.querySelector('#appearance');
    const compact = document.querySelector('#appearance-select');
    function syncControls() {
      picker.querySelector('input[value="' + mode + '"]').checked = true;
      compact.value = mode;
    }
    function choose(event) {
      if (!options.includes(event.target.value)) return;
      mode = event.target.value;
      try { localStorage.setItem(key, mode); } catch { /* Keep session choice. */ }
      paint();
      syncControls();
      applyNative();
    }
    syncControls();
    applyNative();
    picker.addEventListener('change', choose);
    compact.addEventListener('change', choose);
  });
})();
