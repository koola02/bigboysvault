/* Admin › Binders. Two grids that look exactly like the home page's Binders section:
   "On your home page" (in order) and "Hidden". Drag a binder to move it (also between the two grids). Tap to edit.
   Every change saves straight away. */
(() => {
  const root = document.getElementById('bd'); if (!root || root.dataset.ready) return; root.dataset.ready = '1';
  const $ = (id) => document.getElementById(id);
  const A = window.Admin || { toast: () => {} };
  const home = $('bdHome'), hidden = $('bdHidden'), sheet = $('bdSheet');
  const f = { title: $('bdSheetT'), name: $('bdName'), onHome: $('bdOnHome'), file: $('bdFile'), auto: $('bdAuto'), state: $('bdState'), pick: $('bdPick'), del: $('bdDelete'), done: $('bdDone') };
  let binders = JSON.parse(root.dataset.binders || '[]');
  let editing = null;          // handle being edited, or '' for a new binder
  let lastFocus = null;

  const json = { 'Content-Type': 'application/json', Accept: 'application/json' };
  async function call(url, body) {
    const r = await fetch(url, { method: 'POST', credentials: 'same-origin', headers: json, body: JSON.stringify(body) });
    const j = await r.json().catch(() => ({}));
    if (!r.ok || !j.ok) throw new Error(j.error || 'That didn’t save. Try again.');
    if (j.binders) { binders = j.binders; draw(); }
    return j;
  }
  const send = (h, body) => call('/admin/binders/' + encodeURIComponent(h), body);

  /* ---------- the two grids ---------- */
  function tile(b) {
    const li = document.createElement('li'); li.className = 'bd-tile'; li.dataset.h = b.handle;
    const btn = document.createElement('button'); btn.type = 'button'; btn.className = 'binder bd-card-btn';
    btn.setAttribute('aria-label', `${b.title}, ${b.count} cards. Tap to edit, or drag to move.`);
    if (b.image) { const i = document.createElement('img'); i.src = b.image; i.alt = ''; i.draggable = false; btn.append(i); }
    else { const n = document.createElement('span'); n.className = 'bd-nopic'; n.textContent = 'No picture yet'; btn.append(n); }
    const t = document.createElement('span'); t.className = 'binder-t'; t.textContent = b.title;
    const s = document.createElement('small'); s.textContent = `${b.count} card${b.count === 1 ? '' : 's'}`; t.append(s);
    const e = document.createElement('span'); e.className = 'bd-edit-tag'; e.textContent = 'Edit';
    const sp = document.createElement('i'); sp.className = 'bnd'; sp.setAttribute('aria-hidden', 'true');
    btn.append(t, e, sp); btn.addEventListener('click', () => openEdit(b.handle));
    li.append(btn); return li;
  }
  function addTile() {
    const li = document.createElement('li'); li.className = 'bd-add-li';
    const b = document.createElement('button'); b.type = 'button'; b.className = 'bd-add';
    b.innerHTML = '<span class="bd-plus" aria-hidden="true">+</span><span>New binder</span>';
    b.addEventListener('click', () => openEdit('')); li.append(b); return li;
  }
  function draw() {
    const on = binders.filter((b) => b.on_home), off = binders.filter((b) => !b.on_home);
    home.replaceChildren(...on.map(tile), addTile());
    hidden.replaceChildren(...off.map(tile));
    if (!off.length) { const p = document.createElement('li'); p.className = 'bd-empty'; p.textContent = 'Drag a binder here to hide it from the home page.'; hidden.append(p); }
    $('bdHomeN').textContent = on.length; $('bdHiddenN').textContent = off.length;
  }
  draw();

  if (window.dragSort) window.dragSort([home, hidden], { item: '.bd-tile', key: (li) => li.dataset.h, onDrop: async ([onList, offList], li) => {
    const moved = binders.find((b) => b.handle === li.dataset.h);
    try {
      await call('/admin/binders/order', { handles: [...onList, ...offList], home: onList });
      const nowOn = onList.includes(li.dataset.h);
      A.toast(moved && moved.on_home !== nowOn ? (nowOn ? `“${moved.title}” is on the home page ✓` : `“${moved.title}” is hidden ✓`) : 'Order saved ✓', { kind: 'ok' });
    } catch (err) { A.toast(err.message, { kind: 'err' }); draw(); }
  } });

  /* ---------- the edit sheet ---------- */
  function openEdit(handle) {
    editing = handle; lastFocus = document.activeElement;
    const b = binders.find((x) => x.handle === handle);
    const isNew = !b;
    sheet.classList.toggle('is-new', isNew);
    f.title.textContent = isNew ? 'New binder' : 'Edit binder';
    f.name.value = b ? b.title : ''; f.name.placeholder = 'For example: Eeveelutions';
    f.done.textContent = isNew ? 'Create binder' : 'Done';
    if (b) { f.onHome.checked = b.on_home; loadPicks(b); }
    sheet.hidden = false; requestAnimationFrame(() => sheet.classList.add('open')); document.body.style.overflow = 'hidden';
    if (isNew) setTimeout(() => f.name.focus(), 250);
  }
  function closeEdit() {
    sheet.classList.remove('open'); document.body.style.overflow = ''; setTimeout(() => { sheet.hidden = true; }, 220);
    editing = null; if (lastFocus && document.contains(lastFocus)) lastFocus.focus({ preventScroll: true });
  }
  async function loadPicks(b) {
    f.pick.replaceChildren(); f.state.textContent = 'Loading the cards in this binder…';
    try {
      const r = await fetch(`/admin/binders/${encodeURIComponent(b.handle)}/covers`, { headers: { Accept: 'application/json' }, credentials: 'same-origin' });
      const j = await r.json(); if (editing !== b.handle) return;
      const cards = j.cards || [];
      f.state.textContent = cards.length ? '' : 'No card in this binder has a photo yet. Upload a photo instead.';
      f.pick.replaceChildren(...cards.map((c) => {
        const btn = document.createElement('button'); btn.type = 'button'; btn.className = 'bd-card'; btn.title = c.title;
        const i = document.createElement('img'); i.src = c.image; i.alt = c.title; i.loading = 'lazy';
        btn.append(i); btn.addEventListener('click', () => setPicture({ cover: String(c.id) }, 'Picture changed ✓')); return btn;
      }));
    } catch (_) { f.state.textContent = 'Couldn’t load the cards. Check your connection.'; }
  }
  async function setPicture(body, msg) {
    try { await send(editing, body); A.toast(msg, { kind: 'ok' }); closeEdit(); }
    catch (err) { A.toast(err.message, { kind: 'err' }); }
  }
  async function saveName() {
    const v = f.name.value.trim();
    if (editing === '') {
      if (!v) { f.name.focus(); return A.toast('Type a name for the new binder.', { kind: 'err' }); }
      try { await call('/admin/binders', { title: v }); A.toast(`“${v}” added. Add cards to it from Quick add or Add by set.`, { kind: 'ok', ms: 4500 }); closeEdit(); }
      catch (err) { A.toast(err.message, { kind: 'err' }); }
      return;
    }
    const b = binders.find((x) => x.handle === editing);
    if (!b || v === b.title) return;
    if (!v) { f.name.value = b.title; return A.toast('A binder needs a name.', { kind: 'err' }); }
    try { await send(editing, { title: v }); A.toast('Name saved ✓', { kind: 'ok' }); } catch (err) { A.toast(err.message, { kind: 'err' }); }
  }
  f.name.addEventListener('change', () => { if (editing) saveName(); });
  f.name.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); if (editing === '') saveName(); else { f.name.blur(); } } });
  f.done.addEventListener('click', async () => { if (editing === '') return saveName(); await saveName(); closeEdit(); });
  f.onHome.addEventListener('change', async () => {
    try { await send(editing, { on_home: f.onHome.checked }); A.toast(f.onHome.checked ? 'Now on the home page ✓' : 'Hidden from the home page', { kind: 'ok' }); }
    catch (err) { f.onHome.checked = !f.onHome.checked; A.toast(err.message, { kind: 'err' }); }
  });
  f.auto.addEventListener('click', () => setPicture({ cover: 'auto' }, 'Picture set to the most valuable card ✓'));
  f.file.addEventListener('change', async () => {
    const file = f.file.files[0]; if (!file) return;
    f.state.textContent = 'Uploading…';
    try {
      const small = A.resize ? await A.resize(file, 1600).catch(() => file) : file;
      const fd = new FormData(); fd.append('photo', small, 'cover.jpg');
      const r = await fetch(`/admin/binders/${encodeURIComponent(editing)}/cover`, { method: 'POST', body: fd, headers: { Accept: 'application/json' }, credentials: 'same-origin' });
      const j = await r.json().catch(() => ({}));
      if (!r.ok || !j.ok) throw new Error(j.error || 'Upload failed. Try again.');
      binders = j.binders; draw(); A.toast('Picture uploaded ✓', { kind: 'ok' }); closeEdit();
    } catch (err) { f.state.textContent = err.message; }
    finally { f.file.value = ''; }
  });
  f.del.addEventListener('click', async () => {
    const b = binders.find((x) => x.handle === editing); if (!b) return;
    if (!confirm(`Delete “${b.title}”?\n\nThe ${b.count} card${b.count === 1 ? '' : 's'} in it stay in your collection. Only the binder goes away.`)) return;
    try { await send(editing, { delete: true }); A.toast(`“${b.title}” deleted`, { kind: 'ok' }); closeEdit(); }
    catch (err) { A.toast(err.message, { kind: 'err' }); }
  });
  sheet.addEventListener('click', (e) => { if (e.target.closest('[data-close]')) closeEdit(); });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !sheet.hidden) closeEdit(); });
})();
