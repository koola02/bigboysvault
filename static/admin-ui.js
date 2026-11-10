// Shared admin helpers (toast + Undo, bottom sheet, deferred deletes) and the card manager:
// one-tap photo swap, selection checkboxes, and bulk edits with Undo.
window.Admin = (() => {
  const safe = (fn) => { try { return fn(); } catch (_) { return null; } };
  let toastEl = document.getElementById('toast');
  if (!toastEl) { toastEl = document.createElement('div'); toastEl.id = 'toast'; toastEl.className = 'toast'; toastEl.setAttribute('role', 'status'); toastEl.setAttribute('aria-live', 'polite'); toastEl.hidden = true; document.body.appendChild(toastEl); }
  let timer;

  /** Show a toast. opts: { kind: 'ok'|'err', undo: fn, ms } */
  function toast(msg, opts = {}) {
    clearTimeout(timer);
    toastEl.className = 'toast ' + (opts.kind || '');
    toastEl.textContent = msg;
    if (opts.undo) {
      const b = document.createElement('button'); b.type = 'button'; b.className = 'toast-undo'; b.textContent = 'Undo';
      b.onclick = () => { clearTimeout(timer); toastEl.hidden = true; opts.undo(); };
      toastEl.appendChild(b);
    }
    toastEl.hidden = false;
    timer = setTimeout(() => (toastEl.hidden = true), opts.ms || (opts.kind === 'err' ? 8000 : 2600));
  }

  async function send(url, init) {
    const r = await fetch(url, { method: 'POST', credentials: 'same-origin', ...init, headers: { Accept: 'application/json', ...(init.headers || {}) } });
    const j = await r.json().catch(() => ({}));
    if (!r.ok || j.error) throw new Error(j.error || 'Something went wrong. Please try again.');
    return j;
  }
  /** POST a FormData, a plain object (form-encoded), or nothing. */
  const post = (url, body) => body instanceof FormData ? send(url, { body })
    : send(url, { body: body ? new URLSearchParams(body) : undefined, headers: body ? { 'Content-Type': 'application/x-www-form-urlencoded' } : {} });
  const postJSON = (url, obj) => send(url, { body: JSON.stringify(obj), headers: { 'Content-Type': 'application/json' } });

  // Condition shares and pricing
  let _shares = null;
  async function loadShares() {
    if (_shares) return _shares;
    try { const r = await fetch('/api/condition-shares'); _shares = await r.json(); } catch (_) {}
    return _shares || {};
  }
  loadShares(); // Preload

  function normCond(c) {
    if (!c) return null;
    let t = c.split('/')[0].split('·')[0].trim().toLowerCase().replace(/[_/.]/g, ' ');
    t = t.replace(/-/g, ' ');
    const words = t.split(/\s+/);
    const compact = t.replace(/\s+/g, '');
    switch (compact) {
      case 'nearmint': case 'nm': case 'mint': case 'pristine': return 'Near Mint';
      case 'lpnm': case 'nmlp': return 'LP-NM';
      case 'lightlyplayed': case 'lp': return 'Lightly Played';
      case 'mplp': case 'lpmp': return 'MP-LP';
      case 'moderatelyplayed': case 'mp': case 'modplayed': return 'Moderately Played';
      case 'hpmp': case 'mphp': return 'HP-MP';
      case 'heavilyplayed': case 'hp': case 'heavyplayed': return 'Heavily Played';
      case 'damagedhp': case 'hpdamaged': case 'dmghp': case 'hpdmg': return 'Damaged-HP';
      case 'damaged': case 'dmg': return 'Damaged';
    }
    if (words.includes('near') && words.includes('mint')) return 'Near Mint';
    if (words.includes('mint') || words.includes('pristine')) return 'Near Mint';
    if ((words.includes('lp') && words.includes('nm')) || (words.includes('nm') && words.includes('lp'))) return 'LP-NM';
    if (words.includes('mp') && words.includes('lp')) return 'MP-LP';
    if (words.includes('hp') && words.includes('mp')) return 'HP-MP';
    if ((words.includes('damaged') || words.includes('dmg')) && words.includes('hp')) return 'Damaged-HP';
    if (words.includes('lightly') || words[0] === 'lp') return 'Lightly Played';
    if (words.includes('moderately') || words.includes('mod') || words[0] === 'mp') return 'Moderately Played';
    if (words.includes('heavily') || words.includes('heavy') || words[0] === 'hp') return 'Heavily Played';
    if (words.includes('damaged') || words[0] === 'dmg') return 'Damaged';
    return null;
  }
  
  function isGraded(c) {
    const t = (c || '').trim().toLowerCase();
    const words = t.split(/\s+/);
    const first = words[0] || '';
    if (['psa', 'cgc', 'bgs', 'sgc', 'ace', 'tag'].some(g => first === g || first.startsWith(g))) return true;
    return t.includes('gem mt') || t.includes('gem-mt') || t.includes('graded') || t.includes('slab');
  }

  function isSealed(c) { return (c || '').trim().toLowerCase().includes('sealed'); }

  function getShare(c) {
    if (isGraded(c) || isSealed(c)) return null;
    const std = normCond(c);
    if (!std) return null;
    if (_shares && _shares[std] !== undefined) return _shares[std];
    const defs = { 'Near Mint': 1, 'LP-NM': 0.9, 'Lightly Played': 0.8, 'MP-LP': 0.7, 'Moderately Played': 0.6, 'HP-MP': 0.5, 'Heavily Played': 0.4, 'Damaged-HP': 0.32, 'Damaged': 0.25 };
    return defs[std] || null;
  }

  function roundPhp(raw) {
    if (raw <= 0) return 1;
    let r;
    if (raw < 100) r = Math.round(raw);
    else if (raw <= 1000) r = Math.round(raw / 5) * 5;
    else r = Math.round(raw / 10) * 10;
    return Math.max(1, r);
  }

  function suggest(nearMint, cond) {
    if (nearMint <= 0) return null;
    const share = getShare(cond);
    if (share === null) return null;
    return roundPhp(nearMint * share);
  }

  const STD_CONDS = [
    'Near Mint', 'LP-NM', 'Lightly Played', 'MP-LP', 'Moderately Played', 'HP-MP', 'Heavily Played', 'Damaged-HP', 'Damaged'
  ];

  function renderSug(targetNode, curPrice, nmValue, curCond, label, source, updated, onSelect) {
    let bar = targetNode.nextElementSibling;
    if (!bar || !bar.classList.contains('cond-price-bar')) {
      bar = document.createElement('div');
      bar.className = 'cond-price-bar';
      targetNode.after(bar);
    }
    bar.replaceChildren();

    if (isGraded(curCond)) {
      const box = document.createElement('div');
      box.className = 'cpb-notice';
      box.innerHTML = `<span>⭐ Graded card: set your own price</span><small>Raw Near Mint is ₱${Math.round(nmValue || 0)}</small>`;
      bar.append(box);
      return;
    }
    if (isSealed(curCond)) {
      const box = document.createElement('div');
      box.className = 'cpb-notice';
      box.innerHTML = `<span>📦 Sealed item: set your own price</span><small>Raw Near Mint is ₱${Math.round(nmValue || 0)}</small>`;
      bar.append(box);
      return;
    }
    if (!nmValue || nmValue <= 0) {
      const box = document.createElement('div');
      box.className = 'cpb-notice';
      box.innerHTML = `<span>No market price found for this card</span><small>Type your own price</small>`;
      bar.append(box);
      return;
    }

    const head = document.createElement('div');
    head.className = 'cpb-head';
    const t = document.createElement('span');
    t.className = 'cpb-title';
    t.textContent = 'Condition prices';
    const ref = document.createElement('span');
    ref.className = 'cpb-ref';
    ref.textContent = `${source || 'TCGplayer'} · NM ₱${Math.round(nmValue)}${label ? ' (' + label + ')' : ''}${updated ? ' · ' + updated : ''}`;
    head.append(t, ref);
    bar.append(head);

    const chips = document.createElement('div');
    chips.className = 'cpb-chips';
    chips.setAttribute('role', 'group');
    chips.setAttribute('aria-label', 'Condition price suggestions');

    const activeNorm = normCond(curCond);

    STD_CONDS.forEach((cName) => {
      const sug = suggest(nmValue, cName);
      if (!sug) return;
      const chip = document.createElement('button');
      chip.type = 'button';
      chip.className = 'cpb-chip' + (activeNorm === cName ? ' on' : '');
      chip.innerHTML = `<span class="cpb-c">${cName}</span><span class="cpb-p">₱${sug}</span>`;
      chip.setAttribute('aria-label', `${cName}: ₱${sug}`);
      chip.onclick = (e) => {
        e.preventDefault();
        if (onSelect) onSelect(cName, sug);
      };
      chips.append(chip);
    });

    bar.append(chips);
  }

  // Resize on the device: fast uploads, handles HEIC and rotation.
  async function resize(file, max = 1600) {
    let bmp;
    try { bmp = await createImageBitmap(file, { imageOrientation: 'from-image' }); }
    catch (_) { bmp = await new Promise((res, rej) => { const i = new Image(); i.onload = () => res(i); i.onerror = rej; i.src = URL.createObjectURL(file); }); }
    const k = Math.min(1, max / Math.max(bmp.width, bmp.height));
    const c = document.createElement('canvas'); c.width = Math.round(bmp.width * k); c.height = Math.round(bmp.height * k);
    c.getContext('2d').drawImage(bmp, 0, 0, c.width, c.height);
    return new Promise((res, rej) => c.toBlob((b) => (b ? res(b) : rej(new Error('encode'))), 'image/jpeg', 0.86));
  }

  /** Delete-with-undo: hide now, really delete after 6s unless the user taps Undo. */
  const pending = new Set();
  function undoable({ hide, show, commit, label }) {
    hide();
    const job = { done: false };
    const run = async () => {
      if (job.done) return; job.done = true; pending.delete(job);
      try { await commit(); } catch (e) { show(); toast(e.message, { kind: 'err' }); }
    };
    job.run = run; pending.add(job);
    const t = setTimeout(run, 6000);
    toast(label, { ms: 6000, undo: () => { clearTimeout(t); job.done = true; pending.delete(job); show(); } });
  }
  addEventListener('pagehide', () => pending.forEach((j) => j.run && j.run()));

  /* ---------- bottom sheet: slides up on phones, centered card on big screens ---------- */
  const sheet = (() => {
    let root, titleEl, saveBtn, bodyEl, onSave = null, lastFocus = null;
    function ensure() {
      if (root) return;
      root = document.createElement('div'); root.className = 'sheet'; root.hidden = true;
      root.innerHTML = '<div class="sheet-bg" data-close></div><div class="sheet-panel" role="dialog" aria-modal="true"><header class="sheet-h"><button class="linkbtn" type="button" data-close>Cancel</button><h2></h2><button class="linkbtn strong" type="button" data-save>Save</button></header><div class="sheet-body"></div></div>';
      document.body.appendChild(root);
      titleEl = root.querySelector('h2'); saveBtn = root.querySelector('[data-save]'); bodyEl = root.querySelector('.sheet-body');
      root.addEventListener('click', (e) => { if (e.target.closest('[data-close]')) close(); });
      addEventListener('keydown', (e) => { if (e.key === 'Escape' && !root.hidden) close(); });
      saveBtn.addEventListener('click', save);
    }
    async function save() {
      if (!onSave) return; saveBtn.disabled = true; const t = saveBtn.textContent; saveBtn.textContent = 'Working…';
      try { await onSave(); } catch (err) { toast(err.message, { kind: 'err' }); }
      finally { saveBtn.disabled = false; saveBtn.textContent = t; }
    }
    function open(title, node, handler, label = 'Save') {
      ensure(); lastFocus = document.activeElement; titleEl.textContent = title; saveBtn.textContent = label; saveBtn.hidden = !handler;
      bodyEl.replaceChildren(node); onSave = handler; root.hidden = false; document.body.style.overflow = 'hidden';
      requestAnimationFrame(() => root.classList.add('open'));
      setTimeout(() => (node.querySelector('input,textarea,select,button') || saveBtn).focus({ preventScroll: true }), 260);
    }
    function close() {
      if (!root || root.hidden) return;
      root.classList.remove('open'); document.body.style.overflow = '';
      setTimeout(() => { root.hidden = true; bodyEl.replaceChildren(); }, 280); lastFocus?.focus?.();
    }
    return { open, close, save };
  })();

  return { toast, post, postJSON, resize, undoable, safe, sheet, loadShares, isGraded, isSealed, getShare, suggest, renderSug };
})();

// ----- card manager -----
(() => {
  const A = window.Admin;
  const root = document.getElementById('cardsRoot'); if (!root) return;
  const $ = (id) => document.getElementById(id);
  const el = (tag, cls, txt) => { const n = document.createElement(tag); if (cls) n.className = cls; if (txt != null) n.textContent = txt; return n; };

  /* ----- per-row: delete with undo, one-tap photo swap ----- */
  document.addEventListener('click', (e) => {
    const b = e.target.closest('[data-del]'); if (!b) return;
    const row = b.closest('.crow'); const id = b.dataset.del;
    A.undoable({
      label: `Deleted “${row.dataset.title}”`,
      hide: () => { row.style.height = row.offsetHeight + 'px'; requestAnimationFrame(() => row.classList.add('gone')); },
      show: () => { row.classList.remove('gone'); row.style.height = ''; },
      commit: async () => { await A.post('/admin/delete/' + id); row.remove(); },
    });
  });
  document.addEventListener('change', async (e) => {
    const inp = e.target.closest('input[data-photo]'); if (!inp || !inp.files[0]) return;
    const thumb = inp.closest('.thumb'); thumb.classList.add('busy');
    try {
      const blob = await A.resize(inp.files[0]);
      const fd = new FormData(); fd.set('photo', blob, 'card.jpg');
      const j = await A.post('/admin/photo/' + inp.dataset.photo, fd);
      let img = thumb.querySelector('img');
      if (!img) { img = document.createElement('img'); img.alt = ''; thumb.prepend(img); thumb.querySelector('.thumb-empty')?.remove(); }
      img.src = j.src + '?t=' + Date.now();
      A.toast('Photo updated ✓', { kind: 'ok' });
    } catch (err) { A.toast(err.message, { kind: 'err' }); }
    finally { thumb.classList.remove('busy'); inp.value = ''; }
  });

  /* ----- selection ----- */
  const sel = new Set(); let allMatching = false; let lastId = null;
  const wrap = () => $('rows-wrap');
  const boxes = () => [...document.querySelectorAll('[data-sel]')];
  const total = () => +(wrap()?.dataset.total || 0);
  const selCount = () => (allMatching ? total() : sel.size);

  function sync() {
    for (const b of boxes()) { b.checked = allMatching || sel.has(+b.dataset.sel); b.closest('.crow').classList.toggle('sel', b.checked); }
    const n = selCount(); $('selCount').textContent = `${n.toLocaleString()} selected`; $('selbar').hidden = n === 0;
    document.body.classList.toggle('has-sel', n > 0);
    const all = $('selAll'), vis = boxes();
    if (all) { all.checked = vis.length > 0 && vis.every((b) => b.checked); all.indeterminate = !all.checked && vis.some((b) => b.checked); }
    const note = $('selNote');
    if (note) {
      if (allMatching) { note.hidden = false; note.replaceChildren(document.createTextNode(`All ${total().toLocaleString()} matching cards are selected. `), btn('Clear selection', clear)); }
      else if (all?.checked && total() > vis.length) { note.hidden = false; note.replaceChildren(document.createTextNode(`${vis.length} cards on this page are selected. `), btn(`Select all ${total().toLocaleString()}`, () => { allMatching = true; sync(); })); }
      else note.hidden = true;
    }
  }
  const btn = (txt, fn) => { const b = el('button', 'linkbtn inline', txt); b.type = 'button'; b.onclick = fn; return b; };
  function clear() { sel.clear(); allMatching = false; lastId = null; sync(); }

  document.addEventListener('click', (e) => {
    const b = e.target.closest('[data-sel]'); if (!b) return;
    const id = +b.dataset.sel;
    if (e.shiftKey && lastId != null) {          // shift-click selects a range, like a file manager
      const vis = boxes(), a = vis.findIndex((x) => +x.dataset.sel === lastId), z = vis.findIndex((x) => +x.dataset.sel === id);
      if (a >= 0 && z >= 0) for (let i = Math.min(a, z); i <= Math.max(a, z); i++) b.checked ? sel.add(+vis[i].dataset.sel) : sel.delete(+vis[i].dataset.sel);
    }
    if (allMatching && !b.checked) { allMatching = false; boxes().forEach((x) => x !== b && sel.add(+x.dataset.sel)); }
    b.checked ? sel.add(id) : sel.delete(id); lastId = id; sync();
  });
  document.addEventListener('change', (e) => {
    if (e.target.id !== 'selAll') return;
    const on = e.target.checked; boxes().forEach((b) => (on ? sel.add(+b.dataset.sel) : sel.delete(+b.dataset.sel)));
    if (!on) allMatching = false; sync();
  });
  document.addEventListener('htmx:afterSwap', () => { lastId = null; sync(); });

  /* ----- bulk actions ----- */
  // A tile can stand for several copies of the same card: act on every copy.
  const copiesOf = (id) => { const b = document.querySelector(`[data-sel="${id}"]`); return (b?.dataset.ids || String(id)).split(',').filter(Boolean).map(Number); };
  const target = () => (allMatching ? { scope: { q: wrap().dataset.q || '', binder: wrap().dataset.binder || '' } } : { ids: [...sel].flatMap(copiesOf) });
  async function refreshList() {
    const html = await (await fetch(location.pathname + location.search, { credentials: 'same-origin' })).text();
    const fresh = new DOMParser().parseFromString(html, 'text/html').getElementById('rows-wrap');
    if (fresh) { wrap().replaceWith(fresh); sync(); }
  }
  async function run(body, say) {
    const j = await A.postJSON('/admin/bulk', { ...target(), ...body });
    const msg = say(j);
    if (j.undo && j.n) A.toast(msg, { kind: 'ok', ms: 9000, undo: async () => {
      try { await A.postJSON('/admin/bulk', { action: 'restore', snapshot: j.undo }); await refreshList(); A.toast('Undone ✓', { kind: 'ok' }); } catch (e) { A.toast(e.message, { kind: 'err' }); }
    } });
    else A.toast(msg, { kind: 'ok' });
    await refreshList();
  }
  const plural = (n, w = 'card') => `${n.toLocaleString()} ${w}${n === 1 ? '' : 's'}`;
  const binderList = () => [...document.querySelectorAll('#ab option')].filter((o) => o.value).map((o) => [o.value, o.textContent]);
  function segment(opts, onPick, first) {
    const wrapEl = el('div', 'seg seg-wide'); let cur = first || opts[0][0];
    const draw = () => wrapEl.querySelectorAll('button').forEach((b) => { const on = b.dataset.v === cur; b.classList.toggle('on', on); b.setAttribute('aria-pressed', String(on)); });
    for (const [v, t] of opts) { const b = el('button', null, t); b.type = 'button'; b.dataset.v = v; b.onclick = () => { cur = v; draw(); onPick(v); }; wrapEl.append(b); }
    draw(); return wrapEl;
  }

  function binderSheet() {
    let mode = 'add'; const picked = new Set(); const node = el('div', 'sheet-form');
    const hint = el('p', 'hint show', 'Pick the binders to add these cards to.');
    node.append(segment([['add', 'Add to'], ['remove', 'Remove from'], ['move', 'Move to']], (v) => { mode = v; hint.textContent = { add: 'Pick the binders to add these cards to.', remove: 'Pick the binders to take these cards out of.', move: 'These cards will be in only the binders you pick.' }[v]; }), hint);
    const pills = el('div', 'pills');
    for (const [h, t] of binderList()) { const lab = el('label', 'pill'); const i = el('input'); i.type = 'checkbox'; i.onchange = () => (i.checked ? picked.add(h) : picked.delete(h)); lab.append(i, el('span', null, t)); pills.append(lab); }
    node.append(pills);
    A.sheet.open(`Binders · ${plural(selCount())}`, node, async () => {
      if (!picked.size) throw new Error('Pick at least one binder.');
      await run({ action: 'binder', mode, binders: [...picked] }, (j) => `Updated ${plural(j.n)}`); A.sheet.close();
    }, 'Apply');
  }

  function gradeSheet() {
    const node = el('div', 'sheet-form'); const chips = el('div', 'chips'); const inp = el('input'); inp.type = 'text'; inp.placeholder = 'Or type your own'; inp.className = 'sheet-input';
    for (const g of ['Near Mint', 'LP-NM', 'Lightly Played', 'MP-LP', 'Moderately Played', 'HP-MP', 'Damaged-HP', 'Sealed', 'PSA 10', 'PSA 9', 'CGC 10', 'CGC 9.5', 'BGS 9.5']) { const b = el('button', null, g); b.type = 'button'; b.onclick = () => { inp.value = g; chips.querySelectorAll('button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); }; chips.append(b); }
    const clearBtn = el('button', 'pbtn danger', 'Clear grade'); clearBtn.type = 'button'; clearBtn.onclick = () => { inp.value = ''; A.sheet.save(); };
    node.append(el('p', 'hint show', 'Sets the grade or condition on every selected card.'), chips, inp, clearBtn);
    A.sheet.open(`Grade · ${plural(selCount())}`, node, async () => { await run({ action: 'grade', text: inp.value }, (j) => `Updated ${plural(j.n)}`); A.sheet.close(); }, 'Apply');
  }

  function valueSheet() {
    let mode = 'set'; const node = el('div', 'sheet-form'); const inp = el('input'); inp.type = 'text'; inp.inputMode = 'decimal'; inp.className = 'sheet-input'; inp.placeholder = '₱ amount';
    const hint = el('p', 'hint show', 'Every selected card gets exactly this value.');
    const lab = el('label', 'sf-label'); lab.append(el('span', 'sf-l', 'Amount'), inp);
    node.append(segment([['set', 'Set to'], ['up', 'Raise %'], ['down', 'Lower %'], ['market', 'Market today']], (v) => {
      mode = v; lab.hidden = v === 'market'; inp.placeholder = v === 'set' ? '₱ amount' : '% change';
      hint.textContent = { set: 'Every selected card gets exactly this value.', up: 'Raises every value by this percentage.', down: 'Lowers every value by this percentage.',
        market: 'Looks up today’s market price for cards that were added with a card number, and updates their value. Up to 40 at a time.' }[v];
    }), lab, hint);
    A.sheet.open(`Value · ${plural(selCount())}`, node, async () => {
      const n = parseFloat(inp.value.replace(/[^0-9.]/g, ''));
      if (mode === 'market') {
        await run({ action: 'market' }, (j) => j.n ? `Updated ${plural(j.n)} to today’s market price${j.skipped ? ` · ${j.skipped} had no card number` : ''}${j.capped ? ' · run again for the rest' : ''}` : 'None of the selected cards have a card number to look up.');
      } else {
        if (!isFinite(n)) throw new Error('Enter a number.');
        await run(mode === 'set' ? { action: 'set_value', number: n } : { action: 'adjust_value', number: mode === 'up' ? n : -n }, (j) => `Updated ${plural(j.n)}`);
      }
      A.sheet.close();
    }, 'Apply');
  }

  function bulkDelete() {
    const n = selCount();
    const go = () => {
      const rows = boxes().filter((b) => b.checked).map((b) => b.closest('.crow'));
      const body = { ...target(), action: 'delete' };
      A.undoable({
        label: `Deleted ${plural(n)}`,
        hide: () => rows.forEach((r) => { r.style.height = r.offsetHeight + 'px'; requestAnimationFrame(() => r.classList.add('gone')); }),
        show: () => rows.forEach((r) => { r.classList.remove('gone'); r.style.height = ''; }),
        commit: async () => { await A.postJSON('/admin/bulk', body); clear(); await refreshList(); },
      });
    };
    if (n < 10) return go();
    const node = el('div', 'sheet-form'); node.append(el('p', 'confirm', `Delete ${plural(n)}? You’ll have a few seconds to undo.`));
    A.sheet.open('Delete cards', node, async () => { A.sheet.close(); go(); }, `Delete ${plural(n)}`);
  }

  document.addEventListener('click', (e) => {
    const b = e.target.closest('[data-bulk]'); if (!b) return;
    ({ binder: binderSheet, grade: gradeSheet, value: valueSheet, delete: bulkDelete, clear })[b.dataset.bulk]();
  });
  sync();
})();
