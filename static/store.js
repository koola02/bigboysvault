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
    if (r.status === 401) { location.href = '/join?signin=1'; throw new Error('Please sign in again.'); }
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
    const tileOf = (n) => n.closest('.st-tile');
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
    const paintAll = () => rows.querySelectorAll('.st-tile').forEach(paintSug);
    document.addEventListener('bbv:shares', paintAll);

    function apply(tile, card) {
      Object.assign(tile.dataset, { status: card.status, market: card.market, own: card.value_own, cond: card.cond, qty: card.qty });
      const st = tile.querySelector('.st-status'); st.textContent = card.status; st.dataset.s = card.status;
      tile.querySelector('[data-status]').value = card.status;
      tile.querySelector('[data-qty-in]').value = card.qty;
      const pr = tile.querySelector('[data-price]'); pr.value = card.value_own > 0 ? String(Math.round(card.value_own)) : ''; pr.placeholder = card.market > 0 ? card.price_text : 'Price';
      const renew = tile.querySelector('[data-renew]'); renew.textContent = card.status === 'Expired' ? 'Renew for 30 days' : card.expires_at > 0 ? 'Change end date' : 'Set an end date';
      const exp = tile.querySelector('.st-exp');
      if (card.expires_text) { if (exp) exp.textContent = card.expires_text; else { const m = tile.querySelector('.st-meta'); m.append(' · '); const s = el('span', 'st-exp', card.expires_text); m.append(s); } }
      else exp?.remove();
      paintSug(tile);
    }
    async function patch(tile, body, node) {
      try { const j = await send(`/me/listings/${tile.dataset.id}`, body); apply(tile, j.card); if (node) flash(node, true); A.toast('Saved ✓', { kind: 'ok' }); return j.card; }
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
      else if (t.matches('[data-qty-in]')) patch(tile, { qty: Math.max(0, Math.round(Number(t.value) || 0)) }, t);
      else if (t.matches('[data-status]')) patch(tile, { status: t.value }, t);
      else if (t.matches('[data-binder]')) patch(tile, { binder_id: t.value ? Number(t.value) : null }, t);
      else if (t.matches('[data-notes]')) patch(tile, { notes: t.value }, t);
    });
    rows.addEventListener('keydown', (e) => { if (e.key === 'Enter' && e.target.matches('[data-price], [data-qty-in]')) { e.preventDefault(); e.target.blur(); } });
    rows.addEventListener('click', async (e) => {
      const tile = tileOf(e.target); if (!tile) return;
      const q = e.target.closest('[data-qty]');
      if (q) { const inp = tile.querySelector('[data-qty-in]'); const v = Math.max(0, (Number(inp.value) || 0) + Number(q.dataset.qty)); inp.value = v; patch(tile, { qty: v }, inp); return; }
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
      boxes().forEach((b) => { b.checked = sel.has(+b.dataset.sel); b.closest('.st-tile').classList.toggle('sel', b.checked); });
      $('selCount').textContent = `${sel.size} selected`; $('selbar').hidden = sel.size === 0; document.body.classList.toggle('has-sel', sel.size > 0);
      const all = $('selAll'); if (all) { const vis = boxes(); all.checked = vis.length > 0 && vis.every((b) => b.checked); all.indeterminate = !all.checked && vis.some((b) => b.checked); }
    }
    const clear = () => { sel.clear(); sync(); };
    rows.addEventListener('change', (e) => { const b = e.target.closest('[data-sel]'); if (!b) return; b.checked ? sel.add(+b.dataset.sel) : sel.delete(+b.dataset.sel); sync(); });
    $('selAll')?.addEventListener('change', (e) => { boxes().forEach((b) => (e.target.checked ? sel.add(+b.dataset.sel) : sel.delete(+b.dataset.sel))); sync(); });
    const tilesSel = () => [...sel].map((id) => rows.querySelector(`.st-tile[data-id="${id}"]`)).filter(Boolean);
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
    const CONDS = [...document.querySelector('.st-tile [data-cond]')?.options || []].map((o) => o.value).filter((v) => v && v !== '__own');
    const STATUSES = [...document.querySelector('.st-tile [data-status]')?.options || []].map((o) => o.value);
    const BINDERS = [...document.querySelector('.st-tile [data-binder]')?.options || []].map((o) => [o.value, o.textContent]);
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
        node.append(el('p', 'hint show', 'Hidden cards stay in your store but nobody else sees them. Sold cards stop being for sale.'), chips(STATUSES, inp));
        A.sheet.open(`Status · ${plural(sel.size)}`, node, async () => { if (!inp.value) throw new Error('Pick a status.'); await bulk({ action: 'edit', status: inp.value }, (j) => `Updated ${plural(j.n)}`); }, 'Apply');
      },
      binder() {
        const node = el('div', 'sheet-form'); const s = el('select', 'sheet-input'); for (const [v, t] of BINDERS) s.append(new Option(t, v));
        node.append(el('p', 'hint show', 'Moves every selected card into this binder.'), s);
        A.sheet.open(`Binder · ${plural(sel.size)}`, node, async () => { await bulk({ action: 'edit', binder_id: s.value ? Number(s.value) : null }, (j) => `Moved ${plural(j.n)}`); }, 'Move');
      },
      stock() {
        const node = el('div', 'sheet-form'); const inp = el('input', 'sheet-input'); inp.type = 'number'; inp.min = '0'; inp.inputMode = 'numeric'; inp.placeholder = 'Copies in stock';
        node.append(el('p', 'hint show', 'Every selected card gets this many copies in stock. 0 means sold out.'), inp);
        A.sheet.open(`Stock · ${plural(sel.size)}`, node, async () => { const n = Math.round(Number(inp.value)); if (!Number.isFinite(n) || n < 0) throw new Error('Type how many.'); await bulk({ action: 'edit', qty: n }, (j) => `Updated ${plural(j.n)}`); }, 'Apply');
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
        else { const t = tiles[0]; const j = await send(`/me/listings/${t.dataset.id}/renew`, { days }); apply(t, j.card); A.sheet.close(); A.toast(days ? `On sale for ${days} more days ✓` : 'No end date ✓', { kind: 'ok' }); }
      }, 'Apply');
    }
    function count() { const n = rows.querySelectorAll('.st-tile').length; $('shownN').textContent = n; const empty = $('listEmpty'); if (empty && n > 0) empty.hidden = true; }
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

    /* ----- adding cards ----- */
    const panel = $('addPanel');
    const openAdd = (show) => { panel.hidden = !show; if (show) { $('addQ').focus(); panel.scrollIntoView({ block: 'start', behavior: 'smooth' }); } };
    $('addOpen').onclick = () => openAdd(panel.hidden);
    $('addClose').onclick = () => openAdd(false);
    panel.querySelectorAll('[data-add]').forEach((b) => b.onclick = () => {
      panel.querySelectorAll('[data-add]').forEach((x) => { const on = x === b; x.classList.toggle('on', on); x.setAttribute('aria-selected', String(on)); });
      panel.querySelectorAll('.st-add-pane').forEach((p) => p.hidden = p.dataset.pane !== b.dataset.add);
    });
    if (Number(root.dataset.total) === 0) openAdd(true);

    let lang = 'en', timer, seq = 0, added = 0;
    const q = $('addQ'), results = $('addResults'), state = $('addState');
    $('addLang').addEventListener('click', (e) => { const b = e.target.closest('button'); if (!b) return; lang = b.dataset.lang; $('addLang').querySelectorAll('button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); search(); });
    q.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(search, 320); });
    q.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); clearTimeout(timer); search(); q.blur(); } });
    async function search(text = q.value.trim(), into = results, say = state) {
      const mine = ++seq;
      if (text.length < 2) { into.replaceChildren(); say.textContent = 'Type a name and tap the card you’re selling.'; return; }
      say.textContent = 'Searching…';
      try {
        const r = await fetch(`/me/search?q=${encodeURIComponent(text)}&lang=${lang}`, { credentials: 'same-origin', headers: { Accept: 'application/json' } });
        const j = await r.json(); if (mine !== seq) return;
        const cards = j.cards || [];
        say.textContent = j.error && !cards.length ? j.error : cards.length ? `${cards.length} found. Tap the one you’re selling.` : `No cards called “${text}”. Check the spelling.`;
        into.replaceChildren(...cards.slice(0, 120).map(result));
      } catch (_) { if (mine === seq) say.textContent = 'Couldn’t search just now. Check your connection.'; }
    }
    function result(c) {
      const b = el('button', 'me-res'); b.type = 'button'; b.dataset.id = c.id;
      const pic = el('span', 'me-res-pic');
      if (c.image) { const i = el('img'); i.src = c.image; i.alt = ''; i.loading = 'lazy'; i.width = 245; i.height = 342; i.onerror = () => i.remove(); pic.append(i); }
      b.append(pic, el('b', null, c.name), el('small', null, [c.en, c.set, c.number].filter(Boolean).join(' · ')));
      b.addEventListener('click', async () => {
        if (b.disabled) return; b.classList.add('busy'); b.disabled = true;
        try {
          const j = await send('/me/cards', { id: c.id, image: c.image, image_large: c.image_large, en: c.en });
          added++; b.classList.add('in'); b.querySelector('b').textContent = `${c.name} ✓ added`;
          A.toast(`Added ${c.name} at ${j.card.value || 'no price yet'} ✓`, { kind: 'ok' });
          root.dataset.total = Number(root.dataset.total) + 1;
        } catch (err) { A.toast(err.message, { kind: 'err' }); }
        b.classList.remove('busy'); b.disabled = false;
      });
      return b;
    }
    // Leaving the add panel with new cards: show them.
    $('addClose').addEventListener('click', () => { if (added) location.reload(); });
    window.addEventListener('pagehide', () => {});

    /* ----- scan: a photo, read on the device when the browser can, then the card list ----- */
    const scanFile = $('scanFile');
    if (scanFile) scanFile.addEventListener('change', async () => {
      const f = scanFile.files[0]; if (!f) return;
      const shot = $('scanShot'); shot.src = URL.createObjectURL(f); shot.hidden = false; $('scanHint').hidden = true;
      const say = $('scanState'), into = $('scanResults');
      say.textContent = 'Reading the card…';
      let guess = '';
      try {
        if ('LanguageModel' in self) {
          const opts = { expectedInputs: [{ type: 'image' }, { type: 'text', languages: ['en'] }], expectedOutputs: [{ type: 'text', languages: ['en'] }] };
          if ((await LanguageModel.availability(opts)) !== 'unavailable') {
            const session = await LanguageModel.create(opts);
            const bmp = await createImageBitmap(await shrink(f, 800));
            const out = await session.prompt([{ role: 'user', content: [{ type: 'text', value: 'This is a Pokémon trading card. Reply with only the Pokémon or card name printed at the top, then a space, then the card number printed at the bottom (like 58/102). Nothing else.' }, { type: 'image', value: bmp }] }]);
            guess = String(out || '').split('\n')[0].replace(/[^\w\s/'-]/g, ' ').trim().slice(0, 60);
            session.destroy?.();
          }
        }
      } catch (_) {}
      if (!guess) {
        say.textContent = 'This browser can’t read cards yet. Type the name under the photo and tap the right card.';
        const inp = el('input', 'sheet-input'); inp.placeholder = 'Card name and number, like Pikachu 58'; inp.autocomplete = 'off';
        let t; inp.addEventListener('input', () => { clearTimeout(t); t = setTimeout(() => search(inp.value.trim(), into, say), 320); });
        say.after(inp); inp.focus();
        return;
      }
      say.textContent = `Looks like “${guess}”. Tap the right card, or search again.`;
      q.value = guess; await search(guess, into, say);
    });

    /* ----- a card from your own photo ----- */
    const own = $('ownForm');
    if (own) {
      const file = $('ownPhoto');
      file.addEventListener('change', () => { const f = file.files[0]; if (!f) return; $('ownShot').src = URL.createObjectURL(f); $('ownShot').hidden = false; $('ownHint').hidden = true; });
      own.addEventListener('submit', async (e) => {
        e.preventDefault();
        const f = file.files[0]; if (!f) { A.toast('Add a photo of the card first.', { kind: 'err' }); return; }
        const btn = own.querySelector('button[type=submit]'); btn.disabled = true; btn.textContent = 'Adding…';
        try {
          const fd = new FormData(own); fd.set('photo', await shrink(f, 1600), 'card.jpg');
          const j = await send('/me/cards/upload', fd);
          if (fd.get('flag')) await send(`/me/listings/${j.card.id}`, { flag: true });
          added++; A.toast('Added ✓', { kind: 'ok' }); own.reset(); $('ownShot').hidden = true; $('ownHint').hidden = false;
        } catch (err) { A.toast(err.message, { kind: 'err' }); }
        btn.disabled = false; btn.textContent = 'Add to my store';
      });
    }

    paintAll(); sync();
    const focus = root.dataset.focus;
    if (focus && focus !== '0') { const t = rows.querySelector(`.st-tile[data-id="${focus}"]`); if (t) { t.classList.add('focus'); t.scrollIntoView({ block: 'center' }); } }
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
        if (!confirm('Delete this binder? The cards stay in your store.')) return;
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
    const loose = bRoot.querySelector('.st-loose .st-sort'); if (loose) $('looseN').textContent = loose.children.length ? `${loose.children.length}` : 'none';
  }

  /* ================= orders ================= */
  const oRoot = $('storeOrders');
  if (oRoot) {
    oRoot.addEventListener('click', async (e) => {
      const li = e.target.closest('.st-order'); if (!li) return;
      const res = e.target.closest('[data-res]');
      if (res) {
        const status = res.dataset.res;
        if (status === 'Declined' && !confirm('Decline this reservation?')) return;
        if (status === 'Cancelled' && !confirm('Cancel your reservation?')) return;
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
    nRoot.addEventListener('change', async (e) => {
      const tr = e.target.closest('tr[data-event]'); if (!tr) return;
      const get = (k) => tr.querySelector(`[data-ch="${k}"]`).checked;
      try { await send('/me/notifications/prefs', { event: tr.dataset.event, site: get('site'), email: get('email'), push: get('push') }); A.toast('Saved ✓', { kind: 'ok' }); }
      catch (err) { e.target.checked = !e.target.checked; A.toast(err.message, { kind: 'err' }); }
    });
    $('pushOn')?.addEventListener('click', async () => {
      const key = nRoot.dataset.pushKey; if (!key || !('serviceWorker' in navigator) || !('PushManager' in window)) { A.toast('This browser can’t do push notifications.', { kind: 'err' }); return; }
      try {
        const reg = await navigator.serviceWorker.register('/static/push-sw.js');
        const raw = atob(key.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(key.length / 4) * 4, '='));
        const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: Uint8Array.from(raw, (c) => c.charCodeAt(0)) });
        const j = await send('/api/notifications/push', sub.toJSON());
        A.toast(j.enabled ? 'Push notifications on ✓' : 'Saved. Push sending isn’t switched on for the site yet.', { kind: 'ok' });
      } catch (err) { A.toast(err.message || 'Couldn’t turn on push.', { kind: 'err' }); }
    });
  }
})();
