/* Binder tools (new look). Whoever owns binders arranges them where they appear, the way an album is edited where you
   see it: a seller on their own store, a full admin on the vault's store (/store/vault). The same tools for both.
   - A store's Binders: "Edit binders" turns the tiles into an editor. Drag a binder to move it (a mouse drags straight
     away; a finger presses and holds first; the arrow keys move the binder whose handle has focus). "+ New binder", and
     on every binder Rename, Cover and Delete. Tapping a binder's picture opens it, ready to add cards.
   - A binder's own page: "Edit binder" lists its cards with Cover and Remove (Remove can be undone), drag to put them
     in order, and "Add cards": a picker of the owner's cards (the vault's cards; a seller's cards on sale, where a card
     in another binder moves here, and the picker says so).
   - Store Manager › Binders: each binder has a handle to drag it into place.
   - The Stores page: "Choose what shows" on the owner's own store picks which binders show there and which cards are
     highlighted (a full admin does the same for the vault's store).
   Everything saves by itself and says so in a short message. The server decides who may change what. */
(() => {
  if (window.bbvBinderTools) return;
  window.bbvBinderTools = true;

  const enc = encodeURIComponent;
  const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const plural = (n, one, many = `${one}s`) => `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`;
  const clean = (s) => String(s || '').replace(/\s+/g, ' ').trim();
  const coarse = matchMedia('(pointer: coarse)').matches;

  /* ---------- small pictures ---------- */
  const ICONS = {
    pencil: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>',
    image: '<rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="8.5" cy="10" r="1.5"/><path d="M21 16l-5-5-9 8"/>',
    trash: '<path d="M4 7h16"/><path d="M10 11v6M14 11v6"/><path d="M6 7l1 13h10l1-13"/><path d="M9 7V4h6v3"/>',
    grip: '<circle cx="9" cy="6" r="1.4"/><circle cx="15" cy="6" r="1.4"/><circle cx="9" cy="12" r="1.4"/><circle cx="15" cy="12" r="1.4"/><circle cx="9" cy="18" r="1.4"/><circle cx="15" cy="18" r="1.4"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    x: '<path d="M6 6l12 12M18 6L6 18"/>',
    star: '<path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"/>',
    out: '<circle cx="12" cy="12" r="8.5"/><path d="M8.5 12h7"/>',
    check: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
    book: '<path d="M5 4h11a3 3 0 0 1 3 3v13H8a3 3 0 0 1-3-3z"/><path d="M5 17a3 3 0 0 1 3-3h11"/><path d="M9 4v10"/>',
  };
  function icon(name) {
    const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    s.setAttribute('viewBox', '0 0 24 24'); s.setAttribute('aria-hidden', 'true'); s.classList.add('bt-ic', `bt-ic-${name}`);
    s.innerHTML = ICONS[name];
    return s;
  }
  function button(cls, label, iconName, words) {
    const b = el('button', cls); b.type = 'button';
    if (iconName) b.append(icon(iconName));
    if (words) b.append(el('span', '', words));
    if (label) { b.setAttribute('aria-label', label); b.title = label; }
    return b;
  }

  /* ---------- talking to the site ---------- */
  async function req(url, body) {
    const opts = { credentials: 'same-origin', headers: { Accept: 'application/json' } };
    if (body !== undefined) {
      opts.method = 'POST';
      if (body instanceof FormData) opts.body = body;
      else { opts.headers['Content-Type'] = 'application/json'; opts.body = JSON.stringify(body); }
    }
    let r;
    try { r = await fetch(url, opts); } catch (_) { throw new Error('No connection. Nothing changed; please try again.'); }
    const j = await r.json().catch(() => ({}));
    if (r.status === 401) throw new Error(j.error || 'Please sign in again, then try once more.');
    if (!r.ok || j.ok === false) throw new Error(j.error || 'That didn’t save. Please try again.');
    return j;
  }

  // The same tools, two owners: the vault's binders (full admins) and a seller's own.
  const API = {
    vault: {
      list: () => req('/admin/binders'),
      create: (title) => req('/admin/binders', { title }),
      order: (keys) => req('/admin/binders/order', { handles: keys }),
      rename: (k, title) => req(`/admin/binders/${enc(k)}`, { title }),
      remove: (k) => req(`/admin/binders/${enc(k)}`, { delete: true }),
      cover: (k, id) => req(`/admin/binders/${enc(k)}`, { cover: id == null ? 'auto' : String(id) }),
      upload: (k, file) => { const fd = new FormData(); fd.append('photo', file, 'cover.jpg'); return req(`/admin/binders/${enc(k)}/cover`, fd); },
      cards: (k) => req(`/admin/binders/${enc(k)}/cards`),
      choices: (k, q) => req(`/admin/binders/${enc(k)}/choices?q=${enc(q)}`),
      change: (k, body) => req(`/admin/binders/${enc(k)}/cards`, body),
      arrange: (k, ids) => req(`/admin/binders/${enc(k)}/arrange`, { ids }),
      nameMax: 40,
      auto: 'Automatic: the dearest card',
      stays: 'It stays in the vault.',
      keeps: (n) => (n ? `The ${plural(n, 'card')} in it stay in the vault, and in any other binder they’re in. Only the binder goes away.` : 'It has no cards. Only the binder goes away.'),
      find: 'Search the vault’s cards: name, number or tag',
      none: 'Every card in the vault is already in this binder.',
    },
    seller: {
      list: () => req('/me/binders'),
      create: (title) => req('/me/binders', { title }),
      order: (keys) => req('/me/binders/order', { ids: keys.map(Number) }),
      rename: (k, title) => req(`/me/binders/${enc(k)}/edit`, { title }),
      remove: (k) => req(`/me/binders/${enc(k)}/delete`, {}),
      cover: (k, id) => req(`/me/binders/${enc(k)}/cover`, { card_id: id == null ? null : Number(id) }),
      upload: null,
      cards: (k) => req(`/me/binders/${enc(k)}/cards`),
      choices: (k, q) => req(`/me/binders/${enc(k)}/choices?q=${enc(q)}`),
      change: (k, body) => req(`/me/binders/${enc(k)}/cards`, body),
      arrange: (k, ids) => req(`/me/binders/${enc(k)}/order`, { ids }),
      nameMax: 60,
      auto: 'Automatic: its first card',
      stays: 'It stays in your store.',
      keeps: (n) => (n ? `The ${plural(n, 'card')} in it stay in your store, as cards not in a binder. Only the binder goes away.` : 'It has no cards. Only the binder goes away.'),
      find: 'Search your cards: name, number or set',
      none: 'Every card you have on sale is already in this binder.',
    },
  };

  // Drag and drop is the Manager's own (static/dragsort.js), fetched the first time an editor opens.
  let sortReady = null;
  function needSort() {
    if (window.dragSort) return Promise.resolve();
    sortReady ||= new Promise((ok, no) => {
      const s = document.createElement('script'); s.src = '/static/dragsort.js?v=5';
      s.onload = () => ok(); s.onerror = () => { sortReady = null; no(new Error('Couldn’t load the editor. Check your connection.')); };
      document.head.append(s);
    });
    return sortReady;
  }

  // While a binder or card is dragged, the bars stuck to the top (the site's header, a binder's own bar) cover the page
  // under them, and dragsort.js scrolls only within 80px of the screen's edge. Held just below the bars, the page
  // scrolls up as well, and the gap where it will land follows.
  let held = null, rolling = false;
  addEventListener('pointermove', (e) => {
    if (!document.body.classList.contains('is-dragging')) { held = null; return; }
    held = { x: e.clientX, y: e.clientY, id: e.pointerId, type: e.pointerType };
    if (!rolling) { rolling = true; requestAnimationFrame(roll); }
  }, { passive: true });
  function roll() {
    if (!held || !document.body.classList.contains('is-dragging')) { rolling = false; return; }
    const bar = document.querySelector('.sf-bbar');
    const top = Math.max(bar ? bar.getBoundingClientRect().bottom : 0, parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--hd-h')) || 56);
    if (held.y >= 80 && held.y < top + 32 && scrollY > 0) {
      scrollBy(0, -10);
      window.dispatchEvent(new PointerEvent('pointermove', { clientX: held.x, clientY: held.y, pointerId: held.id, pointerType: held.type, bubbles: true }));
    }
    requestAnimationFrame(roll);
  }

  /* ---------- a short message at the bottom, now and then with Undo ---------- */
  let toastBox = null, toastTimer = 0;
  function say(msg, { bad = false, undo = null } = {}) {
    if (!toastBox) {
      toastBox = el('div', 'bt-toast'); toastBox.setAttribute('role', 'status'); toastBox.setAttribute('aria-live', 'polite');
      document.body.append(toastBox);
    }
    clearTimeout(toastTimer);
    const t = el('span', '', msg); toastBox.replaceChildren(t);
    toastBox.classList.toggle('bad', bad);
    if (undo) {
      const u = button('bt-undo', null, null, 'Undo');
      u.addEventListener('click', async () => { u.disabled = true; toastBox.classList.remove('on'); try { await undo(); } catch (e) { say(e.message, { bad: true }); } });
      toastBox.append(u);
    }
    toastBox.classList.add('on');
    toastTimer = setTimeout(() => toastBox.classList.remove('on'), undo ? 6500 : 3400);
  }

  /* ---------- a window over the page: in the middle on a computer, from the bottom on a phone ---------- */
  let uid = 0;
  function dialog(title, { wide = false } = {}) {
    const d = el('dialog', `bt-dlg${wide ? ' bt-wide' : ''}`);
    const head = el('header', 'bt-dlg-h');
    const h = el('h2', '', title); h.id = `btDlg${++uid}`; d.setAttribute('aria-labelledby', h.id);
    const x = button('bt-x', 'Close', 'x');
    head.append(h, x);
    const body = el('div', 'bt-dlg-b'), foot = el('footer', 'bt-dlg-f');
    d.append(head, body, foot);
    const back = document.activeElement;
    const close = () => { if (d.open) d.close(); };
    d.addEventListener('close', () => {
      d.remove(); document.documentElement.classList.remove('bt-modal');
      if (back && back.isConnected) back.focus({ preventScroll: true });
    });
    x.addEventListener('click', close);
    d.addEventListener('mousedown', (e) => { if (e.target === d) close(); });   // the dimmed page around it
    document.body.append(d);
    d.showModal(); document.documentElement.classList.add('bt-modal');
    return { d, body, foot, close };
  }
  function ask(title, text, yes) {
    return new Promise((done) => {
      const g = dialog(title); g.d.classList.add('bt-ask');
      g.body.append(el('p', 'bt-ask-t', text));
      const no = button('btn ghost', null, null, 'Cancel'), ok = button('btn bt-danger', null, null, yes);
      let answer = false;
      no.addEventListener('click', g.close);
      ok.addEventListener('click', () => { answer = true; g.close(); });
      g.d.addEventListener('close', () => done(answer));
      g.foot.append(no, ok);
      no.focus();
    });
  }
  // Phone photos are big: made smaller here first, so the upload is quick on mobile data.
  async function shrink(file, max = 1600) {
    try {
      const bmp = await createImageBitmap(file);
      const k = Math.min(1, max / Math.max(bmp.width, bmp.height));
      if (k === 1 && file.size < 2500000) return file;
      const c = document.createElement('canvas'); c.width = Math.round(bmp.width * k); c.height = Math.round(bmp.height * k);
      c.getContext('2d').drawImage(bmp, 0, 0, c.width, c.height);
      return await new Promise((ok) => c.toBlob((b) => ok(b || file), 'image/jpeg', 0.88));
    } catch (_) { return file; }
  }

  // The arrow keys (and Home, End) on a handle move its item. Returns the new order, or null when nothing moved.
  function keyMove(e, items, item) {
    const i = items.indexOf(item);
    const j = { ArrowLeft: i - 1, ArrowUp: i - 1, ArrowRight: i + 1, ArrowDown: i + 1, Home: 0, End: items.length - 1 }[e.key];
    if (j === undefined) return null;
    e.preventDefault();
    if (i < 0 || j < 0 || j >= items.length || j === i) return null;
    const order = [...items]; order.splice(i, 1); order.splice(j, 0, item);
    return { order, at: j };
  }

  /* ---------- the cover: one of the binder's cards (the vault can also upload a photo) ---------- */
  function coverWindow(api, key, title, done) {
    const g = dialog(`Cover for “${title}”`); g.d.classList.add('bt-fit');
    g.body.append(el('p', 'bt-how', 'Tap a card: it shows first on the binder’s tile.'));
    const opts = el('div', 'bt-cover-opts');
    const auto = button('btn ghost sm', null, null, api.auto);
    auto.addEventListener('click', () => pick(null));
    opts.append(auto);
    if (api.upload) {
      const up = el('label', 'btn ghost sm bt-up'); up.append(icon('image'), el('span', '', 'Upload a photo'));
      const f = el('input', 'sr'); f.type = 'file'; f.accept = 'image/*'; f.setAttribute('aria-label', 'Upload a photo for the cover');
      up.append(f);
      f.addEventListener('change', async () => {
        const file = f.files[0]; if (!file) return;
        state.textContent = 'Uploading…';
        try { await api.upload(key, await shrink(file)); g.close(); say('Photo uploaded: it’s the cover now ✓'); done(); }
        catch (e) { state.textContent = e.message; }
      });
      opts.append(up);
    }
    g.body.append(opts);
    const state = el('p', 'bt-state', 'Loading the cards in this binder…');
    const list = el('ul', 'bt-covers');
    g.body.append(state, list);
    const close = button('btn ghost', null, null, 'Close'); close.addEventListener('click', g.close); g.foot.append(close);
    async function pick(id) {
      g.close();
      try { await api.cover(key, id); say(id == null ? 'The cover picks itself again ✓' : 'Cover changed ✓'); done(); }
      catch (e) { say(e.message, { bad: true }); }
    }
    api.cards(key).then((j) => {
      const cards = (j.cards || []).filter((c) => c.image);
      state.textContent = cards.length ? '' : `No card in this binder has a picture yet. Add cards first${api.upload ? ', or upload a photo' : ''}.`;
      list.replaceChildren(...cards.map((c) => {
        const li = el('li');
        const b = button('bt-pc', `Make ${c.title} the cover`);
        b.setAttribute('aria-pressed', String(!!c.cover));
        const pic = el('span', 'bt-pc-pic'); const im = el('img'); im.src = c.image; im.alt = ''; im.loading = 'lazy'; pic.append(im);
        if (c.cover) pic.append(el('span', 'bt-flag', 'Cover'));
        b.append(pic, el('span', 'bt-pc-t', c.title));
        b.addEventListener('click', () => pick(c.id));
        li.append(b); return li;
      }));
      list.querySelector('[aria-pressed="true"]')?.scrollIntoView({ block: 'nearest' });
    }).catch((e) => { state.textContent = e.message; });
  }

  /* ================= a store's binders, arranged where they are shown ================= */
  function gridEditor(toggle) {
    const kind = toggle.dataset.btEdit, api = API[kind];
    const head = toggle.closest('.bt-head');
    const grid = head && head.nextElementSibling;
    if (!api || !grid || !grid.matches('.sf-bgrid')) return;
    const none = grid.nextElementSibling && grid.nextElementSibling.matches('.bt-none') ? grid.nextElementSibling : null;
    const words = toggle.querySelector('span');
    let tiles = [], on = false, dirty = false, saved = null, hint = null, sorted = false;

    async function load() { const j = await api.list(); tiles = j.binders || []; }
    async function refresh(focusKey, sel) {
      try { await load(); } catch (e) { say(e.message, { bad: true }); }
      draw();
      if (focusKey != null) grid.querySelector(`.bt-li[data-key="${CSS.escape(String(focusKey))}"] ${sel || '.bt-grip'}`)?.focus({ preventScroll: true });
    }

    async function enter() {
      toggle.disabled = true;
      try { await needSort(); await load(); }
      catch (e) { toggle.disabled = false; say(e.message, { bad: true }); return; }
      toggle.disabled = false;
      on = true; saved = [...grid.childNodes];
      grid.classList.add('bt-on');
      toggle.setAttribute('aria-pressed', 'true'); toggle.classList.remove('ghost'); words.textContent = 'Done';
      hint = el('p', 'bt-hint', coarse ? 'Press and hold a binder, then drag it to move it. Tap its picture to add or remove cards. Everything saves by itself.'
        : 'Drag a binder to move it. Click its picture to add or remove cards. Everything saves by itself.');
      head.after(hint);
      if (none) none.hidden = true;
      draw();
      if (!sorted) {
        sorted = true;
        window.dragSort(grid, { item: '.sf-bgrid.bt-on > li.bt-li', key: (li) => li.dataset.key, onDrop: ([keys], li) => saveOrder(keys, li.dataset.key) });
      }
      if (!tiles.length) startNew(); else grid.querySelector('.bt-grip')?.focus({ preventScroll: true });
    }
    function leave() {
      // What visitors see is drawn by the site: after a change, the page comes back fresh.
      if (dirty) { toggle.disabled = true; location.reload(); return; }
      on = false; grid.classList.remove('bt-on'); grid.replaceChildren(...saved); hint?.remove(); if (none) none.hidden = false;
      toggle.setAttribute('aria-pressed', 'false'); toggle.classList.add('ghost'); words.textContent = 'Edit binders';
      toggle.focus({ preventScroll: true });
    }
    toggle.addEventListener('click', () => (on ? leave() : enter()));

    function draw() {
      grid.replaceChildren(...tiles.map(tile), newTile());
    }
    function tile(t) {
      const li = el('li', 'bt-li'); li.dataset.key = t.key;
      const box = el('div', 'sf-btile bt-tile'); if (t.off) box.dataset.off = '';
      const open = el('a', 'bt-open'); open.href = `${t.href}#edit`; open.draggable = false;
      open.setAttribute('aria-label', `Open ${t.title} to add, remove or arrange its cards`);
      const cov = el('span', `sf-bcov${t.photo ? ' sf-bphoto' : ''}`); cov.dataset.n = String(t.pics.length);
      t.pics.forEach((p) => { const im = el('img'); im.src = p; im.alt = ''; im.draggable = false; im.decoding = 'async'; cov.append(im); });
      if (!t.pics.length) { const n = icon('book'); n.setAttribute('class', 'sf-bnone'); cov.append(n); }
      open.append(cov);
      const grip = button('bt-grip', `Move ${t.title}: drag it, or press the arrow keys`, 'grip');
      const txt = el('span', 'sf-btxt'); txt.append(el('b', 'sf-bname', t.title), el('small', '', t.words));
      const acts = el('div', 'bt-acts'); acts.dataset.nodrag = '';
      const ren = button('bt-act', `Rename ${t.title}`, 'pencil', 'Rename'); ren.dataset.act = 'rename';
      const cv = button('bt-act', `Choose the cover of ${t.title}`, 'image', 'Cover'); cv.dataset.act = 'cover';
      const del = button('bt-act bt-del', `Delete ${t.title}`, 'trash', 'Delete'); del.dataset.act = 'delete';
      ren.addEventListener('click', () => rename(li, t));
      cv.addEventListener('click', () => coverWindow(api, t.key, t.title, () => { dirty = true; refresh(t.key, '[data-act="cover"]'); }));
      del.addEventListener('click', () => remove(t));
      acts.append(ren, cv, del);
      box.append(open, grip, txt, acts);
      li.append(box);
      return li;
    }

    async function saveOrder(keys, moved) {
      const was = tiles;
      tiles = keys.map((k) => tiles.find((t) => t.key === k)).filter(Boolean);
      draw();
      grid.querySelector(`.bt-li[data-key="${CSS.escape(String(moved))}"] .bt-grip`)?.focus({ preventScroll: true });
      try {
        await api.order(tiles.map((t) => t.key)); dirty = true;
        const at = tiles.findIndex((t) => t.key === moved);
        say(at >= 0 ? `Moved to place ${at + 1} of ${tiles.length} ✓` : 'Order saved ✓');
      } catch (e) { tiles = was; draw(); say(e.message, { bad: true }); }
    }
    grid.addEventListener('keydown', (e) => {
      const g = on && e.target.closest('.bt-grip'); if (!g) return;
      const items = [...grid.querySelectorAll('.bt-li')].map((x) => x.dataset.key);
      const m = keyMove(e, items, g.closest('.bt-li').dataset.key);
      if (m) saveOrder(m.order, g.closest('.bt-li').dataset.key);
    });
    grid.addEventListener('click', (e) => {
      if (on && e.target.closest('.bt-grip')) say(coarse ? 'Press and hold a binder, then drag it where you want it.' : 'Drag the binder where you want it, or use the arrow keys.');
    });
    grid.addEventListener('dragstart', (e) => { if (on) e.preventDefault(); });

    function rename(li, t) {
      const name = li.querySelector('.sf-bname'); if (!name) return;
      const input = el('input', 'bt-name-in'); input.type = 'text'; input.value = t.title; input.maxLength = api.nameMax;
      input.setAttribute('aria-label', `New name for ${t.title}`); input.dataset.nodrag = ''; input.enterKeyHint = 'done'; input.autocomplete = 'off';
      name.replaceWith(input); input.focus(); input.select();
      let done = false;
      const finish = async (keep) => {
        if (done) return; done = true;
        const v = clean(input.value);
        if (!keep || !v || v === t.title) {
          if (keep && !v) say('A binder needs a name.', { bad: true });
          input.replaceWith(name); li.querySelector('[data-act="rename"]')?.focus({ preventScroll: true }); return;
        }
        input.disabled = true;
        try { await api.rename(t.key, v); dirty = true; say(`Renamed to “${v}” ✓`); await refresh(t.key, '[data-act="rename"]'); }
        catch (e) { say(e.message, { bad: true }); input.replaceWith(name); }
      };
      input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') { e.preventDefault(); finish(true); }
        if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); finish(false); }
      });
      input.addEventListener('blur', () => finish(true));
    }

    async function remove(t) {
      if (!await ask(`Delete “${t.title}”?`, api.keeps(t.count), 'Delete binder')) return;
      const i = tiles.findIndex((x) => x.key === t.key);
      try {
        await api.remove(t.key); dirty = true;
        say(`“${t.title}” is deleted. Its cards are safe ✓`);
        await refresh();
        const next = grid.querySelectorAll('.bt-li')[Math.min(i, tiles.length - 1)];
        (next ? next.querySelector('.bt-grip') : grid.querySelector('.bt-newbtn'))?.focus({ preventScroll: true });
      } catch (e) { say(e.message, { bad: true }); }
    }

    function newTile() {
      const li = el('li', 'bt-new');
      const b = button('bt-newbtn', null, 'plus', 'New binder');
      b.addEventListener('click', startNew);
      li.append(b);
      return li;
    }
    function startNew() {
      const li = grid.querySelector('.bt-new'); if (!li) return;
      const f = el('form', 'bt-newf'); f.noValidate = true;
      const lab = el('label', 'bt-newf-t', 'New binder');
      const input = el('input'); input.type = 'text'; input.maxLength = api.nameMax; input.placeholder = 'Its name, like “Base Set”'; input.autocomplete = 'off'; input.enterKeyHint = 'done';
      input.id = `btNew${++uid}`; lab.htmlFor = input.id;
      const acts = el('div', 'bt-newf-acts');
      const no = button('btn ghost sm', null, null, 'Cancel'), go = el('button', 'btn sm', 'Make it'); go.type = 'submit';
      acts.append(no, go);
      f.append(lab, input, acts);
      li.replaceChildren(f); input.focus();
      const back = () => { li.replaceWith(newTile()); grid.querySelector('.bt-newbtn')?.focus({ preventScroll: true }); };
      no.addEventListener('click', back);
      input.addEventListener('keydown', (e) => { if (e.key === 'Escape') { e.preventDefault(); back(); } });
      f.addEventListener('submit', async (e) => {
        e.preventDefault();
        const v = clean(input.value);
        if (!v) { input.focus(); say('Type a name for the new binder.', { bad: true }); return; }
        go.disabled = true;
        try {
          await api.create(v); dirty = true;
          await refresh();
          const made = [...grid.querySelectorAll('.bt-li')].pop();
          made?.querySelector('.bt-open')?.focus({ preventScroll: true });
          made?.scrollIntoView({ block: 'nearest' });
          say(`“${v}” is ready ✓ ${coarse ? 'Tap' : 'Click'} its picture to add cards.`);
        } catch (err) { go.disabled = false; say(err.message, { bad: true }); }
      });
    }
  }

  /* ================= one binder's own page: its cards ================= */
  function binderEditor(toggle) {
    const kind = toggle.dataset.btBinder, api = API[kind], key = toggle.dataset.key;
    const bar = toggle.closest('.sf-bbar');
    if (!api || !key || !bar) return;
    const page = bar.parentElement;
    const words = toggle.querySelector('span');
    let title = toggle.dataset.title || 'this binder';
    let cards = [], on = false, dirty = false, box = null, list = null, count = null, empty = null, sorted = false;

    async function load() { const j = await api.cards(key); cards = j.cards || []; if (j.title) title = j.title; }
    async function enter() {
      toggle.disabled = true;
      try { await needSort(); await load(); }
      catch (e) { toggle.disabled = false; say(e.message, { bad: true }); return; }
      toggle.disabled = false;
      on = true;
      toggle.setAttribute('aria-pressed', 'true'); toggle.classList.remove('ghost'); words.textContent = 'Done';
      page.querySelectorAll(toggle.dataset.hide || '#grid').forEach((n) => { if (!n.hidden) { n.hidden = true; n.dataset.btHid = '1'; } });
      box = build();
      const after = [...page.children].filter((n) => n === bar || n.matches('.sf-banner, .sf-desc')).pop() || bar;
      after.after(box);
      draw();
      if (!sorted) {
        sorted = true;
        window.dragSort(list, { item: 'ul.bt-cards > li.bt-card', key: (li) => li.dataset.id, onDrop: ([ids], li) => saveOrder(ids.map(Number), Number(li.dataset.id)) });
      }
      box.querySelector('[data-add]')?.focus({ preventScroll: true });
    }
    function leave() {
      if (dirty) { toggle.disabled = true; location.reload(); return; }
      on = false; box?.remove();
      page.querySelectorAll('[data-bt-hid]').forEach((n) => { n.hidden = false; delete n.dataset.btHid; });
      toggle.setAttribute('aria-pressed', 'false'); toggle.classList.add('ghost'); words.textContent = 'Edit binder';
      toggle.focus({ preventScroll: true });
    }
    toggle.addEventListener('click', () => (on ? leave() : enter()));

    function build() {
      const s = el('section', 'bt-ed'); s.setAttribute('aria-label', `Edit ${title}`);
      const top = el('div', 'bt-ed-top');
      const how = el('p', 'bt-hint', coarse ? 'Press and hold a card, then drag it to change the order. Everything saves by itself.'
        : 'Drag a card to change the order. Everything saves by itself.');
      const add = button('btn', null, 'plus', 'Add cards'); add.dataset.add = '';
      add.addEventListener('click', picker);
      top.append(how, add);
      count = el('p', 'bt-count'); count.setAttribute('aria-live', 'polite');
      list = el('ul', 'bt-cards');
      empty = el('div', 'bt-empty');
      const add2 = button('btn', null, 'plus', 'Add cards'); add2.addEventListener('click', picker);
      empty.append(el('p', '', 'No cards in this binder yet.'), add2);
      s.append(top, count, list, empty);
      list.addEventListener('keydown', (e) => {
        const g = e.target.closest('.bt-grip'); if (!g) return;
        const items = [...list.querySelectorAll('.bt-card')].map((x) => Number(x.dataset.id));
        const id = Number(g.closest('.bt-card').dataset.id);
        const m = keyMove(e, items, id);
        if (m) saveOrder(m.order, id);
      });
      list.addEventListener('click', (e) => {
        if (e.target.closest('.bt-grip')) say(coarse ? 'Press and hold a card, then drag it where you want it.' : 'Drag the card where you want it, or use the arrow keys.');
      });
      list.addEventListener('dragstart', (e) => e.preventDefault());
      return s;
    }
    function draw() {
      list.replaceChildren(...cards.map(card));
      count.textContent = cards.length ? plural(cards.length, 'card') : '';
      empty.hidden = cards.length > 0;
    }
    function card(c) {
      const li = el('li', 'bt-card'); li.dataset.id = String(c.id);
      const pic = el('span', 'bt-card-pic');
      if (c.image) { const im = el('img'); im.src = c.image; im.alt = ''; im.loading = 'lazy'; im.decoding = 'async'; im.draggable = false; pic.append(im); }
      if (c.cover) pic.append(el('span', 'bt-flag', 'Cover'));
      if (c.note) pic.append(el('span', 'bt-note', c.note));
      const grip = button('bt-grip', `Move ${c.title}: drag it, or press the arrow keys`, 'grip');
      const txt = el('span', 'bt-card-t');
      txt.append(el('b', '', c.title));
      if (c.sub) txt.append(el('small', '', c.sub));
      txt.append(el('small', 'bt-price', c.price));
      const acts = el('div', 'bt-acts'); acts.dataset.nodrag = '';
      const cv = button('bt-act', c.cover ? `${c.title} is the cover. Tap to let the cover pick itself again` : `Make ${c.title} the cover`, 'star', 'Cover');
      cv.dataset.act = 'cover'; cv.setAttribute('aria-pressed', String(!!c.cover));
      const rm = button('bt-act bt-del', `Take ${c.title} out of this binder`, 'out', 'Remove'); rm.dataset.act = 'remove';
      cv.addEventListener('click', () => makeCover(c));
      rm.addEventListener('click', () => takeOut(c));
      acts.append(cv, rm);
      li.append(pic, grip, txt, acts);
      return li;
    }
    const focusCard = (id, sel) => list.querySelector(`.bt-card[data-id="${id}"] ${sel}`)?.focus({ preventScroll: true });

    async function saveOrder(ids, moved) {
      const was = cards;
      cards = ids.map((id) => cards.find((c) => c.id === id)).filter(Boolean);
      draw(); focusCard(moved, '.bt-grip');
      try {
        await api.arrange(key, cards.map((c) => c.id)); dirty = true;
        const at = cards.findIndex((c) => c.id === moved);
        say(at >= 0 ? `Moved to place ${at + 1} of ${cards.length} ✓` : 'Order saved ✓');
      } catch (e) { cards = was; draw(); say(e.message, { bad: true }); }
    }
    async function makeCover(c) {
      const to = c.cover ? null : c.id;
      try {
        await api.cover(key, to); dirty = true;
        cards.forEach((x) => { x.cover = to != null && x.id === to; });
        draw(); focusCard(c.id, '[data-act="cover"]');
        say(to == null ? 'The cover picks itself again ✓' : `“${c.title}” is the cover now ✓ It shows first on the binder’s tile.`);
      } catch (e) { say(e.message, { bad: true }); }
    }
    async function takeOut(c) {
      const before = cards.map((x) => x.id), i = before.indexOf(c.id);
      try {
        await api.change(key, { remove: [c.id] }); dirty = true;
        cards = cards.filter((x) => x.id !== c.id); draw();
        const next = list.querySelectorAll('.bt-card')[Math.min(i, cards.length - 1)];
        (next ? next.querySelector('[data-act="remove"]') : box.querySelector('[data-add]'))?.focus({ preventScroll: true });
        say(`“${c.title}” is out of this binder. ${api.stays}`, {
          undo: async () => {
            await api.change(key, { add: [c.id] });
            await api.arrange(key, before);
            if (c.cover) await api.cover(key, c.id);
            await load(); draw(); focusCard(c.id, '[data-act="remove"]');
            say(`“${c.title}” is back ✓`);
          },
        });
      } catch (e) { say(e.message, { bad: true }); }
    }

    function picker() {
      const g = dialog(`Add cards to “${title}”`, { wide: true });
      const find = el('input', 'bt-find'); find.type = 'search'; find.placeholder = api.find; find.setAttribute('aria-label', api.find);
      find.autocomplete = 'off'; find.enterKeyHint = 'search'; find.spellcheck = false;
      const state = el('p', 'bt-state'); state.setAttribute('aria-live', 'polite');
      const results = el('ul', 'bt-picks');
      g.body.append(find, state, results);
      const chosen = new Map();
      const tally = el('span', 'bt-tally'); tally.setAttribute('aria-live', 'polite');
      const no = button('btn ghost', null, null, 'Cancel'), go = button('btn', null, null, 'Add');
      go.disabled = true;
      g.foot.append(tally, no, go);
      no.addEventListener('click', g.close);
      const sync = () => {
        const n = chosen.size, moving = [...chosen.values()].filter((c) => c.inside).length;
        go.disabled = !n; go.textContent = n ? `Add ${plural(n, 'card')}` : 'Add';
        tally.textContent = !n ? 'Tap the cards to add.' : moving ? `${plural(moving, 'card')} will move here from ${moving === 1 ? 'another binder' : 'other binders'}.` : `${plural(n, 'card')} chosen`;
      };
      sync();
      function row(c) {
        const li = el('li');
        const b = button('bt-pc bt-pick', null);
        b.setAttribute('aria-pressed', String(chosen.has(c.id)));
        const pic = el('span', 'bt-pc-pic');
        if (c.image) { const im = el('img'); im.src = c.image; im.alt = ''; im.loading = 'lazy'; pic.append(im); }
        const tick = el('span', 'bt-tick'); tick.append(icon('check')); pic.append(tick);
        const t = el('span', 'bt-pc-t'); t.append(el('b', '', c.title));
        if (c.sub) t.append(el('small', '', c.sub));
        t.append(el('small', 'bt-price', c.note ? `${c.price} · ${c.note}` : c.price));
        if (c.inside) t.append(el('small', 'bt-moves', `In “${c.inside}” · moves here`));
        b.append(pic, t);
        b.addEventListener('click', () => {
          if (chosen.has(c.id)) chosen.delete(c.id); else chosen.set(c.id, c);
          b.setAttribute('aria-pressed', String(chosen.has(c.id))); sync();
        });
        li.append(b); return li;
      }
      let seq = 0, wait = 0;
      async function search() {
        const my = ++seq, q = clean(find.value);
        state.textContent = 'Looking…';
        try {
          const j = await api.choices(key, q); if (my !== seq) return;
          const rows = j.cards || [];
          state.textContent = rows.length ? (kind === 'vault' && !q ? 'The vault’s dearest cards not in this binder yet. Type to find any card.' : '')
            : q ? `No card outside this binder matches “${q}”.` : api.none;
          results.replaceChildren(...rows.map(row));
        } catch (e) { if (my === seq) state.textContent = e.message; }
      }
      find.addEventListener('input', () => { clearTimeout(wait); wait = setTimeout(search, 250); });
      find.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); clearTimeout(wait); search(); } });
      go.addEventListener('click', async () => {
        go.disabled = true;
        try {
          const j = await api.change(key, { add: [...chosen.keys()] }); dirty = true;
          g.close();
          const a = j.added || 0, m = j.moved || 0;
          say(`Added ${plural(a, 'card')}${m ? ` · ${m} moved here from ${m === 1 ? 'another binder' : 'other binders'}` : ''} ✓`);
          await load(); draw();
          list.lastElementChild?.scrollIntoView({ block: 'nearest' });
        } catch (e) { go.disabled = false; state.textContent = e.message; }   // said in the window: a message under it would be hidden
      });
      search();
      find.focus();
    }

    // Arriving from a binder tile in edit mode, or the Manager's "Add or remove cards": open the editor at once.
    const fromLink = () => {
      if (location.hash !== '#edit') return;
      const go = () => { history.replaceState(null, '', location.pathname + location.search); if (!on) enter(); };
      if (kind === 'seller') go();
      else if (window.bbvWho) window.bbvWho().then((j) => { if (j && j.full) go(); });
    };
    fromLink();
    addEventListener('hashchange', fromLink);
  }

  /* ================= Store Manager › Binders: drag a binder by its handle ================= */
  function managerOrder(root) {
    const list = root.querySelector('.st-binders'); if (!list) return;
    const toast = (m, bad) => (window.Admin && window.Admin.toast ? window.Admin.toast(m, { kind: bad ? 'err' : 'ok' }) : say(m, { bad }));
    const items = () => [...list.querySelectorAll('.st-binder')];
    async function save(moved) {
      const ids = items().map((a) => Number(a.dataset.id));
      try {
        await API.seller.order(ids.map(String));
        toast(`Moved to place ${ids.indexOf(Number(moved)) + 1} of ${ids.length} ✓`);
      } catch (e) { toast(e.message, true); }
    }
    needSort().then(() => window.dragSort(list, { item: '.st-binder', handle: '[data-bgrip]', key: (a) => a.dataset.id, onDrop: (_, a) => save(a.dataset.id) })).catch(() => {});
    list.addEventListener('keydown', (e) => {
      const g = e.target.closest('[data-bgrip]'); if (!g) return;
      const art = g.closest('.st-binder'), all = items();
      const m = keyMove(e, all, art); if (!m) return;
      list.replaceChildren(...m.order);
      g.focus({ preventScroll: true });
      save(art.dataset.id);
    });
    list.addEventListener('click', (e) => { if (e.target.closest('[data-bgrip]')) toast('Drag the handle to move the binder, or use the arrow keys.'); });
  }

  /* ================= The Stores page: what a store shows there, chosen by its owner ================= */
  // A seller (on their own store) and a full admin (on the vault's) pick the binders and the highlighted cards shoppers
  // see on /stores. Binders keep the store's own order; highlighted cards show in the order they were picked.
  const FEATURE = {
    vault: {
      get: () => req('/admin/featured'), choices: (q) => req(`/admin/featured/choices?q=${enc(q)}`), save: (b) => req('/admin/featured', b),
      title: 'What Big Boys Vault shows on Stores', order: 'They show in the vault’s order (Edit binders on its store changes it).',
      find: 'Search the vault’s cards: name, number or set', start: 'The vault’s dearest cards. Type to find any card.',
      none: 'No binder has cards for sale yet.',
    },
    seller: {
      get: () => req('/me/featured'), choices: (q) => req(`/me/featured/choices?q=${enc(q)}`), save: (b) => req('/me/featured', b),
      title: 'What your store shows on Stores', order: 'They show in your store’s order (Edit binders on your store changes it).',
      find: 'Search your cards: name, number or set', start: '',
      none: 'None of your binders has cards for sale yet. Make one with Edit binders on your store, then come back.',
    },
  };
  function featureChooser(btn) {
    const api = FEATURE[btn.dataset.feature]; if (!api) return;
    btn.addEventListener('click', async () => {
      btn.disabled = true;
      try { open(await api.get()); } catch (e) { say(e.message, { bad: true }); } finally { btn.disabled = false; }
    });
    function open(state) {
      const max = state.max || 12;
      const g = dialog(api.title, { wide: true }); g.d.classList.add('bt-feat');
      const on = new Set(state.binders.filter((b) => b.on).map((b) => b.key));
      const picked = new Map(state.cards.map((c) => [String(c.id), c]));

      // Binders: tap to show or hide.
      const bh = el('h3', 'bt-sec', 'Binders');
      const bhow = el('p', 'bt-how', state.binders.length ? `Tap a binder to show or hide it. ${api.order}` : api.none);
      const bl = el('ul', 'bt-fbinders');
      bl.replaceChildren(...state.binders.map((b) => {
        const li = el('li');
        const t = button('bt-fb', null); t.setAttribute('aria-pressed', String(on.has(b.key)));
        const cov = el('span', `sf-bcov${b.photo ? ' sf-bphoto' : ''}`); cov.dataset.n = String(b.pics.length);
        b.pics.forEach((p) => { const im = el('img'); im.src = p; im.alt = ''; im.loading = 'lazy'; im.decoding = 'async'; cov.append(im); });
        if (!b.pics.length) { const n = icon('book'); n.setAttribute('class', 'sf-bnone'); cov.append(n); }
        const tick = el('span', 'bt-tick'); tick.append(icon('check'));
        const txt = el('span', 'sf-btxt'); txt.append(el('b', 'sf-bname', b.title), el('small', '', b.words));
        t.append(cov, tick, txt);
        t.addEventListener('click', () => { if (on.has(b.key)) on.delete(b.key); else on.add(b.key); t.setAttribute('aria-pressed', String(on.has(b.key))); sync(); });
        li.append(t); return li;
      }));

      // Highlighted cards: the chosen ones in order (each can come off), then a search to add more.
      const ch = el('h3', 'bt-sec');
      const chow = el('p', 'bt-how', `Pick up to ${max} cards to show off. Tap a card again to take it off.`);
      const chosen = el('ul', 'bt-fchosen'); chosen.setAttribute('aria-label', 'Highlighted cards, in order');
      const find = el('input', 'bt-find'); find.type = 'search'; find.placeholder = api.find; find.setAttribute('aria-label', api.find);
      find.autocomplete = 'off'; find.enterKeyHint = 'search'; find.spellcheck = false;
      const status = el('p', 'bt-state'); status.setAttribute('aria-live', 'polite');
      const results = el('ul', 'bt-picks');
      g.body.append(bh, bhow, bl, ch, chow, chosen, find, status, results);

      const tally = el('span', 'bt-tally'); tally.setAttribute('aria-live', 'polite');
      const no = button('btn ghost', null, null, 'Cancel'), go = button('btn', null, null, 'Save');
      g.foot.append(tally, no, go);
      no.addEventListener('click', g.close);

      function sync() {
        const nb = on.size, nc = picked.size;
        ch.textContent = `Highlighted cards · ${nc} of ${max}`;
        tally.textContent = !nb && !nc ? 'Only your store’s name and its Visit store button will show.'
          : `${plural(nb, 'binder')} and ${plural(nc, 'card')} will show.`;
      }
      function drawChosen() {
        chosen.replaceChildren(...[...picked.values()].map((c) => {
          const li = el('li', 'bt-fc');
          const pic = el('span', 'bt-fc-pic');
          if (c.image) { const im = el('img'); im.src = c.image; im.alt = ''; im.decoding = 'async'; pic.append(im); }
          const x = button('bt-fc-x', `Take ${c.title} off`, 'x');
          x.addEventListener('click', () => { picked.delete(String(c.id)); drawChosen(); mark(); sync(); find.focus({ preventScroll: true }); });
          li.append(pic, el('span', 'bt-fc-t', c.title), x);
          return li;
        }));
        chosen.hidden = !picked.size;
      }
      function mark() { results.querySelectorAll('.bt-pc').forEach((b) => b.setAttribute('aria-pressed', String(picked.has(b.dataset.id)))); }
      function row(c) {
        const li = el('li');
        const b = button('bt-pc bt-pick', null); b.dataset.id = String(c.id);
        b.setAttribute('aria-pressed', String(picked.has(String(c.id))));
        const pic = el('span', 'bt-pc-pic');
        if (c.image) { const im = el('img'); im.src = c.image; im.alt = ''; im.loading = 'lazy'; im.decoding = 'async'; pic.append(im); }
        const tick = el('span', 'bt-tick'); tick.append(icon('check')); pic.append(tick);
        const t = el('span', 'bt-pc-t'); t.append(el('b', '', c.title));
        if (c.sub) t.append(el('small', '', c.sub));
        t.append(el('small', 'bt-price', c.price));
        b.append(pic, t);
        b.addEventListener('click', () => {
          const id = String(c.id);
          if (picked.has(id)) picked.delete(id);
          else if (picked.size >= max) { status.textContent = `Up to ${max} cards. Take one off first.`; return; }
          else picked.set(id, c);
          status.textContent = '';
          b.setAttribute('aria-pressed', String(picked.has(id))); drawChosen(); sync();
        });
        li.append(b); return li;
      }
      let seq = 0, wait = 0;
      async function search() {
        const my = ++seq, q = clean(find.value);
        status.textContent = 'Looking…';
        try {
          const j = await api.choices(q); if (my !== seq) return;
          const rows = j.cards || [];
          status.textContent = rows.length ? (q ? '' : api.start) : q ? `No card for sale matches “${q}”.` : 'You have no cards on sale right now.';
          results.replaceChildren(...rows.map(row));
        } catch (e) { if (my === seq) status.textContent = e.message; }
      }
      find.addEventListener('input', () => { clearTimeout(wait); wait = setTimeout(search, 250); });
      find.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); clearTimeout(wait); search(); } });
      go.addEventListener('click', async () => {
        go.disabled = true;
        try {
          await api.save({ binders: state.binders.map((b) => b.key).filter((k) => on.has(k)), cards: [...picked.keys()] });
          try { sessionStorage.setItem('bbv:toast', 'Saved. This is what shoppers see on Stores now ✓'); } catch (_) {}
          location.reload();
        } catch (e) { go.disabled = false; status.textContent = e.message; status.scrollIntoView({ block: 'nearest' }); }
      });
      drawChosen(); sync(); search();
      (bl.querySelector('.bt-fb') || find).focus({ preventScroll: true });
    }
  }

  /* ---------- start ---------- */
  document.querySelectorAll('[data-bt-edit]').forEach(gridEditor);
  document.querySelectorAll('[data-bt-binder]').forEach(binderEditor);
  document.querySelectorAll('[data-feature]').forEach(featureChooser);
  const mgr = document.getElementById('storeBinders'); if (mgr) managerOrder(mgr);
})();
