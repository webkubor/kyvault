// The select owns its value, labels, native validation and form submission.
// Mouse and unmodified opening keys get a themed list; touch, native AT
// activation, Option+Down, forced colours and older WebKit keep the OS picker.
import { platformMark } from './platform-icon.js';

export function installControls() {
  if (!('showPopover' in HTMLElement.prototype)) return;
  const nativeColors = matchMedia('(forced-colors: active)');
  let opened;
  const coarse = matchMedia('(pointer: coarse)');
  const editable = node => node instanceof HTMLInputElement && ['text','search','url','email','tel'].includes(node.type) && (node.list || opened?.select === node);
  const eligible = node => node instanceof HTMLElement && !nativeColors.matches && !coarse.matches &&
    !node.matches(':disabled,[data-native]') && (editable(node) ? !node.readOnly :
    node instanceof HTMLSelectElement && !node.multiple && node.size <= 1 && node.options.length);
  let composing;

  function close(restore = false) {
    if (!opened) return;
    const { select, list, observer, source, original } = opened;
    opened = null;
    observer.disconnect();
    if (select instanceof HTMLInputElement) select.setAttribute('list',source.id);
    for (const [key,value] of original) {
      if (value === null) select.removeAttribute(key);
      else select.setAttribute(key,value);
    }
    list.remove();
    if (restore && select.isConnected) select.focus({preventScroll:true});
  }

  function activate(row, scroll = true) {
    if (!row || row.getAttribute('aria-disabled') === 'true') return;
    const p = opened;
    if (p.active === row) return;
    p.active?.classList.remove('active');
    p.active = row;
    row.classList.add('active');
    (p.editing ? p.select : p.list).setAttribute('aria-activedescendant', row.id);
    if (scroll) row.scrollIntoView({block:'nearest',inline:'nearest'});
  }

  function choose(row) {
    if (!row || row.getAttribute('aria-disabled') === 'true') return;
    const { select, source, editing } = opened;
    const option = source.options[Number(row.dataset.index)];
    if (!eligible(select) || !option || option.matches(':disabled')) return close(true);
    close(true);
    if (editing) {
      if (select.value === option.value) return;
      select.value = option.value;
    } else {
      if (select.selectedIndex === option.index) return;
      select.selectedIndex = option.index;
    }
    selecting = true;
    try {
      for (const type of ['input','change']) select.dispatchEvent(new Event(type,{bubbles:true}));
    } finally { selecting = false; }
  }

  let selecting = false;
  function open(select) {
    if (opened) close();
    const editing = select instanceof HTMLInputElement;
    const source = editing ? select.list : select;
    const attributes = ['aria-controls','aria-expanded'];
    if (editing) attributes.push('role','aria-autocomplete','aria-activedescendant');
    const original = attributes.map(key => [key,select.getAttribute(key)]);
    const query = editing ? select.value.trim().toLocaleLowerCase() : '';
    const list = document.createElement('div');
    list.id = 'ky-open-options';
    list.className = 'ky-options';
    list.popover = 'manual';
    list.tabIndex = -1;
    list.setAttribute('role','listbox');
    const labelledBy = select.getAttribute('aria-labelledby');
    if (labelledBy) list.setAttribute('aria-labelledby',labelledBy);
    else {
      const label = select.labels?.[0]?.cloneNode(true);
      label?.querySelectorAll('select,input,textarea,[aria-hidden=true]').forEach(n => n.remove());
      list.setAttribute('aria-label',select.getAttribute('aria-label') || label?.textContent.trim() || select.name);
    }
    let group;
    for (const [index, option] of Array.from(source.options).entries()) {
      const platformInput = editing && source.id === 'credential-platforms';
      const title = platformInput ? option.label.replace(/^[^\p{L}\p{N}]+/u,'') : option.label;
      if (editing && ![title,option.value].some(text => text.toLocaleLowerCase().includes(query))) continue;
      const parent = option.parentElement;
      if (option.hidden || parent.hidden) continue;
      if (parent instanceof HTMLOptGroupElement && parent !== group) {
        const heading = document.createElement('div');
        heading.className = 'ky-option-group';
        heading.textContent = parent.label;
        list.append(heading);
      }
      group = parent;
      const row = document.createElement('div');
      row.className = 'ky-option';
      row.id = 'ky-option-' + index;
      row.dataset.index = index;
      row.setAttribute('role','option');
      row.setAttribute('aria-selected', String(editing ? select.value === option.value : option.selected));
      if (option.matches(':disabled') || group.disabled) row.setAttribute('aria-disabled','true');
      const platform = platformInput ? option.value : select.id === 'vault-platform' ? option.dataset.platform :
        select.dataset.accountField === 'provider' ? option.value : null;
      const mark = platform && platformMark(platform);
      if (mark) row.append(mark);
      const caption = document.createElement('span');
      caption.className = 'ky-option-label';
      caption.textContent = title;
      row.append(caption);
      list.append(row);
    }
    if (!list.querySelector('[role=option]')) return;
    if (editing) {
      select.removeAttribute('list');
      select.setAttribute('role','combobox');
      select.setAttribute('aria-autocomplete','list');
    }
    (select.closest('dialog[open]') || document.body).append(list);
    list.showPopover();
    const rect = select.getBoundingClientRect();
    const gap = 4;
    const margin = 8;
    list.style.minWidth = Math.min(rect.width,innerWidth-margin*2) + 'px';
    const below = innerHeight-rect.bottom-margin-gap;
    const above = rect.top-margin-gap;
    const up = list.offsetHeight > below && above > below;
    list.style.maxHeight = Math.max(1,Math.min(320,up ? above : below)) + 'px';
    list.style.left = Math.max(margin,Math.min(rect.left,innerWidth-list.offsetWidth-margin)) + 'px';
    list.style.top = (up ? rect.top-gap-list.offsetHeight : rect.bottom+gap) + 'px';
    const observer = new MutationObserver(records => {
      if (!select.isConnected || !eligible(select) || !select.getClientRects().length ||
          records.some(r => source.contains(r.target))) close();
    });
    observer.observe(document.body,{childList:true,subtree:true,attributes:true,attributeFilter:['hidden','disabled','open','label','value','selected']});
    opened = {select,list,source,editing,observer,original,active:null,buffer:'',typed:0};
    select.setAttribute('aria-controls',list.id);
    select.setAttribute('aria-expanded','true');
    if (!editing) list.focus({preventScroll:true});
    if (!editing) activate(list.querySelector('[aria-selected=true]:not([aria-disabled])') || list.querySelector('[role=option]:not([aria-disabled])'));
    list.onpointermove = e => {
      if (e.pointerType === 'mouse') activate(e.target.closest('[role=option]'),false);
    };
    list.onmousedown = e => { if (e.target.closest('[role=option]')) e.preventDefault(); };
    list.onclick = e => choose(e.target.closest('[role=option]'));
  }

  document.addEventListener('pointerdown', e => {
    if (opened && !opened.list.contains(e.target)) {
      const same = opened.select === e.target;
      close(same);
      if (same) { if (!(e.target instanceof HTMLInputElement)) e.preventDefault(); return; }
    }
    if (composing === e.target || e.pointerType !== 'mouse' || e.button !== 0 || e.altKey || !eligible(e.target)) return;
    if (!(e.target instanceof HTMLInputElement)) e.preventDefault();
    e.target.focus({preventScroll:true});
    open(e.target);
  },true);

  document.addEventListener('compositionstart', () => { composing = document.activeElement; if (opened?.editing) close(); });
  document.addEventListener('compositionend', e => { composing = null; if (editable(e.target) && eligible(e.target)) open(e.target); });
  document.addEventListener('input', e => {
    if (!selecting && composing !== e.target && !e.isComposing && editable(e.target) && eligible(e.target) && document.activeElement === e.target) open(e.target);
  });
  document.addEventListener('keydown', e => {
    if (composing === e.target || e.keyCode === 229 || e.isComposing || e.metaKey || e.ctrlKey || e.altKey) return;
    if (!opened) {
      if (eligible(e.target) && (editable(e.target) ? ['ArrowDown','ArrowUp'] : ['ArrowDown','ArrowUp','Enter',' ','F4']).includes(e.key)) {
        e.preventDefault();
        open(e.target);
        if (opened?.editing) {
          const choices = opened.list.querySelectorAll('[role=option]:not([aria-disabled])');
          activate(e.key === 'ArrowUp' ? choices[choices.length-1] : choices[0]);
        }
      }
      return;
    }
    const p = opened;
    const rows = [...p.list.querySelectorAll('[role=option]:not([aria-disabled])')];
    const index = rows.indexOf(p.active);
    // Keep caret movement, typing, Space and free-text Enter native.
    if (p.editing && !['ArrowDown','ArrowUp','Escape','Tab','Enter'].includes(e.key)) return;
    if (p.editing && e.key === 'Enter' && !p.active) { close(); return; }
    if (e.key === 'Tab') { close(true); return; }
    if (e.key === 'Escape') close(true);
    else if (['Enter',' '].includes(e.key)) choose(p.active);
    else if (e.key === 'ArrowDown') activate(rows[Math.min(rows.length-1,index+1)]);
    else if (e.key === 'ArrowUp') activate(rows[Math.max(0,index-1)]);
    else if (e.key === 'Home') activate(rows[0]);
    else if (e.key === 'End') activate(rows.at(-1));
    else if (e.key.length === 1) {
      const now = performance.now();
      const char = e.key.toLocaleLowerCase();
      p.buffer = now-p.typed < 700 && p.buffer !== char ? p.buffer+char : char;
      p.typed = now;
      const ordered = p.buffer.length === 1 ? [...rows.slice(index+1),...rows.slice(0,index+1)] : rows;
      activate(ordered.find(r => r.querySelector('.ky-option-label').textContent.toLocaleLowerCase().startsWith(p.buffer)));
    } else return;
    e.preventDefault();
    e.stopImmediatePropagation();
  },true);
  document.addEventListener('focusin',e => {
    if (opened && !opened.list.contains(e.target) && opened.select !== e.target) close();
  });
  document.addEventListener('scroll', e => {
    if (opened && !opened.list.contains(e.target) && opened.select !== e.target) close();
  },true);
  window.addEventListener('resize',() => close(true));
  window.addEventListener('blur',() => close());
  nativeColors.addEventListener('change',() => close(true));
}
