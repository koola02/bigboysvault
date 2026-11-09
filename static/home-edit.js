/* Admin › Home page: the real home page, edited in place.
   - ① Top menu: drag binder names, ✕ / ＋, rename the site.  ② Page map: the only place to add (+), remove (✕) and reorder (drag) parts.
   - On the page: tap words to type over them, tap a picture to replace it, ⚙ Settings and 👁 Hide on each part. */
(() => {
  const list = document.getElementById('edList'); if (!list || list.dataset.ready) return; list.dataset.ready = '1';
  const A = window.Admin;
  const collections = JSON.parse(list.dataset.collections || '[]');
  const el = (tag, cls, txt) => { const n = document.createElement(tag); if (cls) n.className = cls; if (txt != null) n.textContent = txt; return n; };
  const blockOf = (n) => n.closest('.eb');
  const dataOf = (eb) => JSON.parse(eb.dataset.json || '{}');

  /* ---------- re-draw one part from the server (after a picture or settings change) ---------- */
  async function refreshBlock(id) {
    const html = await (await fetch('/admin/page', { credentials: 'same-origin' })).text();
    const doc = new DOMParser().parseFromString(html, 'text/html');
    const fresh = doc.querySelector(`.eb[data-id="${id}"]`), old = list.querySelector(`.eb[data-id="${id}"]`);
    if (!fresh || !old) return location.reload();
    old.replaceWith(fresh); wire(fresh); runScripts(fresh); return fresh;
  }
  function runScripts(scope) { scope.querySelectorAll('script').forEach((s) => { const n = document.createElement('script'); n.textContent = s.textContent; s.replaceWith(n); }); }

  /* ---------- words: type straight on the page ---------- */
  const multiLine = (f) => f === 'body' || f === 'subtitle';
  let wire = function (eb) {
    eb.querySelectorAll('[data-f]').forEach((n) => {
      try { n.contentEditable = 'plaintext-only'; } catch (_) { n.contentEditable = 'true'; }
      if (n.contentEditable !== 'plaintext-only') n.contentEditable = 'true';
      n.spellcheck = true; n.setAttribute('role', 'textbox'); n.setAttribute('aria-label', n.dataset.ph || 'Text');
    });
  }
  /* ---------- number boxes: each box can be renamed, dragged, removed (✕) or added back (＋) ---------- */
  const STAT_KEYS = [['cards', 'Cards'], ['graded', 'Graded'], ['binders', 'Binders'], ['value', 'Collection value']];
  async function saveStats(eb, fd, msg) {
    try { await A.post(`/admin/blocks/${eb.dataset.id}/save`, fd); const fresh = await refreshBlock(eb.dataset.id); A.toast(msg, { kind: 'ok' }); return fresh; }
    catch (err) { A.toast(err.message, { kind: 'err' }); }
  }
  const statOrder = (eb) => [...eb.querySelectorAll('.stat')].map((n) => n.dataset.stat);
  function wireStats(eb) {
    const dl = eb.querySelector('.stats'); if (!dl || dl.dataset.wired) return; dl.dataset.wired = '1';
    dl.querySelectorAll('.stat').forEach((box) => {
      const x = el('button', 'stat-x', '✕'); x.type = 'button'; x.dataset.nodrag = ''; x.setAttribute('aria-label', 'Remove this box');
      x.onclick = () => { const fd = new FormData(); fd.set('items', statOrder(eb).filter((k) => k !== box.dataset.stat).join(',')); saveStats(eb, fd, 'Box removed ✓'); };
      box.append(x);
      const dt = box.querySelector('[data-stat-label]');
      try { dt.contentEditable = 'plaintext-only'; } catch (_) { dt.contentEditable = 'true'; }
      if (dt.contentEditable !== 'plaintext-only') dt.contentEditable = 'true';
      dt.dataset.nodrag = ''; dt.dataset.before = dt.textContent.trim();
      dt.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); dt.blur(); } });
      dt.addEventListener('blur', () => {
        const v = dt.textContent.trim(); if (v === dt.dataset.before) return;
        const fd = new FormData(); fd.set('label_' + box.dataset.stat, v); saveStats(eb, fd, 'Saved ✓');
      });
    });
    const missing = STAT_KEYS.filter(([k]) => !statOrder(eb).includes(k));
    if (missing.length) {
      const add = el('button', 'stat-add'); add.type = 'button'; add.innerHTML = '<span class="st-plus">+</span><span>Add a box</span>';
      add.onclick = () => {
        const box = el('div', 'picker add-picker');
        for (const [k, name] of missing) box.append(pickBtn(name, 'Shows your live ' + name.toLowerCase(), async () => { A.sheet.close(); const fd = new FormData(); fd.set('items', [...statOrder(eb), k].join(',')); saveStats(eb, fd, `“${name}” box added ✓`); }));
        A.sheet.open('Add a number box', box, null);
      };
      dl.after(add);
    }
    if (window.dragSort) window.dragSort(dl, { item: '.stat', key: (n) => n.dataset.stat, onDrop: async ([keys]) => { const fd = new FormData(); fd.set('items', keys.join(',')); saveStats(eb, fd, 'Moved ✓'); } });
  }
  /* ---------- drag cards and binders right on the page ---------- */
  const OWN = 'new';   // a row's "Your order"
  function wireDrag(eb) {
    if (!window.dragSort) return;
    const kind = eb.dataset.kind, done = (msg) => A.toast(msg, { kind: 'ok' }), fail = (err) => A.toast(err.message, { kind: 'err' });
    const keep = (d) => { eb.dataset.json = JSON.stringify(d); };
    const GRIP = '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="9" cy="6" r="1.7"/><circle cx="15" cy="6" r="1.7"/><circle cx="9" cy="12" r="1.7"/><circle cx="15" cy="12" r="1.7"/><circle cx="9" cy="18" r="1.7"/><circle cx="15" cy="18" r="1.7"/></svg>';
    const prep = (items, what) => items.forEach((n) => {
      n.classList.add('ed-drag'); n.title = `Drag to move this ${what}`; n.draggable = false;
      n.querySelectorAll('img').forEach((i) => { i.draggable = false; });
      if (!n.querySelector(':scope > .ed-grip')) { const g = el('span', 'ed-grip'); g.innerHTML = GRIP; n.append(g); }
    });
    if (kind === 'showcase') {
      const grid = eb.querySelector('.sc-grid'), cards = grid ? [...grid.querySelectorAll('.sc-card')] : [];
      if (cards.length < 2) return;
      prep(cards, 'card');
      window.dragSort(grid, { item: '.sc-card', key: (n) => n.dataset.id, onDrop: async ([ids]) => {
        try { const fd = new FormData(); fd.set('ids', ids.join(',')); await A.post(`/admin/blocks/${eb.dataset.id}/save`, fd); const d = dataOf(eb); d.ids = ids.map(Number); keep(d); done('Showcase order saved ✓'); }
        catch (err) { fail(err); }
      } });
    }
    if (kind === 'row') {
      const rail = eb.querySelector('.rail'), cards = rail ? [...rail.querySelectorAll('a.card')] : [];
      if (cards.length < 2) return;
      const ids = (rail.dataset.ids || '').split('|');
      cards.forEach((c, i) => { c.dataset.ids = ids[i] || ''; });
      prep(cards, 'card');
      window.dragSort(rail, { item: 'a.card', key: (n) => n.dataset.ids, onDrop: async ([keys]) => {
        const d = dataOf(eb), own = (d.sort || OWN) === OWN;
        try {
          // A row sorted another way switches to your order, with these cards first, so it keeps showing exactly what you arranged.
          await A.postJSON('/admin/order', { groups: keys.map((k) => k.split(',').filter(Boolean).map(Number)), top: !own });
          if (!own) { const fd = new FormData(); fd.set('sort', OWN); await A.post(`/admin/blocks/${eb.dataset.id}/save`, fd); d.sort = OWN; keep(d); }
          done(own ? 'Order saved ✓ It’s the same order everywhere on the site' : 'Order saved ✓ This row now shows your order');
        } catch (err) { fail(err); }
      } });
    }
    if (kind === 'highlights') {
      const grid = eb.querySelector('.hl-grid'), picks = grid ? [...grid.querySelectorAll('.hl')] : [];
      if (picks.length < 2) return;
      prep(picks, 'trainer');
      window.dragSort(grid, { item: '.hl', key: (n) => n.dataset.id, onDrop: async ([ids]) => {
        try { const fd = new FormData(); fd.set('order', ids.join(',')); await A.post(`/admin/blocks/${eb.dataset.id}/save`, fd); const d = dataOf(eb); d.order = ids.map(Number); keep(d); done('Trainers’ picks order saved ✓'); }
        catch (err) { fail(err); }
      } });
    }
    if (kind === 'binders') {
      const pages = [...eb.querySelectorAll('.bsl-page')], all = eb.querySelectorAll('.bsl-page .binder');
      if (all.length < 2) return;
      prep([...all], 'binder');
      window.dragSort(pages, { item: '.binder', key: (n) => n.dataset.h, onDrop: async (lists) => {
        try { await A.postJSON('/admin/binders/order', { handles: lists.flat() }); await refreshBlock(eb.dataset.id); done('Binder order saved ✓'); }
        catch (err) { fail(err); }
      } });
    }
  }
  const _wire = wire;
  wire = (eb) => { _wire(eb); if (eb.dataset.kind === 'stats') wireStats(eb); wireDrag(eb); };
  list.querySelectorAll('.eb').forEach(wire);
  list.addEventListener('focusin', (e) => {
    const n = e.target.closest('[data-f]'); if (!n) return;
    const raw = dataOf(blockOf(n))[n.dataset.f];
    if (typeof raw === 'string' && raw.includes('{count}')) n.textContent = raw;   // show the live-count placeholder while editing
    n.dataset.before = n.textContent.trim();
  });
  list.addEventListener('keydown', (e) => {
    const n = e.target.closest('[data-f]'); if (!n) return;
    if (e.key === 'Enter' && !(multiLine(n.dataset.f) && e.shiftKey)) { e.preventDefault(); n.blur(); }
    if (e.key === 'Escape') { n.textContent = n.dataset.before || ''; n.blur(); }
  });
  list.addEventListener('focusout', async (e) => {
    const n = e.target.closest('[data-f]'); if (!n) return;
    // textContent, not innerText: titles are shown in capitals by the design, but saved exactly as typed.
    const eb = blockOf(n), key = n.dataset.f, val = n.textContent.replace(/ /g, ' ').trim();
    if (val === (n.dataset.before || '')) { if (val.includes('{count}')) await refreshBlock(eb.dataset.id); return; }
    if (key === 'title' && eb.dataset.kind === 'hero' && !val) { n.textContent = n.dataset.before; return A.toast('The big title can’t be empty.', { kind: 'err' }); }
    const fd = new FormData(); fd.set(key, val);
    try {
      await A.post(`/admin/blocks/${eb.dataset.id}/save`, fd);
      const d = dataOf(eb); d[key] = val; eb.dataset.json = JSON.stringify(d);
      A.toast('Saved ✓', { kind: 'ok' });
      if (val.includes('{count}') || (eb.dataset.kind === 'row' && key === 'title' && !val)) await refreshBlock(eb.dataset.id);
    } catch (err) { n.textContent = n.dataset.before || ''; A.toast(err.message, { kind: 'err' }); }
  });
  // While editing, links and buttons on the page don't navigate away.
  list.addEventListener('click', (e) => {
    const a = e.target.closest('a'); if (a && list.contains(a) && !e.target.closest('.eb-tools')) e.preventDefault();
  });

  /* ---------- pictures: tap to replace ---------- */
  const picker = el('input'); picker.type = 'file'; picker.accept = 'image/*'; picker.hidden = true; document.body.append(picker);
  let pickFor = null;
  list.addEventListener('click', (e) => {
    const fig = e.target.closest('[data-img]'); if (!fig || !list.contains(fig)) return;
    e.preventDefault(); pickFor = { id: blockOf(fig).dataset.id, key: fig.dataset.img }; picker.value = ''; picker.click();
  });
  picker.addEventListener('change', async () => {
    const f = picker.files[0]; if (!f || !pickFor) return;
    const { id, key } = pickFor; A.toast('Uploading picture…');
    try {
      const file = f.type === 'image/png' ? f : await A.resize(f);
      const fd = new FormData(); fd.set(key, file, file.name || 'picture.jpg');
      await A.post(`/admin/blocks/${id}/save`, fd);
      await refreshBlock(id); A.toast('Picture changed ✓', { kind: 'ok' });
    } catch (err) { A.toast(err.message, { kind: 'err' }); }
  });

  /* ---------- tools on each part ---------- */
  const SORTS = [['new', 'Your order (drag the cards on the page)'], ['newest', 'Newest first'], ['price-desc', 'Most valuable first'], ['price-asc', 'Cheapest first'], ['title', 'A to Z']];
  const SETTINGS = {
    hero: [['text', 'button_link', 'When someone taps the button, go to', 'For example /all (every card) or /collections/favorites'], ['text', 'sticker', 'Little sticker on the pictures (optional)', 'For example “New this week”. Leave empty for none.'], ['remove', 'img_a', 'Remove the left picture'], ['remove', 'img_b', 'Remove the right picture']],
    feature: [['text', 'button_link', 'When someone taps the button, go to', 'For example /all or /collections/trainers'], ['remove', 'img', 'Remove the picture']],
    row: [['select', 'handle', 'Show cards from', collections.map((c) => [c.handle, c.title])], ['select', 'sort', 'Order', SORTS], ['switch', 'spotlight', 'Make this row stand out', 'Dark band with gold edges.']],
    ticker: [['area', 'phrases', 'Words that slide across', 'One per line.']],
    binders: [['link', '/admin/binders', 'Choose binders and pictures →', 'Drag binders right on the page to change their order.']],
  };
  function openSettings(eb) {
    const c1 = document.querySelector('.sheet .sheet-h [data-close]'); if (c1) c1.textContent = 'Cancel';
    if (eb.dataset.kind === 'showcase') return openShowcase(eb);
    const kind = eb.dataset.kind, data = dataOf(eb), fields = SETTINGS[kind] || [];
    const form = el('form', 'sheet-form'); form.onsubmit = (e) => { e.preventDefault(); A.sheet.save(); };
    const get = {}, removed = new Set();
    for (const [type, key, label, hint] of fields) {
      const wrap = el('div', 'sf');
      if (type === 'link') { if (hint) wrap.append(el('p', 'hint', hint)); const a = el('a', 'btn ghost', label); a.href = key; wrap.append(a); form.append(wrap); continue; }
      if (type === 'remove') {
        if (!data[key]) continue;
        const b = el('button', 'pbtn danger', label); b.type = 'button';
        b.onclick = () => { removed.add(key); b.textContent = 'Will be removed when you save'; b.disabled = true; }; wrap.append(b); form.append(wrap); continue;
      }
      if (type === 'switch') {
        const row = el('label', 'sf-switch'); const inp = el('input'); inp.type = 'checkbox'; inp.className = 'sr'; inp.checked = !!data[key];
        row.append(el('span', null, label), inp, el('span', 'knob')); wrap.append(row); if (hint) wrap.append(el('p', 'hint', hint));
        get[key] = () => (inp.checked ? '1' : '0'); form.append(wrap); continue;
      }
      const lab = el('label', 'sf-label'); lab.append(el('span', 'sf-l', label));
      let inp;
      if (type === 'area') { inp = el('textarea'); inp.rows = 5; }
      else if (type === 'select') { inp = el('select'); for (const [v, t] of hint) { const o = el('option', null, t); o.value = v; inp.append(o); } }
      else { inp = el('input'); inp.type = 'text'; }
      inp.value = type === 'select' ? (data[key] || (key === 'sort' ? 'new' : 'all')) : (data[key] || '');
      inp.autocomplete = 'off'; lab.append(inp); wrap.append(lab); if (hint && type !== 'select') wrap.append(el('p', 'hint', hint));
      get[key] = () => inp.value; form.append(wrap);
    }
    A.sheet.open(`${eb.dataset.label}: settings`, form, Object.keys(get).length || kind !== 'binders' ? async () => {
      const fd = new FormData(); for (const k in get) fd.set(k, get[k]()); for (const k of removed) fd.set('rm_' + k, '1');
      await A.post(`/admin/blocks/${eb.dataset.id}/save`, fd);
      A.sheet.close(); await refreshBlock(eb.dataset.id); A.toast('Saved ✓', { kind: 'ok' });
    } : null);
  }
  list.addEventListener('click', async (e) => {
    const b = e.target.closest('.eb-tools [data-act]'); if (!b) return;
    const eb = blockOf(b), id = eb.dataset.id;
    try {
      if (b.dataset.act === 'settings') return openSettings(eb);
      if (b.dataset.act === 'vis') {
        const show = eb.classList.contains('eb-off');
        await A.post(`/admin/blocks/${id}/visible`, { on: show ? '1' : '0' });
        await refreshBlock(id); A.toast(show ? 'Visitors can see this part again ✓' : 'Hidden from visitors. It stays here, greyed out.', { kind: 'ok' });
      }
      if (b.dataset.act === 'del') {
        A.undoable({
          label: `Deleted “${eb.dataset.label}”`,
          hide: () => { eb.hidden = true; }, show: () => { eb.hidden = false; },
          commit: async () => { await A.post(`/admin/blocks/${id}/delete`); eb.remove(); },
        });
      }
    } catch (err) { A.toast(err.message, { kind: 'err' }); }
  });

  /* ---------- add a part: named by what it shows, dropped exactly where you asked ---------- */
  const PARTS = [
    ['hero', 'Top banner', 'Big title, two pictures and a button'],
    ['binders', 'Binders slider', 'Your binders, four at a time'],
    ['feature', 'Picture banner', 'Words, a button and a picture'],
    ['ticker', 'Moving text', 'A gold strip of sliding words'],
    ['text', 'Text', 'A title and a few lines'],
    ['highlights', 'Trainers\' picks', 'Each approved trainer\'s highlighted card, with their name and picture'],
    ['vouches', 'Vouches', 'What approved trainers say about the vault'],
  ];
  function pickBtn(title, desc, onPick, cls = '') {
    const b = el('button', 'pick ' + cls); b.type = 'button'; b.append(el('strong', null, title), el('small', null, desc)); b.onclick = onPick; return b;
  }
  function openAdd(after) {
    const c0 = document.querySelector('.sheet .sheet-h [data-close]'); if (c0) c0.textContent = 'Cancel';
    const box = el('div', 'picker add-picker');
    const add = (kind, extra = {}) => async () => {
      try {
        const body = { kind, ...extra }; if (after != null) body.after = after;
        const j = await A.post('/admin/blocks/add', body); A.sheet.close();
        const html = await (await fetch('/admin/page', { credentials: 'same-origin' })).text();
        const fresh = new DOMParser().parseFromString(html, 'text/html').querySelector(`.eb[data-id="${j.id}"]`);
        if (!fresh) return location.reload();
        const prev = after > 0 ? list.querySelector(`.eb[data-id="${after}"]`) : null;
        if (after === -1) list.prepend(fresh); else if (prev) prev.after(fresh); else list.append(fresh);
        wire(fresh); runScripts(fresh);
        fresh.scrollIntoView({ behavior: 'smooth', block: 'center' }); fresh.classList.add('dropped');
        if (kind === 'showcase') { setTimeout(() => openShowcase(fresh), 450); return; }
        A.toast('Added at the bottom ✓ Drag it in the page map to move it.', { kind: 'ok', ms: 4500 });
        const first = fresh.querySelector('[data-f]');
        if (first) setTimeout(() => { first.focus(); const r = document.createRange(); r.selectNodeContents(first); const sel = getSelection(); sel.removeAllRanges(); sel.addRange(r); }, 500);
      } catch (err) { A.toast(err.message, { kind: 'err' }); }
    };
    box.append(el('p', 'pick-h', 'Show off cards'));
    box.append(pickBtn('⭐ Showcase', 'Pick any cards yourself. 1 card = big spotlight, more cards = smaller', add('showcase'), 'pick-star'));
    for (const c of collections) {
      box.append(pickBtn(c.handle === 'all' ? 'All cards' : c.title, c.handle === 'all' ? 'Newest cards from your whole collection' : `A row of cards from the ${c.title} binder`, add('row', { handle: c.handle })));
    }
    box.append(el('p', 'pick-h', 'Page parts'));
    for (const [kind, name, desc] of PARTS) box.append(pickBtn(name, desc, add(kind)));
    A.sheet.open('Add a part', box, null);
  }

  /* ---------- ② the page map: the page drawn small, top to bottom. Drag = move, ✕ = remove, + = add. ---------- */
  const G = 'fill="currentColor"';
  const ICONS = {
    hero: `<rect x="4" y="9" width="24" height="5" rx="1.5" ${G}/><rect x="4" y="17" width="18" height="3" rx="1.5" ${G} opacity=".6"/><rect x="4" y="25" width="12" height="5" rx="2" ${G} opacity=".9"/><circle cx="46" cy="20" r="13" fill="none" stroke="currentColor" stroke-width="3"/><circle cx="46" cy="22" r="6" ${G} opacity=".7"/>`,
    binders: `<rect x="17" y="3" width="13" height="16" rx="2" ${G}/><rect x="34" y="3" width="13" height="16" rx="2" ${G}/><rect x="17" y="22" width="13" height="16" rx="2" ${G} opacity=".7"/><rect x="34" y="22" width="13" height="16" rx="2" ${G} opacity=".7"/>`,
    row: `<rect x="3" y="10" width="10" height="14" rx="1.5" ${G}/><rect x="15.5" y="10" width="10" height="14" rx="1.5" ${G}/><rect x="28" y="10" width="10" height="14" rx="1.5" ${G}/><rect x="40.5" y="10" width="10" height="14" rx="1.5" ${G}/><rect x="53" y="10" width="8" height="14" rx="1.5" ${G} opacity=".5"/><rect x="3" y="28" width="22" height="3" rx="1.5" ${G} opacity=".5"/>`,
    showcase: `<path d="M32 2l2.4 5 5.4.6-4 3.7 1.1 5.3L32 14l-4.9 2.6 1.1-5.3-4-3.7 5.4-.6z" ${G}/><rect x="23" y="17" width="18" height="21" rx="2.5" fill="none" stroke="currentColor" stroke-width="3"/><rect x="27" y="21" width="10" height="13" rx="1" ${G} opacity=".6"/>`,
    stats: `<rect x="3" y="12" width="12" height="16" rx="2" ${G}/><rect x="18" y="12" width="12" height="16" rx="2" ${G} opacity=".8"/><rect x="33" y="12" width="12" height="16" rx="2" ${G} opacity=".65"/><rect x="48" y="12" width="13" height="16" rx="2" ${G} opacity=".5"/>`,
    ticker: `<rect x="2" y="13" width="60" height="14" rx="2" fill="none" stroke="currentColor" stroke-width="2.5"/><path d="M10 17l4 3-4 3M24 17l4 3-4 3M38 17l4 3-4 3M52 17l4 3-4 3" stroke="currentColor" stroke-width="2.5" fill="none"/>`,
    text: `<rect x="8" y="8" width="30" height="5" rx="1.5" ${G}/><rect x="8" y="17" width="48" height="3" rx="1.5" ${G} opacity=".6"/><rect x="8" y="23" width="44" height="3" rx="1.5" ${G} opacity=".6"/><rect x="8" y="29" width="34" height="3" rx="1.5" ${G} opacity=".6"/>`,
    feature: `<rect x="2" y="5" width="60" height="30" rx="4" fill="none" stroke="currentColor" stroke-width="2.5"/><rect x="8" y="12" width="22" height="4" rx="1.5" ${G}/><rect x="8" y="20" width="16" height="3" rx="1.5" ${G} opacity=".6"/><rect x="8" y="26" width="10" height="4" rx="2" ${G}/><rect x="40" y="10" width="15" height="20" rx="2" ${G} opacity=".7"/>`,
  };
  const strip = document.getElementById('edStrip');
  const short = (eb) => { const l = eb.dataset.label || ''; const i = l.indexOf(' · '); return { kind: i > 0 ? l.slice(0, i) : l, name: i > 0 ? l.slice(i + 3) : '' }; };
  // A tile shows the part's own pictures (its cards, covers, banner art) or its own words / numbers, not a generic icon.
  function preview(eb) {
    const kind = eb.dataset.kind;
    const imgs = [...eb.querySelectorAll('img')].map((i) => i.currentSrc || i.src).filter((u) => u && !/vault-ring|logo-sm/.test(u));
    const want = { hero: 2, binders: 4, row: 3, showcase: 3, feature: 1, highlights: 3, vouches: 3 }[kind] || 0;
    if (want && imgs.length) {
      const box = el('span', 'st-pic st-photos st-' + kind);
      [...new Set(imgs)].slice(0, want).forEach((u) => { const i = el('img'); i.src = u; i.alt = ''; i.loading = 'lazy'; i.draggable = false; box.append(i); });
      return box;
    }
    if (kind === 'stats') {
      const box = el('span', 'st-pic st-words st-nums');
      eb.querySelectorAll('.stats dd').forEach((d) => box.append(el('b', null, d.textContent.trim())));
      return box;
    }
    if (kind === 'ticker' || kind === 'text') {
      const words = kind === 'ticker' ? [...eb.querySelectorAll('.ticker span')].slice(0, 2).map((x) => x.textContent.trim()).join(' ★ ') : (eb.querySelector('.prose')?.textContent || '').trim();
      const box = el('span', 'st-pic st-words' + (kind === 'ticker' ? ' st-tick' : '')); box.textContent = words.slice(0, 60) || '…'; return box;
    }
    const box = el('span', 'st-pic'); box.innerHTML = `<svg viewBox="0 0 64 40" aria-hidden="true">${ICONS[kind] || ICONS.text}</svg>`; return box;
  }
  function drawStrip() {
    const ebs = [...list.querySelectorAll(':scope > .eb')];
    // Scale: the map is the page shrunk to the map's width, so each part keeps its real proportions.
    const k = Math.max(0.05, (strip.clientWidth || 200) / Math.max(1, list.clientWidth));
    const tiles = ebs.map((eb) => {
      const li = el('li', 'st-tile' + (eb.classList.contains('eb-off') ? ' off' : '')); li.dataset.id = eb.dataset.id; li.dataset.kind = eb.dataset.kind;
      const h = Math.round(Math.min(150, Math.max(40, (eb.getBoundingClientRect().height - 46) * k)));
      li.style.height = h + 'px';
      const pic = preview(eb);
      const { kind, name } = short(eb);
      const txt = el('span', 'st-txt'); txt.append(el('span', 'st-name', name || kind)); if (name) txt.append(el('span', 'st-kind', kind));
      const x = el('button', 'st-x', '✕'); x.type = 'button'; x.dataset.nodrag = ''; x.setAttribute('aria-label', 'Remove ' + eb.dataset.label);
      li.title = eb.dataset.label + ': tap to see it, drag to move';
      li.append(pic, txt, x);
      if (eb.classList.contains('eb-off')) li.append(el('span', 'st-off', 'hidden'));
      return li;
    });
    const add = el('li', 'st-add-li'); const ab = el('button', 'st-add'); ab.type = 'button'; ab.innerHTML = '<span class="st-plus">+</span><span>Add a part</span>';
    ab.onclick = () => openAdd(null); add.append(ab);
    strip.replaceChildren(...tiles, add);
  }
  strip.addEventListener('click', (e) => {
    const x = e.target.closest('.st-x');
    const tile = e.target.closest('.st-tile'); if (!tile) return;
    const eb = list.querySelector(`:scope > .eb[data-id="${tile.dataset.id}"]`); if (!eb) return;
    if (x) {
      A.undoable({
        label: `Removed “${eb.dataset.label}”`,
        hide: () => { eb.hidden = true; tile.hidden = true; }, show: () => { eb.hidden = false; tile.hidden = false; },
        commit: async () => { await A.post(`/admin/blocks/${eb.dataset.id}/delete`); eb.remove(); },
      });
      return;
    }
    const top = eb.getBoundingClientRect().top + scrollY - (document.querySelector('.bar').getBoundingClientRect().height + 16);
    scrollTo({ top, behavior: 'smooth' });
    eb.classList.remove('flash'); void eb.offsetWidth; eb.classList.add('flash');
    if (matchMedia('(max-width: 1099px)').matches) setMap(false);
  });
  if (window.dragSort) window.dragSort(strip, { item: '.st-tile', key: (n) => n.dataset.id, onDrop: async ([ids]) => {
    try {
      await A.postJSON('/admin/blocks/order', { ids: ids.map(Number) });
      ids.forEach((id) => { const eb = list.querySelector(`:scope > .eb[data-id="${id}"]`); if (eb) list.append(eb); });
      A.toast('Moved ✓', { kind: 'ok' });
    } catch (err) { A.toast(err.message, { kind: 'err' }); location.reload(); }
  } });
  let redraw; const later = () => { clearTimeout(redraw); redraw = setTimeout(drawStrip, 120); };
  drawStrip();
  new MutationObserver(later).observe(list, { childList: true });
  new ResizeObserver(later).observe(list);
  // Show / hide the map (it starts open on big screens, closed on phones).
  const mapEl = document.getElementById('edMap'), mapBtn = document.getElementById('mapToggle');
  function setMap(open) { mapEl.classList.toggle('closed', !open); mapBtn.textContent = open ? 'Hide' : 'Show'; mapBtn.setAttribute('aria-expanded', String(open)); if (open) later(); }
  mapBtn.addEventListener('click', () => setMap(mapEl.classList.contains('closed')));
  setMap(!matchMedia('(max-width: 1099px)').matches);

  /* ---------- ① the top menu: drag the binder names, ✕ to take one out, ＋ to put one in, tap the name to rename ---------- */
  const mnList = document.getElementById('mnList'), mnName = document.getElementById('mnName');
  const menuHandles = () => [...mnList.querySelectorAll('.mn-chip')].map((c) => c.dataset.h);
  function syncSiteMenu() {
    const nav = document.querySelector('.bar .menu'); if (!nav) return;
    nav.replaceChildren(...[...mnList.querySelectorAll('.mn-chip')].map((c) => { const a = el('a', null, c.querySelector('.mn-t').textContent); a.href = '/collections/' + c.dataset.h; return a; }));
  }
  async function saveMenu(msg) {
    try { await A.postJSON('/admin/menu', { handles: menuHandles() }); syncSiteMenu(); A.toast(msg, { kind: 'ok' }); }
    catch (err) { A.toast(err.message, { kind: 'err' }); }
  }
  function chip(c) {
    const li = el('li', 'mn-chip'); li.dataset.h = c.handle;
    const x = el('button', 'mn-x', '✕'); x.type = 'button'; x.dataset.nodrag = ''; x.setAttribute('aria-label', `Take ${c.title} out of the menu`);
    li.append(el('span', 'mn-t', c.title), x); return li;
  }
  mnList.addEventListener('click', (e) => {
    const x = e.target.closest('.mn-x'); if (!x) return;
    const li = x.closest('.mn-chip'); const name = li.querySelector('.mn-t').textContent;
    li.remove(); saveMenu(`“${name}” is out of the menu`);
  });
  if (window.dragSort) window.dragSort(mnList, { item: '.mn-chip', key: (n) => n.dataset.h, onDrop: async () => saveMenu('Menu order saved ✓') });
  document.getElementById('mnAdd').addEventListener('click', () => {
    const inMenu = new Set(menuHandles());
    const out = collections.filter((c) => c.handle !== 'all' && !inMenu.has(c.handle));
    const box = el('div', 'mn-pick');
    if (!out.length) box.append(el('p', 'hint', 'Every binder is already in the menu. Make a new binder in the Binders tab.'));
    for (const c of out) {
      const b = el('button', 'mn-opt'); b.type = 'button';
      const pic = el('span', 'mn-opt-pic'); if (c.image) { const i = el('img'); i.src = c.image; i.alt = ''; pic.append(i); }
      b.append(pic, el('strong', null, c.title), el('small', null, `${c.count} card${c.count === 1 ? '' : 's'}`));
      b.onclick = () => { mnList.append(chip(c)); A.sheet.close(); saveMenu(`“${c.title}” is in the menu ✓`); };
      box.append(b);
    }
    A.sheet.open('Put a binder in the top menu', box, null);
  });
  // Site name: type straight into the copy of the header.
  try { mnName.contentEditable = 'plaintext-only'; } catch (_) { mnName.contentEditable = 'true'; }
  if (mnName.contentEditable !== 'plaintext-only') mnName.contentEditable = 'true';
  let nameBefore = mnName.textContent.trim();
  mnName.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); mnName.blur(); } if (e.key === 'Escape') { mnName.textContent = nameBefore; mnName.blur(); } });
  mnName.addEventListener('blur', async () => {
    const v = mnName.textContent.trim(); if (v === nameBefore) return;
    if (!v) { mnName.textContent = nameBefore; return A.toast('The site needs a name.', { kind: 'err' }); }
    try { await A.post('/admin/settings', { brand: v }); nameBefore = v; A.toast('Site name saved ✓', { kind: 'ok' }); setTimeout(() => location.reload(), 900); }
    catch (err) { mnName.textContent = nameBefore; A.toast(err.message, { kind: 'err' }); }
  });

  /* ---------- showcase: pick any cards from the collection ---------- */
  function openShowcase(eb) {
    let ids = (dataOf(eb).ids || []).map(Number);
    const box = el('div', 'scp');
    const how = el('p', 'hint', '1 card gets the biggest spotlight. 2 cards are a little smaller, 3–4 smaller again, 5 or more show as a grid.');
    const chosenH = el('p', 'pick-h'); const chosen = el('ul', 'scp-chosen');
    const search = el('input'); search.type = 'search'; search.placeholder = 'Search your cards by name or number'; search.className = 'scp-search';
    const results = el('div', 'scp-results');
    const cache = new Map();
    box.append(how, chosenH, chosen, search, results);
    const thumb = (c, cls) => { const t = el('span', cls); if (c.image) { const i = el('img'); i.src = c.image; i.alt = ''; i.loading = 'lazy'; t.append(i); } return t; };
    function drawChosen() {
      chosenH.textContent = ids.length ? `Showing ${ids.length} card${ids.length === 1 ? '' : 's'} (drag to change the order)` : 'No cards picked yet. Tap cards below to add them.';
      chosen.replaceChildren(...ids.map((id) => {
        const c = cache.get(id) || { id, title: '…' };
        const li = el('li', 'scp-c'); li.dataset.id = id; li.title = c.title;
        const x = el('button', 'scp-x', '×'); x.type = 'button'; x.dataset.nodrag = ''; x.setAttribute('aria-label', 'Remove ' + c.title);
        x.onclick = () => { ids = ids.filter((v) => v !== id); drawChosen(); drawResults(); };
        li.append(thumb(c, 'scp-ti'), x); return li;
      }));
    }
    let shown = [];
    function drawResults() {
      results.replaceChildren(...shown.map((c) => {
        const b = el('button', 'scp-r' + (ids.includes(c.id) ? ' on' : '')); b.type = 'button';
        b.append(thumb(c, 'scp-ti'), el('span', 'scp-rt', c.title), el('span', 'scp-rp', c.price));
        b.onclick = () => { ids = ids.includes(c.id) ? ids.filter((v) => v !== c.id) : [...ids, c.id].slice(0, 24); drawChosen(); drawResults(); };
        return b;
      }));
      if (!shown.length) results.append(el('p', 'hint', 'No cards match.'));
    }
    async function load(q) {
      const j = await (await fetch('/admin/pick?q=' + encodeURIComponent(q), { headers: { Accept: 'application/json' }, credentials: 'same-origin' })).json();
      shown = j.cards || []; shown.forEach((c) => cache.set(c.id, c)); drawResults();
    }
    (async () => {
      if (ids.length) { const j = await (await fetch('/admin/pick?ids=' + ids.join(','), { headers: { Accept: 'application/json' }, credentials: 'same-origin' })).json(); (j.cards || []).forEach((c) => cache.set(c.id, c)); }
      drawChosen(); load('');
    })();
    let t; search.addEventListener('input', () => { clearTimeout(t); t = setTimeout(() => load(search.value), 250); });
    A.sheet.open('Pick cards to show off', box, async () => {
      const fd = new FormData(); fd.set('ids', ids.join(','));
      await A.post(`/admin/blocks/${eb.dataset.id}/save`, fd);
      A.sheet.close(); await refreshBlock(eb.dataset.id); A.toast(`Showcase saved ✓ ${ids.length} card${ids.length === 1 ? '' : 's'}`, { kind: 'ok' });
    });
    setTimeout(() => window.dragSort(chosen, { item: '.scp-c', key: (n) => Number(n.dataset.id), onDrop: async ([order]) => { ids = order; } }), 50);
  }
})();
