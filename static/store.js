// The seller panel: listings, binders, orders, settings and notifications. Everything saves right on the page.
(() => {
  const A = window.Admin;
  const $ = (id) => document.getElementById(id);
  const el = (tag, cls, txt) => { const n = document.createElement(tag); if (cls) n.className = cls; if (txt != null) n.textContent = txt; return n; };
  const peso = (n) => '₱' + Math.round(Number(n) || 0).toLocaleString('en-US');
  async function send(url, body) {
    const opts = { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' } };
    if (body instanceof FormData) opts.body = body;
    else { opts.headers['Content-Type'] = 'application/json'; opts.body = JSON.stringify(body ?? {}); }
    let r;
    try { r = await fetch(url, opts); } catch (_) { throw new Error('No connection. Please try again.'); }
    const j = await r.json().catch(() => ({}));
    if (r.status === 401) { location.href = '/login'; throw new Error('Please sign in again.'); }
    if (!r.ok) throw new Error(j.error || 'That didn’t save. Please try again.');
    return j;
  }
  const flash = (n, good) => { n.classList.remove('saved', 'failed'); void n.offsetWidth; n.classList.add(good ? 'saved' : 'failed'); };
  // Phone photos are huge: shrink here first, so the upload is quick on mobile data.
  const shrink = (file, max = 1600) => A.resize(file, max);

  /* ================= listings ================= */
  const root = $('listings');
  if (root) {
    const rows = $('rows');
    const tileOf = (n) => n.closest('.ls-tile');
    const idOf = (n) => tileOf(n).dataset.id;

    // The suggestion line under each price, from the shared rule in /static/pricing.js.
    function paintSug(tile) {
      const host = tile.querySelector('.st-sug'); if (!host || !window.Price) return;
      const own = Number(tile.dataset.own) || 0, market = Number(tile.dataset.market) || 0;
      window.Price.line(host, {
        nm: market, cond: tile.dataset.cond, own, ownPhoto: tile.hasAttribute('data-own-photo'), price: own,
        onUse: own > 0 ? () => savePrice(tile, 0) : null,
      });
    }
    const paintAll = () => rows.querySelectorAll('.ls-tile').forEach(paintSug);
    document.addEventListener('bbv:shares', paintAll);

    // Each save on a card gets a number. An answer repaints the card only if no newer answer already did, so an old one never undoes a new one.
    const turn = (tile) => (tile._seq = (tile._seq || 0) + 1);
    const fresh = (tile, my) => { if (my <= (tile._painted || 0)) return false; tile._painted = my; return true; };

    // Stock: quick taps wait a moment, then the last count is sent once. One card's stock saves go one at a time.
    const qtyOf = (v) => Math.min(9999, Math.max(0, Math.round(Number(v) || 0)));
    const qtyState = new WeakMap();
    const qs = (tile) => { let s = qtyState.get(tile); if (!s) qtyState.set(tile, (s = { timer: 0, busy: false, again: false })); return s; };
    const qtyWaiting = new Set();
    // While the seller is typing, tapping, or a stock save is on its way, the box keeps what they put in it.
    const qtyHeld = (tile, inp) => { const s = qtyState.get(tile); return document.activeElement === inp || !!(s && (s.timer || s.busy)); };
    function queueQty(tile, inp, wait) {
      const s = qs(tile); clearTimeout(s.timer); qtyWaiting.add(tile);
      s.timer = setTimeout(() => { s.timer = 0; qtyWaiting.delete(tile); saveQty(tile, inp); }, wait);
    }
    async function saveQty(tile, inp) {
      const s = qs(tile);
      if (s.busy) { s.again = true; qtyWaiting.add(tile); return; } // the save on its way sends the newest count when it's back
      s.busy = true;
      let card = null, my = 0, ok = true;
      try {
        do {
          s.again = false; qtyWaiting.delete(tile); my = turn(tile);
          card = (await send(`/me/listings/${tile.dataset.id}`, { qty: qtyOf(inp.value) })).card;
          if (card && fresh(tile, my)) apply(tile, card); // the box stays as typed: a save is still on its way
        } while (s.again && !s.timer);
      } catch (err) { ok = false; A.toast(err.message, { kind: 'err' }); }
      s.busy = false; s.again = false; if (!s.timer) qtyWaiting.delete(tile);
      if (ok) { if (card && !qtyHeld(tile, inp)) inp.value = tile.dataset.qty; flash(inp, true); if (!s.timer) A.toast('Saved ✓', { kind: 'ok' }); }
      else { if (!qtyHeld(tile, inp)) inp.value = tile.dataset.qty; flash(inp, false); }
    }
    // Leaving the page with a count still waiting: send it on the way out.
    window.addEventListener('pagehide', () => {
      const a = document.activeElement; // a count typed but not saved yet (no change event fires on the way out)
      if (a && a.matches && a.matches('[data-qty-in]') && a.value.trim() !== '') { const t = tileOf(a); if (t && String(qtyOf(a.value)) !== String(t.dataset.qty)) qtyWaiting.add(t); }
      qtyWaiting.forEach((tile) => {
        const s = qs(tile); clearTimeout(s.timer); s.timer = 0; s.again = false;
        const inp = tile.querySelector('[data-qty-in]');
        fetch(`/me/listings/${tile.dataset.id}`, { method: 'POST', credentials: 'same-origin', keepalive: true, headers: { 'Content-Type': 'application/json', Accept: 'application/json' }, body: JSON.stringify({ qty: qtyOf(inp.value) }) }).catch(() => {});
      });
      qtyWaiting.clear();
    });
    // Coming back with the Back button shows the page as it was: load it fresh so stock and status are current.
    window.addEventListener('pageshow', (e) => { if (e.persisted) location.reload(); });

    function apply(tile, card) {
      Object.assign(tile.dataset, { status: card.status, market: card.market, own: card.value_own, cond: card.cond, qty: card.qty, kind: card.kind });
      const st = tile.querySelector('.st-status'); st.textContent = card.status; st.dataset.s = card.status;
      tile.querySelector('[data-status]').value = card.status;
      tile.querySelectorAll('[data-kind]').forEach((b) => { const on = b.dataset.kind === card.kind; b.classList.toggle('on', on); b.setAttribute('aria-pressed', String(on)); });
      const sale = tile.querySelector('.ls-sale'); if (sale) sale.hidden = card.kind !== 'sale';
      // The switch shows "Out of stock" when the seller said so, when it's sold, or when no copies are left.
      const stock = tile.querySelector('[data-stock]'); if (stock) { const off = card.status === 'Out of stock' || card.status === 'Sold' || Number(card.qty) <= 0; stock.classList.toggle('off', off); stock.setAttribute('aria-pressed', String(!off)); }
      const qi = tile.querySelector('[data-qty-in]'); if (!qtyHeld(tile, qi)) qi.value = card.qty;
      const pr = tile.querySelector('[data-price]'); pr.value = card.value_own > 0 ? String(Math.round(card.value_own)) : ''; pr.placeholder = card.market > 0 ? card.price_text : 'Price';
      // Same words as the page: an expired card, or one past its end date, needs renewing.
      const late = card.status === 'Expired' || (card.expires_at > 0 && card.expires_at < Date.now() / 1000);
      const renew = tile.querySelector('[data-renew]'); renew.textContent = late ? 'Renew for 30 days' : card.expires_at > 0 ? 'Change end date' : 'Set an end date';
      const exp = tile.querySelector('.st-exp');
      if (card.expires_text) { if (exp) exp.textContent = card.expires_text; else { tile.querySelector('.st-meta').append(el('span', 'st-exp', card.expires_text)); } }
      else exp?.remove();
      paintSug(tile);
    }
    async function patch(tile, body, node) {
      const my = turn(tile);
      try { const j = await send(`/me/listings/${tile.dataset.id}`, body); if (fresh(tile, my)) apply(tile, j.card); if (node) flash(node, true); A.toast('Saved ✓', { kind: 'ok' }); return j.card; }
      catch (err) { if (node) flash(node, false); A.toast(err.message, { kind: 'err' }); return null; }
    }
    const savePrice = (tile, v) => patch(tile, { price: v }, tile.querySelector('[data-price]'));

    rows.addEventListener('change', async (e) => {
      const t = e.target, tile = tileOf(t); if (!tile) return;
      if (t.matches('[data-price]')) { const v = Number(t.value.replace(/[^\d.]/g, '')); if (Number.isFinite(v)) savePrice(tile, v); }
      else if (t.matches('[data-cond]')) {
        if (t.value === '__own') { const v = (prompt('Condition or grade, like PSA 9') || '').trim().slice(0, 40); if (!v) { t.value = tile.dataset.cond; return; } t.insertBefore(new Option(v, v, true, true), t.firstChild); t.value = v; }
        patch(tile, { cond: t.value }, t);
      }
      else if (t.matches('[data-qty-in]')) { t.value = qtyOf(t.value); queueQty(tile, t, 0); }
      else if (t.matches('[data-status]')) patch(tile, { status: t.value }, t);
      else if (t.matches('[data-binder]')) patch(tile, { binder_id: t.value ? Number(t.value) : null }, t);
      else if (t.matches('[data-notes]')) patch(tile, { notes: t.value }, t);
    });
    rows.addEventListener('keydown', (e) => { if (e.key === 'Enter' && e.target.matches('[data-price], [data-qty-in]')) { e.preventDefault(); e.target.blur(); } });
    rows.addEventListener('click', async (e) => {
      const tile = tileOf(e.target); if (!tile) return;
      const kind = e.target.closest('button[data-kind]');
      if (kind) { if (!kind.classList.contains('on')) patch(tile, { kind: kind.dataset.kind }, kind); return; }
      const stock = e.target.closest('button[data-stock]');
      if (stock) { patch(tile, { status: stock.classList.contains('off') ? 'Active' : 'Out of stock' }, stock); return; }
      const q = e.target.closest('button[data-qty]');
      if (q) { const inp = tile.querySelector('[data-qty-in]'); inp.value = qtyOf(qtyOf(inp.value) + Number(q.dataset.qty)); queueQty(tile, inp, 400); return; }
      if (e.target.closest('[data-renew]')) { renewSheet([tile]); return; }
      if (e.target.closest('[data-del]')) {
        const id = tile.dataset.id;
        A.undoable({
          label: `Removed “${tile.querySelector('.ct-t').textContent}”`,
          hide: () => { tile.style.height = tile.offsetHeight + 'px'; requestAnimationFrame(() => tile.classList.add('gone')); },
          show: () => { tile.classList.remove('gone'); tile.style.height = ''; },
          commit: async () => { await send('/me/listings/bulk', { ids: [Number(id)], action: 'delete' }); tile.remove(); count(); },
        });
      }
    });

    /* ----- selection and bulk actions ----- */
    const sel = new Set();
    const boxes = () => [...rows.querySelectorAll('[data-sel]')];
    function sync() {
      boxes().forEach((b) => { b.checked = sel.has(+b.dataset.sel); b.closest('.ls-tile').classList.toggle('sel', b.checked); });
      $('selCount').textContent = `${sel.size} selected`; $('selbar').hidden = sel.size === 0; document.body.classList.toggle('has-sel', sel.size > 0);
      const all = $('selAll'); if (all) { const vis = boxes(); all.checked = vis.length > 0 && vis.every((b) => b.checked); all.indeterminate = !all.checked && vis.some((b) => b.checked); }
    }
    const clear = () => { sel.clear(); sync(); };
    rows.addEventListener('change', (e) => { const b = e.target.closest('[data-sel]'); if (!b) return; b.checked ? sel.add(+b.dataset.sel) : sel.delete(+b.dataset.sel); sync(); });
    $('selAll')?.addEventListener('change', (e) => { boxes().forEach((b) => (e.target.checked ? sel.add(+b.dataset.sel) : sel.delete(+b.dataset.sel))); sync(); });
    const tilesSel = () => [...sel].map((id) => rows.querySelector(`.ls-tile[data-id="${id}"]`)).filter(Boolean);
    const plural = (n, w = 'card') => `${n.toLocaleString()} ${w}${n === 1 ? '' : 's'}`;
    async function bulk(body, msg) {
      const j = await send('/me/listings/bulk', { ids: [...sel], ...body });
      A.toast(msg(j), { kind: 'ok' }); A.sheet.close(); await refreshList(); clear();
    }
    function segment(opts, onPick, first) {
      const w = el('div', 'seg seg-wide'); let cur = first || opts[0][0];
      const draw = () => w.querySelectorAll('button').forEach((b) => { const on = b.dataset.v === cur; b.classList.toggle('on', on); b.setAttribute('aria-pressed', String(on)); });
      for (const [v, t] of opts) { const b = el('button', null, t); b.type = 'button'; b.dataset.v = v; b.onclick = () => { cur = v; draw(); onPick(v); }; w.append(b); }
      draw(); return w;
    }
    function chips(list, inp) {
      const c = el('div', 'chips');
      for (const g of list) { const b = el('button', null, g); b.type = 'button'; b.onclick = () => { inp.value = g; c.querySelectorAll('button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); }; c.append(b); }
      return c;
    }
    const CONDS = [...document.querySelector('.ls-tile [data-cond]')?.options || []].map((o) => o.value).filter((v) => v && v !== '__own');
    const STATUSES = [...document.querySelector('.ls-tile [data-status]')?.options || []].map((o) => o.value);
    const BINDERS = [...document.querySelector('.ls-tile [data-binder]')?.options || []].map((o) => [o.value, o.textContent]);
    const sheets = {
      price() {
        let mode = 'set'; const node = el('div', 'sheet-form'); const inp = el('input', 'sheet-input'); inp.inputMode = 'decimal'; inp.placeholder = '₱ amount';
        const hint = el('p', 'hint show', 'Every selected card gets exactly this price.');
        node.append(segment([['set', 'Set to'], ['up', 'Raise %'], ['down', 'Lower %'], ['market', 'Follow the market']], (v) => {
          mode = v; inp.hidden = v === 'market'; inp.placeholder = v === 'set' ? '₱ amount' : '% change';
          hint.textContent = { set: 'Every selected card gets exactly this price.', up: 'Raises every price by this percentage.', down: 'Lowers every price by this percentage.', market: 'Clears your own prices: these cards follow today’s market price for their condition.' }[v];
        }), inp, hint);
        A.sheet.open(`Price · ${plural(sel.size)}`, node, async () => {
          const n = parseFloat(inp.value.replace(/[^0-9.]/g, ''));
          if (mode === 'market') await bulk({ action: 'edit', price: 0 }, (j) => `Updated ${plural(j.n)}`);
          else { if (!isFinite(n)) throw new Error('Type a number.'); await bulk(mode === 'set' ? { action: 'edit', price: n } : { action: 'reprice', percent: mode === 'up' ? n : -n }, (j) => `Updated ${plural(j.n)}`); }
        }, 'Apply');
      },
      cond() {
        const node = el('div', 'sheet-form'); const inp = el('input', 'sheet-input'); inp.placeholder = 'Or type your own, like PSA 9';
        node.append(el('p', 'hint show', 'Sets the condition on every selected card. Prices that follow the market follow it.'), chips(CONDS, inp), inp);
        A.sheet.open(`Condition · ${plural(sel.size)}`, node, async () => { if (!inp.value.trim()) throw new Error('Pick a condition.'); await bulk({ action: 'edit', cond: inp.value.trim() }, (j) => `Updated ${plural(j.n)}`); }, 'Apply');
      },
      status() {
        const node = el('div', 'sheet-form'); const inp = el('input', 'sheet-input'); inp.hidden = true;
        node.append(el('p', 'hint show', 'Hidden cards stay in your store but nobody else sees them. Sold sets the stock to 0.'), chips(STATUSES.filter((x) => x !== 'Reserved'), inp));
        A.sheet.open(`Status · ${plural(sel.size)}`, node, async () => { if (!inp.value) throw new Error('Pick a status.'); await bulk({ action: 'edit', status: inp.value }, (j) => `Updated ${plural(j.n)}`); }, 'Apply');
      },
      binder() {
        const node = el('div', 'sheet-form'); const s = el('select', 'sheet-input'); for (const [v, t] of BINDERS) s.append(new Option(t, v));
        node.append(el('p', 'hint show', 'Moves every selected card into this binder.'), s);
        A.sheet.open(`Binder · ${plural(sel.size)}`, node, async () => { await bulk({ action: 'edit', binder_id: s.value ? Number(s.value) : null }, (j) => `Moved ${plural(j.n)}`); }, 'Move');
      },
      stock() {
        const node = el('div', 'sheet-form'); const inp = el('input', 'sheet-input'); inp.type = 'number'; inp.min = '0'; inp.max = '9999'; inp.inputMode = 'numeric'; inp.placeholder = 'Copies in stock';
        node.append(el('p', 'hint show', 'Every selected card gets this many copies in stock. 0 means sold out.'), inp);
        A.sheet.open(`Stock · ${plural(sel.size)}`, node, async () => { const n = Math.min(9999, Math.round(Number(inp.value))); if (!Number.isFinite(n) || n < 0) throw new Error('Type how many.'); await bulk({ action: 'edit', qty: n }, (j) => `Updated ${plural(j.n)}`); }, 'Apply');
      },
      expiry() { renewSheet(tilesSel(), true); },
      delete() {
        const n = sel.size, tiles = tilesSel(), ids = [...sel];
        A.undoable({
          label: `Removed ${plural(n)}`,
          hide: () => tiles.forEach((t) => { t.style.height = t.offsetHeight + 'px'; requestAnimationFrame(() => t.classList.add('gone')); }),
          show: () => tiles.forEach((t) => { t.classList.remove('gone'); t.style.height = ''; }),
          commit: async () => { await send('/me/listings/bulk', { ids, action: 'delete' }); tiles.forEach((t) => t.remove()); clear(); count(); },
        });
        clear();
      },
      kind() {
        const node = el('div', 'sheet-form'); const inp = el('input', 'sheet-input'); inp.hidden = true;
        const chips = el('div', 'chips');
        for (const [v, t] of [['own', 'Highlight'], ['sale', 'For sale'], ['wish', 'Wishlist']]) { const b = el('button', null, t); b.type = 'button'; b.onclick = () => { inp.value = v; chips.querySelectorAll('button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); }; chips.append(b); }
        node.append(el('p', 'hint show', 'Highlights are shown off, For sale goes in your store, Wishlist is what you’re after.'), chips);
        A.sheet.open(`What are they · ${plural(sel.size)}`, node, async () => { if (!inp.value) throw new Error('Pick one.'); await bulk({ action: 'edit', kind: inp.value }, (j) => `Updated ${plural(j.n)}`); }, 'Apply');
      },
      clear,
    };
    document.addEventListener('click', (e) => { const b = e.target.closest('[data-bulk]'); if (b && sheets[b.dataset.bulk]) sheets[b.dataset.bulk](); });
    function renewSheet(tiles, many) {
      const node = el('div', 'sheet-form'); const inp = el('input', 'sheet-input'); inp.hidden = true;
      node.append(el('p', 'hint show', 'When the date comes, the card comes off sale and you get a note to renew it. Pick “No end date” to keep it up for good.'),
        chips(['30 days', '60 days', '90 days', 'No end date'], inp));
      A.sheet.open(many ? `End date · ${plural(sel.size)}` : 'End date', node, async () => {
        if (!inp.value) throw new Error('Pick one.');
        const days = parseInt(inp.value) || 0;
        if (many) await bulk({ action: 'edit', expires_days: days }, (j) => `Updated ${plural(j.n)}`);
        else { const t = tiles[0]; const my = turn(t); const j = await send(`/me/listings/${t.dataset.id}/renew`, { days }); if (fresh(t, my)) apply(t, j.card); A.sheet.close(); A.toast(days ? `Ends in ${days} days ✓` : 'No end date ✓', { kind: 'ok' }); }
      }, 'Apply');
    }
    function count() { const n = rows.querySelectorAll('.ls-tile').length; $('shownN').textContent = n; const empty = $('listEmpty'); if (empty && n > 0) empty.hidden = true; }
    async function refreshList() {
      const html = await (await fetch(location.pathname + location.search, { credentials: 'same-origin' })).text();
      const doc = new DOMParser().parseFromString(html, 'text/html');
      const fresh = doc.getElementById('rows'); if (fresh) { rows.replaceWith(fresh); }
      const emptyNow = doc.getElementById('listEmpty'), emptyHere = $('listEmpty');
      if (emptyHere && !emptyNow) emptyHere.remove(); else if (emptyNow && !emptyHere) $('rows').before(emptyNow);
      root.dataset.total = doc.getElementById('listings')?.dataset.total || root.dataset.total;
      // The new list is a new node: listeners above are on the document or re-looked-up, except `rows`.
      location.reload();
    }

    /* ----- filters ----- */
    const filters = $('listFilters');
    filters.addEventListener('change', () => filters.requestSubmit());
    let ft; filters.q.addEventListener('input', () => { clearTimeout(ft); ft = setTimeout(() => filters.requestSubmit(), 500); });

    /* ----- adding cards: their own page now (/me/add); an old link to Listings' add panel goes there ----- */
    if (location.hash === '#add' || location.hash === '#scan') { location.replace('/me/add' + (location.hash === '#scan' ? '#scan' : '')); return; }

    paintAll(); sync();
    const focus = root.dataset.focus;
    if (focus && focus !== '0') { const t = rows.querySelector(`.ls-tile[data-id="${focus}"]`); if (t) { t.classList.add('focus'); t.scrollIntoView({ block: 'center' }); } }
  }

  /* ================= binders ================= */
  const bRoot = $('storeBinders');
  if (bRoot) {
    $('newBinder').addEventListener('submit', async (e) => {
      e.preventDefault();
      try { await send('/me/binders', { title: e.target.title.value }); location.reload(); } catch (err) { A.toast(err.message, { kind: 'err' }); }
    });
    bRoot.addEventListener('change', async (e) => {
      const t = e.target, b = t.closest('.st-binder'); if (!b) return;
      const body = t.matches('[data-title]') ? { title: t.value } : t.matches('[data-desc]') ? { description: t.value } : t.matches('[data-vis]') ? { visibility: t.value } : null;
      if (!body) return;
      try { await send(`/me/binders/${b.dataset.id}/edit`, body); flash(t, true); A.toast('Saved ✓', { kind: 'ok' }); } catch (err) { flash(t, false); A.toast(err.message, { kind: 'err' }); }
    });
    bRoot.addEventListener('click', async (e) => {
      const b = e.target.closest('.st-binder');
      if (e.target.closest('[data-share]') && b) {
        const url = bRoot.dataset.site + bRoot.dataset.store + '/binders/' + b.dataset.slug;
        try { await navigator.clipboard.writeText(url); A.toast('Link copied ✓', { kind: 'ok' }); } catch (_) { prompt('Copy this link', url); }
      }
      if (e.target.closest('[data-del-binder]') && b) {
        if (!(await (window.bbvConfirm ? window.bbvConfirm('Delete this binder? The cards stay in your store.', { yes: 'Delete binder', no: 'Keep it' }) : Promise.resolve(confirm('Delete this binder? The cards stay in your store.'))))) return;
        try { await send(`/me/binders/${b.dataset.id}/delete`, {}); location.reload(); } catch (err) { A.toast(err.message, { kind: 'err' }); }
      }
      const cover = e.target.closest('[data-cover-btn]');
      if (cover && b) {
        const li = cover.closest('.me-c');
        try { await send(`/me/binders/${b.dataset.id}/cover`, { card_id: Number(li.dataset.id) }); b.querySelectorAll('.me-c').forEach((x) => x.toggleAttribute('data-cover', x === li)); const img = li.querySelector('img'); if (img) { const c = b.querySelector('.st-binder-cover'); c.replaceChildren(img.cloneNode()); } A.toast('Cover set ✓', { kind: 'ok' }); }
        catch (err) { A.toast(err.message, { kind: 'err' }); }
      }
    });
    if (window.dragSort) bRoot.querySelectorAll('.st-sort[data-binder]').forEach((list) => {
      const bid = list.dataset.binder; if (!bid) return;
      dragSort(list, { item: '.me-c', key: (n) => n.dataset.id, onDrop: async ([ids]) => { try { await send(`/me/binders/${bid}/order`, { ids: ids.map(Number) }); A.toast('Order saved ✓', { kind: 'ok' }); } catch (err) { A.toast(err.message, { kind: 'err' }); } } });
    });
    const picks = new Set();
    const syncMove = () => { $('moveCount').textContent = `${picks.size} selected`; $('moveBar').hidden = picks.size === 0; document.body.classList.toggle('has-sel', picks.size > 0); };
    bRoot.addEventListener('change', (e) => { const p = e.target.closest('[data-pick]'); if (!p) return; p.checked ? picks.add(+p.dataset.pick) : picks.delete(+p.dataset.pick); syncMove(); });
    $('moveClear').onclick = () => { picks.clear(); bRoot.querySelectorAll('[data-pick]').forEach((p) => p.checked = false); syncMove(); };
    $('moveGo').onclick = async () => {
      const v = $('moveTo').value;
      try { const j = await send('/me/listings/move', { ids: [...picks], binder_id: v ? Number(v) : null }); A.toast(`Moved ${j.n} ✓`, { kind: 'ok' }); location.reload(); } catch (err) { A.toast(err.message, { kind: 'err' }); }
    };
    // Delete the picked cards for good, the same way My listings does: they go at once, with a few seconds to Undo.
    $('moveDel').onclick = () => {
      const ids = [...picks], tiles = ids.map((id) => bRoot.querySelector(`.me-c[data-id="${id}"]`)).filter(Boolean), n = ids.length;
      if (!n) return;
      A.undoable({
        label: `Deleted ${n} card${n === 1 ? '' : 's'}`,
        hide: () => tiles.forEach((t) => { t.hidden = true; }),
        show: () => tiles.forEach((t) => { t.hidden = false; }),
        commit: async () => { await send('/me/listings/bulk', { ids, action: 'delete' }); tiles.forEach((t) => t.remove()); },
      });
      $('moveClear').onclick();
    };
    const loose = bRoot.querySelector('.st-loose .st-sort'); if (loose) $('looseN').textContent = loose.children.length ? `${loose.children.length}` : 'none';
  }

  /* ================= orders ================= */
  const oRoot = $('storeOrders');
  if (oRoot) {
    oRoot.addEventListener('click', async (e) => {
      // One order of several cards: Accept all, Mark all paid, Decline all, each card on its own route, in turn.
      const all = e.target.closest('[data-batch-res]');
      if (all) {
        const group = all.closest('.st-group'), status = all.dataset.batchRes;
        const want = status === 'Paid' ? ['Confirmed'] : status === 'Confirmed' ? ['Pending'] : ['Pending', 'Confirmed'];
        const rows = [...group.querySelectorAll('.st-order[data-kind="reservation"]')].filter((r) => want.includes(r.dataset.status));
        if (!rows.length) return;
        const n = rows.length === 1 ? 'this card' : `these ${rows.length} cards`;
        if (status === 'Declined' && !(await (window.bbvConfirm ? window.bbvConfirm(`Decline ${n}? The buyer is told.`, { yes: 'Decline', no: 'Keep them' }) : Promise.resolve(confirm(`Decline ${n}? The buyer is told.`))))) return;
        const via = group.querySelector('.st-group-h [data-via]')?.value || '';
        all.disabled = true;
        let done = 0;
        for (const r of rows) {
          try { await send(`/me/orders/reservations/${r.dataset.id}`, { status, paid_via: via }); done += 1; }
          catch (err) { A.toast(err.message, { kind: 'err' }); break; }
        }
        if (done) { A.toast(status === 'Paid' ? 'Marked paid ✓ The sale is done.' : 'Saved ✓', { kind: 'ok' }); location.reload(); } else all.disabled = false;
        return;
      }
      const li = e.target.closest('.st-order'); if (!li) return;
      const res = e.target.closest('[data-res]');
      if (res) {
        const status = res.dataset.res;
        if (status === 'Declined' && !(await (window.bbvConfirm ? window.bbvConfirm('Decline this order? The buyer is told.', { yes: 'Decline', no: 'Keep it' }) : Promise.resolve(confirm('Decline this order? The buyer is told.'))))) return;
        if (status === 'Cancelled' && !(await (window.bbvConfirm ? window.bbvConfirm('Cancel this order?', { yes: 'Cancel it', no: 'Keep it' }) : Promise.resolve(confirm('Cancel this order?'))))) return;
        const via = li.querySelector('[data-via]')?.value || '';
        res.disabled = true;
        try { await send(`/me/orders/reservations/${li.dataset.id}`, { status, paid_via: via }); A.toast(status === 'Paid' ? 'Marked paid ✓ The sale is done.' : 'Saved ✓', { kind: 'ok' }); location.reload(); }
        catch (err) { res.disabled = false; A.toast(err.message, { kind: 'err' }); }
      }
      const off = e.target.closest('[data-offer]');
      if (off) {
        const status = off.dataset.offer; const counter = Number(li.querySelector('[data-counter]')?.value) || 0;
        if (status === 'Countered' && !counter) { A.toast('Type your counter price first.', { kind: 'err' }); return; }
        off.disabled = true;
        try { await send(`/me/orders/offers/${li.dataset.id}`, { status, counter }); A.toast('Saved ✓', { kind: 'ok' }); location.reload(); }
        catch (err) { off.disabled = false; A.toast(err.message, { kind: 'err' }); }
      }
      const vc = e.target.closest('[data-vcancel]');
      if (vc) {
        if (!(await (window.bbvConfirm ? window.bbvConfirm('Cancel this order?', { yes: 'Cancel order', no: 'Keep it' }) : Promise.resolve(confirm('Cancel this order?'))))) return;
        vc.disabled = true;
        try { await send(`/me/orders/vault/${li.dataset.id}`, { status: 'Cancelled' }); location.reload(); } catch (err) { vc.disabled = false; A.toast(err.message, { kind: 'err' }); }
        return;
      }
      const ca = e.target.closest('[data-counter-answer]');
      if (ca) {
        ca.disabled = true;
        try { await send(`/me/orders/offers/${li.dataset.id}`, { status: ca.dataset.counterAnswer }); location.reload(); } catch (err) { ca.disabled = false; A.toast(err.message, { kind: 'err' }); }
      }
    });
    oRoot.addEventListener('submit', async (e) => {
      const f = e.target;
      if (f.matches('[data-reply]')) {
        e.preventDefault(); const li = f.closest('.st-order');
        try { await send(`/me/orders/questions/${li.dataset.id}/reply`, { text: f.querySelector('textarea').value }); location.reload(); } catch (err) { A.toast(err.message, { kind: 'err' }); }
      } else if (f.matches('[data-review]')) {
        e.preventDefault();
        const rating = Number(f.querySelector('input[type=radio]:checked')?.value) || 0;
        if (!rating) { A.toast('Pick how many stars.', { kind: 'err' }); return; }
        try { await send('/me/reviews', { transaction_id: Number(f.dataset.review), rating, text: f.querySelector('input[maxlength]').value }); A.toast('Thanks for the review ✓', { kind: 'ok' }); location.reload(); }
        catch (err) { A.toast(err.message, { kind: 'err' }); }
      }
    });
    // Stars light up to the one picked.
    oRoot.querySelectorAll('.st-stars').forEach((s) => s.addEventListener('change', () => { const v = Number(s.querySelector(':checked')?.value) || 0; [...s.querySelectorAll('label')].forEach((l, i) => l.classList.toggle('on', i < v)); }));
  }

  /* ================= settings ================= */
  const sRoot = $('storeSettings');
  if (sRoot) {
    $('slugForm').addEventListener('submit', async (e) => {
      e.preventDefault(); const f = e.target;
      try { const j = await send('/me/settings/slug', { slug: f.slug.value.trim().toLowerCase() }); $('slugNow').textContent = j.slug; f.slug.value = j.slug; sRoot.querySelectorAll('a[href^="/store/"], a[href^="/trainer/"]').forEach((a) => a.href = j.url); A.toast('Address changed ✓', { kind: 'ok' }); }
      catch (err) { A.toast(err.message, { kind: 'err' }); }
    });
    $('coverFile').addEventListener('change', () => { const f = $('coverFile').files[0]; if (!f) return; $('coverImg').src = URL.createObjectURL(f); $('coverPrev').hidden = false; });
    $('storeForm').addEventListener('submit', async (e) => {
      e.preventDefault(); const f = e.target, btn = f.querySelector('button[type=submit]'); btn.disabled = true;
      try {
        const fd = new FormData(f);
        const cover = $('coverFile').files[0];
        if (cover) fd.set('cover', await shrink(cover, 1600), 'cover.jpg'); else fd.delete('cover');
        const j = await send('/me/settings', fd);
        if (j.cover) { $('coverImg').src = j.cover; $('coverPrev').hidden = false; } else { $('coverPrev').hidden = true; }
        f.drop_cover.checked = false; $('coverFile').value = '';
        A.toast('Saved ✓', { kind: 'ok' });
      } catch (err) { A.toast(err.message, { kind: 'err' }); }
      btn.disabled = false;
    });
  }

  /* ================= notifications ================= */
  const nRoot = $('storeNotifs');
  if (nRoot) {
    $('readAll').onclick = async () => { try { await send('/api/notifications/read', {}); nRoot.querySelectorAll('.st-note.unread').forEach((n) => n.classList.remove('unread')); A.toast('All read ✓', { kind: 'ok' }); } catch (err) { A.toast(err.message, { kind: 'err' }); } };
    // One choice per kind: Phone and bell, Bell only, or Off. It saves at once.
    const how = (row) => row.querySelector('[data-how][aria-pressed="true"]')?.dataset.how || 'bell';
    const savePref = (row) => {
      const body = { event: row.dataset.event, how: how(row) }, mail = row.querySelector('[data-ch="email"]');
      if (mail) body.email = mail.checked;
      return send('/me/notifications/prefs', body);
    };
    nRoot.addEventListener('click', async (e) => {
      const b = e.target.closest('[data-how]'); if (!b) return;
      const row = b.closest('[data-event]'), before = how(row);
      if (b.dataset.how === before) return;
      const paint = (k) => row.querySelectorAll('[data-how]').forEach((x) => x.setAttribute('aria-pressed', String(x.dataset.how === k)));
      paint(b.dataset.how);
      try { await savePref(row); A.toast('Saved ✓', { kind: 'ok' }); } catch (err) { paint(before); A.toast(err.message, { kind: 'err' }); }
    });
    nRoot.addEventListener('change', async (e) => {
      const box = e.target.closest('[data-ch="email"]'); if (!box) return;
      try { await savePref(box.closest('[data-event]')); A.toast('Saved ✓', { kind: 'ok' }); } catch (err) { box.checked = !box.checked; A.toast(err.message, { kind: 'err' }); }
    });
    // Buyers keep their email here (sellers have it in Store settings). Saving it switches on the email boxes.
    $('emailForm')?.addEventListener('submit', async (e) => {
      e.preventDefault(); const f = e.currentTarget, btn = f.querySelector('button'); btn.disabled = true;
      try {
        const j = await send('/me/notifications/prefs', { address: f.address.value.trim() });
        f.address.value = j.email || '';
        nRoot.querySelectorAll('[data-ch="email"]').forEach((b) => { b.disabled = !j.email; if (!j.email) b.checked = false; });
        A.toast(j.email ? 'Email saved ✓' : 'Email removed ✓', { kind: 'ok' });
      } catch (err) { A.toast(err.message, { kind: 'err' }); }
      btn.disabled = false;
    });
  }
})();
