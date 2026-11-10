/* Admin › My cards: binder covers filter the grid; − / ＋ change how many copies; 📷 swaps the photo.
   (Selection and bulk actions live in admin-ui.js.) */
(() => {
  const root = document.getElementById('cardsRoot'); if (!root || root.dataset.mc) return; root.dataset.mc = '1';
  const A = window.Admin;
  const $ = (id) => document.getElementById(id);

  // Binder covers → the (hidden) binder select the search form already listens to.
  root.querySelector('.mc-binders').addEventListener('click', (e) => {
    const b = e.target.closest('.mc-b'); if (!b) return;
    root.querySelectorAll('.mc-b').forEach((x) => x.classList.toggle('on', x === b));
    const sel = $('ab'); sel.value = b.dataset.binder; sel.dispatchEvent(new Event('change', { bubbles: true }));
  });

  const idsOf = (tile) => (tile.dataset.ids || tile.dataset.id).split(',').filter(Boolean).map(Number);
  function setCount(tile, ids) {
    tile.dataset.ids = ids.join(','); tile.querySelector('[data-sel]').dataset.ids = tile.dataset.ids;
    const n = ids.length; tile.querySelector('[data-n]').textContent = n;
    const badge = tile.querySelector('[data-qty-badge]'); badge.hidden = n < 2; badge.textContent = '×' + n;
  }
  document.addEventListener('click', async (e) => {
    const plus = e.target.closest('[data-plus]'), minus = e.target.closest('[data-minus]');
    const tile = (plus || minus)?.closest('.ctile'); if (!tile) return;
    const ids = idsOf(tile);
    if (plus) {
      try { const j = await A.post('/admin/dup/' + ids[0]); setCount(tile, [...ids, j.id]); A.toast(`${ids.length + 1} copies ✓`, { kind: 'ok' }); }
      catch (err) { A.toast(err.message, { kind: 'err' }); }
      return;
    }
    if (ids.length > 1) {   // one copy fewer: remove the newest copy
      const gone = Math.max(...ids);
      try { await A.post('/admin/delete/' + gone); setCount(tile, ids.filter((x) => x !== gone)); A.toast(`${ids.length - 1} cop${ids.length - 1 === 1 ? 'y' : 'ies'} left`, { kind: 'ok' }); }
      catch (err) { A.toast(err.message, { kind: 'err' }); }
      return;
    }
    // last copy: delete the card, with Undo
    A.undoable({
      label: `Deleted “${tile.dataset.title}”`,
      hide: () => { tile.hidden = true; }, show: () => { tile.hidden = false; },
      commit: async () => { await A.post('/admin/delete/' + ids[0]); tile.remove(); },
    });
  });

  // Price and description: edit in place, saved as soon as you leave the box. Every copy of the card changes together.
  const flash = (n, ok) => { n.classList.remove('saved', 'bad'); void n.offsetWidth; n.classList.add(ok ? 'saved' : 'bad'); };
  async function quickSave(n, body) {
    const tile = n.closest('.ctile');
    try { await A.postJSON('/admin/quick-edit', { ids: idsOf(tile), ...body }); n.dataset.before = n.value; flash(n, true); A.toast('Saved ✓', { kind: 'ok' }); }
    catch (err) { n.value = n.dataset.before ?? n.defaultValue; flash(n, false); A.toast(err.message, { kind: 'err' }); }
  }
  document.addEventListener('focusin', (e) => { const n = e.target.closest('[data-price], [data-notes]'); if (n && n.dataset.before === undefined) n.dataset.before = n.defaultValue; });
  document.addEventListener('keydown', (e) => {
    const price = e.target.closest?.('[data-price]');
    if (price && e.key === 'Enter') { e.preventDefault(); price.blur(); }
    if (e.key === 'Escape' && e.target.closest?.('[data-price], [data-notes]')) { const n = e.target; n.value = n.dataset.before ?? n.defaultValue; n.blur(); }
  });
  document.addEventListener('focusout', (e) => {
    const price = e.target.closest?.('[data-price]'), notes = e.target.closest?.('[data-notes]');
    if (price) {
      const clean = price.value.replace(/[^\d.]/g, ''), v = Number(clean);
      if (clean === '' || !Number.isFinite(v)) { price.value = price.dataset.before ?? price.defaultValue; return; }
      if (clean === (price.dataset.before ?? price.defaultValue)) { price.value = clean; return; }
      price.value = clean; quickSave(price, { price: v });
    } else if (notes && notes.value !== (notes.dataset.before ?? notes.defaultValue)) quickSave(notes, { notes: notes.value });
  });

  // Condition: saved as soon as one is picked. The grade badge on the picture follows it.
  document.addEventListener('focusin', (e) => { const n = e.target.closest?.('[data-cond]'); if (n) n.dataset.before = n.value; });
  document.addEventListener('change', async (e) => {
    const sel = e.target.closest('[data-cond]'); if (!sel) return;
    const tile = sel.closest('.ctile'), before = sel.dataset.before ?? [...sel.options].find((o) => o.defaultSelected)?.value ?? '';
    try {
      const j = await A.postJSON('/admin/quick-edit', { ids: idsOf(tile), condition: sel.value });
      sel.dataset.before = sel.value; flash(sel, true); A.toast('Saved ✓', { kind: 'ok' });
      const box = tile.querySelector('.ct-img'); let badge = box.querySelector('.grade');
      if (j.grade) { if (!badge) { badge = document.createElement('span'); badge.className = 'grade'; box.append(badge); } badge.textContent = j.grade; } else badge?.remove();
      
      // A market-based price gets a "Price → ₱8?" chip for the new condition. Nothing changes until it's tapped.
      tile.querySelector('.mc-use-chip')?.remove();
      const src = tile.dataset.priceSrc;
      if ((src === 'market' || src === 'suggested') && tile.dataset.apiId) {
        const pr = tile.querySelector('[data-price]');
        const r = await fetch(`/api/price-suggest?id=${encodeURIComponent(tile.dataset.apiId)}&finish=${encodeURIComponent(tile.dataset.finish || '')}&cond=${encodeURIComponent(sel.value)}`, { credentials: 'same-origin', headers: { Accept: 'application/json' } });
        const s = r.ok ? await r.json() : null;
        const now = Number((pr?.value || '').replace(/[^\d.]/g, ''));
        if (pr && s?.suggested && s.suggested !== now) {
          const chip = document.createElement('button'); chip.className = 'use-chip mc-use-chip'; chip.type = 'button';
          chip.textContent = `Price → ₱${s.suggested.toLocaleString('en-US')}?`; chip.title = `${Math.round((s.share || 0) * 100)}% of Near Mint ₱${Math.round(s.near_mint).toLocaleString('en-US')}`;
          chip.onclick = async () => { chip.disabled = true; pr.value = String(s.suggested); await quickSave(pr, { price: s.suggested, price_src: 'suggested' }); tile.dataset.priceSrc = 'suggested'; chip.remove(); };
          tile.querySelector('.ct-pricebox')?.after(chip) ?? tile.append(chip);
        }
      }
    } catch (err) { sel.value = before; flash(sel, false); A.toast(err.message, { kind: 'err' }); }
  });

  // 📷: new photo for every copy of this card
  document.addEventListener('change', async (e) => {
    const inp = e.target.closest('input[data-photo2]'); if (!inp || !inp.files[0]) return;
    const tile = inp.closest('.ctile'); tile.classList.add('busy');
    try {
      const blob = await A.resize(inp.files[0]);
      let src = '';
      for (const id of idsOf(tile)) { const fd = new FormData(); fd.set('photo', blob, 'card.jpg'); const j = await A.post('/admin/photo/' + id, fd); src = j.src; }
      const box = tile.querySelector('.ct-img'); let img = box.querySelector('img');
      if (!img) { img = document.createElement('img'); img.alt = ''; box.prepend(img); box.querySelector('.thumb-empty')?.remove(); }
      img.src = src + '?t=' + Date.now(); A.toast('Photo updated ✓', { kind: 'ok' });
    } catch (err) { A.toast(err.message, { kind: 'err' }); }
    finally { tile.classList.remove('busy'); inp.value = ''; }
  });

  /* ---------- the owner's order: drag a card by its handle ---------- */
  function arrange() {
    const list = document.getElementById('rows');
    if (!list || !window.dragSort) return;
    if ((root.dataset.sort || '') !== '' || document.getElementById('as')?.value) { root.classList.add('no-drag'); return; }
    root.classList.remove('no-drag');
    window.dragSort(list, {
      item: '.ctile', handle: '.ct-grip', key: (li) => li.dataset.ids || li.dataset.id,
      onDrop: async ([keys]) => {
        try { await A.postJSON('/admin/order', { groups: keys.map((k) => k.split(',').filter(Boolean).map(Number)) }); A.toast('Order saved ✓', { kind: 'ok' }); }
        catch (err) { A.toast(err.message, { kind: 'err' }); }
      },
    });
  }
  arrange();
  document.addEventListener('htmx:afterSwap', () => { root.dataset.sort = document.getElementById('as')?.value || ''; arrange(); });
  document.getElementById('as')?.addEventListener('change', (e) => { root.dataset.sort = e.target.value; });

  /* ---------- tags: type one, press Enter ---------- */
  document.addEventListener('tags:change', async (e) => {
    const box = e.target.closest?.('.ctile [data-tag-box]'); if (!box) return;
    const tile = box.closest('.ctile');
    try { await A.postJSON('/admin/quick-edit', { ids: idsOf(tile), tags: e.detail.tags }); A.toast('Tags saved ✓', { kind: 'ok' }); }
    catch (err) { A.toast(err.message, { kind: 'err' }); }
  });
})();
