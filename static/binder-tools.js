/* Binder tools (new look). Whoever owns binders arranges them where they appear, the way an album is edited where you
   see it: a seller on their own store, a full admin on the vault's store (/store/vault). The same tools for both (the
   owner, 19 Nov 2026: one binder editor; the Managers' Binders lines open the store with it on, at #edit-binders).
   - A store's Binders: "Edit binders" turns the tiles into an editor. Drag a binder to move it (a mouse drags straight
     away; a finger presses and holds first; the arrow keys move the binder whose handle has focus). "+ New binder", and
     on every binder one Edit: its cover (one of its cards, or a photo of the owner's own), its name, what's in it, who
     can see it (everyone, only people with its link, only its owner), its cards, Delete. Tapping a binder's picture
     opens it, ready to add cards.
   - A binder's own page: "Edit binder" lists its cards with Cover, Hide and Remove (Remove can be undone), drag to put
     them in order, or Sort them all at once (A to Z, Z to A, by set, by number, by evolution; Undo puts them back).
     "Add cards" opens the one Add cards page with this binder chosen (the owner, 18 Nov 2026: one page for adding):
     a seller's /me/add?binder=id, the vault's /admin/quick?binder=handle. There the owner's own cards come first, then
     every way to add a new card, and Done comes back here. Hide keeps a card in the binder but out of sight: a
     seller's card is hidden everywhere (its listing is Hidden), a vault card only in that binder. Hidden cards show
     faded, with Show.
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
    eye: '<path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
    eyeoff: '<path d="M3 3l18 18"/><path d="M10.6 5.1A10.4 10.4 0 0 1 12 5c6.4 0 10 7 10 7a17.6 17.6 0 0 1-3.2 4.1"/><path d="M6.6 6.6C3.8 8.4 2 12 2 12s3.6 7 10 7a10.2 10.2 0 0 0 5.4-1.6"/><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2"/>',
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
      save: (k, body) => req(`/admin/binders/${enc(k)}`, body),
      remove: (k) => req(`/admin/binders/${enc(k)}`, { delete: true }),
      cover: (k, id) => req(`/admin/binders/${enc(k)}`, { cover: id == null ? 'auto' : String(id) }),
      upload: (k, file) => { const fd = new FormData(); fd.append('photo', file, 'cover.jpg'); return req(`/admin/binders/${enc(k)}/cover`, fd); },
      cards: (k) => req(`/admin/binders/${enc(k)}/cards`),
      choices: (k, q) => req(`/admin/binders/${enc(k)}/choices?q=${enc(q)}`),
      change: (k, body) => req(`/admin/binders/${enc(k)}/cards`, body),
      arrange: (k, ids) => req(`/admin/binders/${enc(k)}/arrange`, { ids }),
      sort: (k, by) => req(`/admin/binders/${enc(k)}/sort`, { by }),
      hide: (k, c, hidden) => req(`/admin/binders/${enc(k)}/hide`, { id: c.id, hidden }),
      hid: (t) => `“${t}” is hidden in this binder only. It stays in the vault and in any other binder.`,
      nameMax: 40,
      who: [['Public', 'Everyone'], ['Unlisted', 'Only people with the link'], ['Private', 'Only the vault’s staff']],
      words: 'Like “Every Pikachu we have, oldest first”',
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
      save: (k, body) => req(`/me/binders/${enc(k)}/edit`, body),
      remove: (k) => req(`/me/binders/${enc(k)}/delete`, {}),
      cover: (k, id) => req(`/me/binders/${enc(k)}/cover`, { card_id: id == null ? null : Number(id) }),
      upload: (k, file) => { const fd = new FormData(); fd.append('photo', file, 'cover.jpg'); return req(`/me/binders/${enc(k)}/photo`, fd); },
      cards: (k) => req(`/me/binders/${enc(k)}/cards`),
      choices: (k, q) => req(`/me/binders/${enc(k)}/choices?q=${enc(q)}`),
      change: (k, body) => req(`/me/binders/${enc(k)}/cards`, body),
      arrange: (k, ids) => req(`/me/binders/${enc(k)}/order`, { ids }),
      sort: (k, by) => req(`/me/binders/${enc(k)}/sort`, { by }),
      // Hidden is the listing's own status: nobody else sees it anywhere. Show puts it back on sale (out of stock with none left).
      hide: (k, c, hidden) => req(`/me/listings/${enc(c.id)}`, { status: hidden ? 'Hidden' : (c.qty > 0 ? 'Active' : 'Out of stock') }),
      hid: (t) => `“${t}” is hidden. Shoppers don’t see it anywhere; you still see it here.`,
      nameMax: 60,
      who: [['Public', 'Everyone'], ['Unlisted', 'Only people with the link'], ['Private', 'Only me']],
      words: 'Like “Base Set holos, all Near Mint”',
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

  // "Add cards" for one binder: the Add cards page with this binder chosen (the owner, 18 Nov 2026: one page for adding).
  const addHref = (kind, key) => (kind === 'seller' ? `/me/add?binder=${enc(key)}` : `/admin/quick?binder=${enc(key)}`);

  /* ---------- one binder's Edit window: cover, name, what's in it, who can see it, its cards, Delete ---------- */
  // The same window for a seller's binders and the vault's (the owner, 19 Nov 2026). The cover is one of the binder's
  // cards, picks itself, or is a photo of the owner's own. Nothing changes until Save; Delete asks first.
  function editWindow(api, kind, t, { saved, remove }) {
    const g = dialog(`Edit “${t.title}”`); g.d.classList.add('bt-edit');
    let cover = null;                 // what Save does to the cover: null leaves it, { auto }, { card }, { file }
    let cards = [], loaded = false;

    // The cover as the tile shows it now, and the two ways to change it.
    const sec = el('section', 'bt-cv'); sec.setAttribute('aria-label', 'Cover');
    const prev = el('span', 'sf-bcov bt-cv-prev');
    const paint = (pics, photo) => {
      prev.className = `sf-bcov bt-cv-prev${photo ? ' sf-bphoto' : ''}`; prev.dataset.n = String(pics.length);
      prev.replaceChildren(...pics.map((p) => { const im = el('img'); im.src = p; im.alt = ''; im.decoding = 'async'; return im; }));
      if (!pics.length) { const n = icon('book'); n.setAttribute('class', 'sf-bnone'); prev.append(n); }
    };
    paint(t.pics, t.photo);
    const side = el('div', 'bt-cv-side');
    const pickBtn = button('btn ghost sm', null, 'star', 'Pick a card'); pickBtn.setAttribute('aria-expanded', 'false');
    const up = el('label', 'btn ghost sm bt-up'); up.append(icon('image'), el('span', '', 'Upload a photo'));
    const file = el('input', 'sr'); file.type = 'file'; file.accept = 'image/*'; file.setAttribute('aria-label', 'Upload a photo for the cover');
    up.append(file);
    const opts = el('div', 'bt-cover-opts'); opts.append(pickBtn, up);
    const note = el('p', 'bt-state'); note.setAttribute('aria-live', 'polite');
    side.append(el('h3', 'bt-sec', 'Cover'), opts, note);
    sec.append(prev, side);

    // "Pick a card": the binder's cards with a picture, and the cover picking itself.
    const pick = el('div', 'bt-cv-pick'); pick.hidden = true;
    const auto = button('btn ghost sm bt-auto', null, null, api.auto);
    const state = el('p', 'bt-state');
    const list = el('ul', 'bt-covers');
    pick.append(auto, state, list);
    pickBtn.addEventListener('click', () => {
      pick.hidden = !pick.hidden; pickBtn.setAttribute('aria-expanded', String(!pick.hidden));
      if (!pick.hidden && !loaded) { loaded = true; load(); }
      if (!pick.hidden) pick.scrollIntoView({ block: 'nearest' });
    });
    function load() {
      state.textContent = 'Loading the cards in this binder…';
      api.cards(t.key).then((j) => {
        cards = (j.cards || []).filter((c) => c.image && !c.hidden);
        state.textContent = cards.length ? '' : 'No card in this binder has a picture yet. Add cards first, or upload a photo.';
        list.replaceChildren(...cards.map((c) => {
          const li = el('li');
          const b = button('bt-pc', `Make ${c.title} the cover`);
          b.setAttribute('aria-pressed', String(!t.photo && !!c.cover));
          const pic = el('span', 'bt-pc-pic'); const im = el('img'); im.src = c.image; im.alt = ''; im.loading = 'lazy'; pic.append(im);
          const tick = el('span', 'bt-tick'); tick.append(icon('check')); pic.append(tick);
          b.append(pic, el('span', 'bt-pc-t', c.title));
          b.addEventListener('click', () => choose(c, b));
          li.append(b); return li;
        }));
        auto.setAttribute('aria-pressed', String(!t.photo && !cards.some((c) => c.cover)));
      }).catch((e) => { loaded = false; state.textContent = e.message; });
    }
    function choose(c, b) {
      list.querySelectorAll('.bt-pc').forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
      auto.setAttribute('aria-pressed', String(!c));
      file.value = '';
      cover = c ? { card: c.id } : { auto: true };
      const rest = cards.filter((x) => !c || x.id !== c.id).map((x) => x.image);
      paint((c ? [c.image, ...rest] : rest).slice(0, 3), false);
      note.textContent = c ? `“${c.title}” will be the cover.` : 'The cover will pick itself.';
    }
    auto.addEventListener('click', () => choose(null, null));
    file.addEventListener('change', async () => {
      const f = file.files[0]; if (!f) return;
      note.textContent = 'Getting the photo ready…';
      const small = await shrink(f);
      cover = { file: small };
      paint([URL.createObjectURL(small)], true);
      list.querySelectorAll('.bt-pc').forEach((x) => x.setAttribute('aria-pressed', 'false')); auto.setAttribute('aria-pressed', 'false');
      note.textContent = 'This photo will be the cover.';
    });

    // Its name, what's in it, who can see it.
    const field = (words, input) => { const l = el('label', 'bt-field'); l.append(el('span', 'bt-field-t', words), input); return l; };
    const name = el('input'); name.type = 'text'; name.value = t.title; name.maxLength = api.nameMax; name.autocomplete = 'off'; name.enterKeyHint = 'done';
    const desc = el('textarea'); desc.rows = 3; desc.maxLength = 300; desc.value = t.description || ''; desc.placeholder = api.words;
    const who = el('select');
    api.who.forEach(([v, words]) => { const o = el('option', '', words); o.value = v; who.append(o); });
    who.value = t.visibility || 'Public';
    const copy = button('linkbtn bt-copy', null, null, 'Copy its link');
    const showCopy = () => { copy.hidden = who.value !== 'Unlisted'; };
    who.addEventListener('change', showCopy); showCopy();
    copy.addEventListener('click', async () => {
      const url = new URL(t.href, location.origin).href;
      try { await navigator.clipboard.writeText(url); say('Link copied ✓'); } catch (_) { prompt('Copy this link', url); }
    });
    const whoBox = field('Who can see it', who); whoBox.append(copy);
    const fields = el('div', 'bt-fields');
    fields.append(field('Name', name), field('What’s in it', desc), whoBox);

    // Its cards: add more (the Add cards page), or arrange and take them out (the binder's own page).
    const cs = el('section', 'bt-cv-cards'); cs.setAttribute('aria-label', 'Cards');
    const addA = el('a', 'btn ghost sm'); addA.href = addHref(kind, t.key); addA.append(icon('plus'), el('span', '', 'Add cards'));
    const arrA = el('a', 'btn ghost sm'); arrA.href = `${t.href}#edit`; arrA.append(icon('grip'), el('span', '', 'Arrange or remove'));
    const row = el('div', 'bt-cover-opts'); row.append(addA, arrA);
    cs.append(el('h3', 'bt-sec', `Cards · ${t.count.toLocaleString('en-US')}`), row);

    g.body.append(sec, pick, fields, cs);
    const err = el('span', 'bt-tally bt-err'); err.setAttribute('role', 'alert');
    const del = button('btn ghost bt-del-b', null, 'trash', 'Delete'), no = button('btn ghost', null, null, 'Cancel'), go = button('btn', null, null, 'Save');
    g.foot.append(err, del, no, go);
    no.addEventListener('click', g.close);
    del.addEventListener('click', () => { g.close(); remove(); });
    name.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); go.click(); } });
    go.addEventListener('click', async () => {
      const v = clean(name.value);
      if (!v) { err.textContent = 'A binder needs a name.'; name.focus(); return; }
      const body = {};
      if (v !== t.title) body.title = v;
      if (desc.value.trim() !== (t.description || '').trim()) body.description = desc.value.trim();
      if (who.value !== (t.visibility || 'Public')) body.visibility = who.value;
      if (!cover && !Object.keys(body).length) { g.close(); return; }
      err.textContent = ''; go.disabled = true; go.querySelector('span').textContent = 'Saving…';
      try {
        if (cover && cover.file) await api.upload(t.key, cover.file);
        else if (cover && cover.auto) await api.cover(t.key, null);
        else if (cover) await api.cover(t.key, cover.card);
        if (Object.keys(body).length) await api.save(t.key, body);
        g.close(); saved(v);
      } catch (e) { err.textContent = e.message; go.disabled = false; go.querySelector('span').textContent = 'Save'; }
    });
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
      hint = el('p', 'bt-hint', coarse ? 'Press and hold a binder to move it. Tap Edit for its cover, name and who can see it, or its picture for its cards.'
        : 'Drag a binder to move it. Click Edit for its cover, name and who can see it, or its picture for its cards.');
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
      const ed = button('bt-act', `Edit ${t.title}: its cover, name and who can see it`, 'pencil', 'Edit'); ed.dataset.act = 'edit';
      ed.addEventListener('click', () => editWindow(api, kind, t, {
        saved: (title) => { dirty = true; say(`“${title}” is saved ✓`); refresh(t.key, '[data-act="edit"]'); },
        remove: () => remove(t),
      }));
      acts.append(ed);
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

    // The Managers' Binders lines open the store at #edit-binders: editing starts at once (the vault's for full admins).
    const fromLink = () => {
      if (location.hash !== '#edit-binders') return;
      const go = () => { history.replaceState(null, '', location.pathname + location.search); head.scrollIntoView({ block: 'start' }); if (!on) enter(); };
      if (kind === 'seller') go();
      else if (window.bbvWho) window.bbvWho().then((j) => { if (j && j.full) go(); });
    };
    fromLink();
    addEventListener('hashchange', fromLink);
  }

  /* ================= one binder's own page: its cards ================= */
  function binderEditor(toggle) {
    const kind = toggle.dataset.btBinder, api = API[kind], key = toggle.dataset.key;
    const bar = toggle.closest('.sf-bbar');
    if (!api || !key || !bar) return;
    const page = bar.parentElement;
    const words = toggle.querySelector('span');
    let title = toggle.dataset.title || 'this binder';
    let cards = [], on = false, dirty = false, box = null, list = null, count = null, empty = null, sorted = false, sortBar = null, sorting = false;
    // The ways to sort, and what the message says after each.
    const SORTS = [['az', 'A to Z'], ['za', 'Z to A'], ['set', 'By set'], ['number', 'By number'], ['evolution', 'By evolution']];
    const SORTED = { az: 'A to Z', za: 'Z to A', set: 'by set', number: 'by number', evolution: 'by evolution' };

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
      const how = el('p', 'bt-hint', coarse ? 'Press and hold a card, then drag it to change the order, or sort them all below. Everything saves by itself.'
        : 'Drag a card to change the order, or sort them all below. Everything saves by itself.');
      const add = addLink(); add.dataset.add = '';
      top.append(how, add);
      sortBar = el('div', 'bt-sort'); sortBar.setAttribute('role', 'group'); sortBar.setAttribute('aria-label', 'Sort the cards');
      sortBar.append(el('span', 'bt-sort-t', 'Sort:'));
      SORTS.forEach(([by, words]) => {
        const b = button('btn ghost sm bt-sort-b', null, null, words); b.dataset.sort = by;
        b.addEventListener('click', () => sortBy(by, b));
        sortBar.append(b);
      });
      count = el('p', 'bt-count'); count.setAttribute('aria-live', 'polite');
      list = el('ul', 'bt-cards');
      empty = el('div', 'bt-empty');
      const add2 = addLink();
      empty.append(el('p', '', 'No cards in this binder yet.'), add2);
      s.append(top, sortBar, count, list, empty);
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
      const hid = cards.filter((c) => c.hidden).length;
      count.textContent = cards.length ? `${plural(cards.length, 'card')}${hid ? ` · ${hid} hidden` : ''}` : '';
      empty.hidden = cards.length > 0;
      sortBar.hidden = cards.length < 2;
    }
    function card(c) {
      const li = el('li', 'bt-card'); li.dataset.id = String(c.id);
      if (c.hidden) li.dataset.hidden = '';
      const pic = el('span', 'bt-card-pic');
      if (c.image) { const im = el('img'); im.src = c.image; im.alt = ''; im.loading = 'lazy'; im.decoding = 'async'; im.draggable = false; pic.append(im); }
      if (c.cover) pic.append(el('span', 'bt-flag', 'Cover'));
      if (c.note) pic.append(el('span', 'bt-note', c.note));
      if (c.hidden) pic.append(el('span', 'bt-hid', 'Hidden'));
      const grip = button('bt-grip', `Move ${c.title}: drag it, or press the arrow keys`, 'grip');
      const txt = el('span', 'bt-card-t');
      txt.append(el('b', '', c.title));
      if (c.sub) txt.append(el('small', '', c.sub));
      txt.append(el('small', 'bt-price', c.price));
      const acts = el('div', 'bt-acts'); acts.dataset.nodrag = '';
      const cv = button('bt-act', c.cover ? `${c.title} is the cover. Tap to let the cover pick itself again` : `Make ${c.title} the cover`, 'star', 'Cover');
      cv.dataset.act = 'cover'; cv.setAttribute('aria-pressed', String(!!c.cover));
      const hd = c.hidden ? button('bt-act bt-show', `Show ${c.title} again`, 'eye', 'Show')
        : button('bt-act', kind === 'vault' ? `Hide ${c.title} in this binder` : `Hide ${c.title} from shoppers`, 'eyeoff', 'Hide');
      hd.dataset.act = 'hide';
      const rm = button('bt-act bt-del', `Take ${c.title} out of this binder`, 'out', 'Remove'); rm.dataset.act = 'remove';
      cv.addEventListener('click', () => makeCover(c));
      hd.addEventListener('click', () => toggleHidden(c, hd));
      rm.addEventListener('click', () => takeOut(c));
      acts.append(cv, hd, rm);
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
    // One tap puts every card in order; the site works it out and saves it like a drag. Undo puts the old order back.
    async function sortBy(by, b) {
      if (sorting) return;
      sorting = true;
      const btns = [...sortBar.querySelectorAll('[data-sort]')], words = b.querySelector('span'), was = words.textContent;
      btns.forEach((x) => { x.disabled = true; });
      b.setAttribute('aria-busy', 'true'); words.textContent = 'Sorting…';
      say(by === 'evolution' ? 'Sorting… The first time, each Pokémon is looked up, so this can take a little while.' : 'Sorting…');
      const before = cards.map((c) => c.id);
      const put = (ids) => { const at = new Map(cards.map((c) => [c.id, c])); cards = [...ids.map((id) => at.get(id)).filter(Boolean), ...cards.filter((c) => !ids.includes(c.id))]; draw(); };
      try {
        const j = await api.sort(key, by); dirty = true;
        put((j.ids || []).map(Number));
        const miss = j.missing ? ` ${plural(j.missing, 'card')} couldn’t be looked up just now and went last: sort again later to place them.` : '';
        say(`Sorted ${SORTED[by]} ✓${miss}`, {
          undo: async () => { await api.arrange(key, before); put(before); say('Back to the order you had ✓'); },
        });
      } catch (e) { say(e.message, { bad: true }); }
      finally {
        sorting = false; words.textContent = was; b.removeAttribute('aria-busy');
        btns.forEach((x) => { x.disabled = false; });
        if (document.activeElement === document.body) b.focus({ preventScroll: true });
      }
    }
    // Hide keeps the card in the binder, out of sight; Show brings it back.
    async function toggleHidden(c, b) {
      const to = !c.hidden;
      b.disabled = true;
      try {
        await api.hide(key, c, to); dirty = true;
        if (to && c.cover && kind === 'seller') await api.cover(key, null);   // a hidden card can't be the cover
        await load(); draw(); focusCard(c.id, '[data-act="hide"]');
        say(to ? `${api.hid(c.title)} ${coarse ? 'Tap' : 'Click'} the eye to show it again.` : `“${c.title}” shows again ✓`);
      } catch (e) { b.disabled = false; say(e.message, { bad: true }); }
    }
    async function makeCover(c) {
      if (c.hidden && !c.cover) { say('This card is hidden. Show it first, then make it the cover.', { bad: true }); return; }
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
            if (c.hidden && kind === 'vault') await api.hide(key, c, true); // taking it out cleared its "hidden"
            await api.arrange(key, before);
            if (c.cover) await api.cover(key, c.id);
            await load(); draw(); focusCard(c.id, '[data-act="remove"]');
            say(`“${c.title}” is back ✓`);
          },
        });
      } catch (e) { say(e.message, { bad: true }); }
    }

    // "Add cards": the Add cards page, with this binder chosen.
    function addLink() {
      const a = el('a', 'btn');
      a.href = addHref(kind, key);
      a.append(icon('plus'), el('span', '', 'Add cards'));
      return a;
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
})();
