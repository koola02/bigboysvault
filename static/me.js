// "My trainer": everything saves right here, on the page.
(() => {
  const root = document.getElementById('me'); if (!root) return;
  const $ = (id) => document.getElementById(id);
  const MAX = Number(root.dataset.max);
  let highlight = root.dataset.highlight;

  const say = (msg, bad) => {
    let t = document.querySelector('.me-toast');
    if (!t) { t = document.createElement('div'); t.className = 'me-toast'; t.setAttribute('role', 'status'); document.body.append(t); }
    t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on');
    clearTimeout(say.t); say.t = setTimeout(() => t.classList.remove('on'), bad ? 5000 : 2400);
  };
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

  /* ---------- picture ---------- */
  // Phone photos are huge: shrink here first, so the upload is quick on mobile data.
  async function shrink(file, max = 800) {
    const url = URL.createObjectURL(file);
    try {
      const img = new Image(); img.src = url; await img.decode();
      const k = Math.min(1, max / Math.max(img.naturalWidth, img.naturalHeight));
      const c = document.createElement('canvas'); c.width = Math.round(img.naturalWidth * k); c.height = Math.round(img.naturalHeight * k);
      c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
      return await new Promise((res) => c.toBlob(res, 'image/jpeg', 0.9));
    } finally { URL.revokeObjectURL(url); }
  }
  let picked = null;
  $('meFile').addEventListener('change', async (e) => {
    const f = e.target.files[0]; if (!f) return;
    try {
      picked = await shrink(f);
      const old = $('meAvatar');
      const img = document.createElement('img'); img.className = 'av av-xl'; img.id = 'meAvatar'; img.alt = ''; img.src = URL.createObjectURL(picked);
      old.replaceWith(img);
      saveProfile();
    } catch (_) { say('That picture couldn’t be read. Try a JPEG or PNG.', true); }
    e.target.value = '';
  });

  /* ---------- name and description ---------- */
  const counter = (area, out) => { const paint = () => { out.textContent = area.value.length; }; area.addEventListener('input', paint); paint(); };
  const form = $('meProfile');
  counter(form.elements.bio, $('bioCount'));
  const status = (live) => {
    const s = $('meStatus'); s.classList.toggle('ok', live);
    s.textContent = live ? '✓ Your page is live.' : '⏳ The vault owner still has to approve your picture, description and binder. Your vouches go up straight away.';
  };
  async function saveProfile() {
    const fd = new FormData(form);
    if (picked) fd.set('avatar', picked, 'avatar.jpg');
    try {
      const j = await send('/me/profile', fd);
      picked = null;
      status(j.approved);
      say(j.approved ? 'Saved ✓' : 'Saved ✓ The vault owner will check it soon.');
    } catch (err) { say(err.message, true); }
  }
  form.addEventListener('submit', (e) => { e.preventDefault(); saveProfile(); });

  /* ---------- binder ---------- */
  const binder = $('meBinder');
  const count = () => binder.querySelectorAll('.me-c').length;
  const conds = ['Near Mint', 'LP-NM', 'Lightly Played', 'MP-LP', 'Moderately Played', 'HP-MP', 'Heavily Played', 'Damaged-HP', 'Damaged', 'Sealed'];
  function paint() {
    const n = count();
    $('meCount').textContent = n;
    $('meEmpty').hidden = n > 0;
    binder.querySelectorAll('.me-c').forEach((c) => {
      const on = c.dataset.id === String(highlight);
      c.classList.toggle('star', on);
      c.querySelector('[data-star]').setAttribute('aria-pressed', String(on));
    });
    document.querySelectorAll('.me-res').forEach((b) => { b.disabled = n >= MAX && !b.classList.contains('in'); });
  }
  function renderSugMe(c, p) {
    if (!p) return;
    p.replaceChildren();
    if (c.own_photo) { p.textContent = 'No market price for your own photo. Set your own value.'; }
    else if ((c.value || 0) <= 0) { p.textContent = 'No market price found for this card. Set your own price.'; }
    else if (c.is_graded) { p.textContent = `Graded card: set your own price. Ungraded Near Mint is ₱${Math.round(c.value)}.`; }
    else if (c.is_sealed) { p.textContent = `Sealed: set your own price. Ungraded Near Mint is ₱${Math.round(c.value)}.`; }
    else if (c.suggested) {
      if ((c.value_own || 0) > 0) {
        p.textContent = `You set ₱${Math.round(c.value_own)}. Suggested for ${c.cond}: ₱${Math.round(c.suggested)} `;
      } else {
        p.textContent = `Suggested ₱${Math.round(c.suggested)} · ${c.cond} · ${Math.round((c.share || 1) * 100)}% of Near Mint ₱${Math.round(c.value)} `;
      }
      const btn = document.createElement('button'); btn.type = 'button'; btn.className = 'me-use-btn';
      btn.textContent = `Use ₱${Math.round(c.suggested)}`;
      btn.dataset.use = Math.round(c.suggested);
      p.append(btn);
    }
  }

  function tile(c) {
    const li = document.createElement('li'); li.className = 'me-c'; li.dataset.id = c.id; li.dataset.card = c.source || '';
    const pic = document.createElement('span'); pic.className = 'me-c-pic';
    if (c.image) { const i = document.createElement('img'); i.src = c.image; i.width = 245; i.height = 342; i.alt = ''; i.draggable = false; pic.append(i); }
    const star = document.createElement('button'); star.type = 'button'; star.className = 'me-star'; star.dataset.nodrag = ''; star.dataset.star = ''; star.textContent = '★'; star.setAttribute('aria-label', `Highlight ${c.title || ''}`);
    const x = document.createElement('button'); x.type = 'button'; x.className = 'me-x'; x.dataset.nodrag = ''; x.dataset.remove = ''; x.textContent = '✕'; x.setAttribute('aria-label', `Take ${c.title || ''} out`);
    pic.append(star, x);
    const t = document.createElement('b'); t.className = 'me-c-t'; t.textContent = c.title || '';
    const s = document.createElement('small'); s.className = 'me-c-s'; s.textContent = [c.set, c.number].filter(Boolean).join(' · ');
    const sel = document.createElement('select'); sel.dataset.nodrag = ''; sel.dataset.cond = ''; sel.setAttribute('aria-label', `Condition of ${c.title || ''}`);
    conds.forEach((o) => sel.append(new Option(o, o, o === c.cond, o === c.cond)));
    const val = document.createElement('label'); val.className = 'me-val'; val.dataset.nodrag = '';
    const vl = document.createElement('span'); vl.textContent = 'Value ₱';
    const vi = document.createElement('input'); vi.type = 'number'; vi.min = '0'; vi.step = '1'; vi.inputMode = 'numeric'; vi.dataset.value = ''; vi.placeholder = c.value && c.value !== '₱0' ? c.value : 'Not known yet'; vi.setAttribute('aria-label', `Value of ${c.title || ''} in pesos`);
    if (c.value_own > 0) vi.value = Math.round(c.value_own);
    val.append(vl, vi);
    const sugP = document.createElement('p'); sugP.className = 'me-c-sug';
    renderSugMe(c, sugP);
    li.append(pic, t, s, sel, val, sugP);
    return li;
  }
  binder.addEventListener('click', async (e) => {
    const li = e.target.closest('.me-c'); if (!li) return;
    if (e.target.closest('[data-star]')) {
      try { const j = await send(`/me/cards/${li.dataset.id}`, { action: 'highlight' }); highlight = j.highlight; paint(); say('Highlighted ★'); } catch (err) { say(err.message, true); }
    } else if (e.target.closest('[data-remove]')) {
      try {
        const j = await send(`/me/cards/${li.dataset.id}`, { action: 'remove' }); worth(j);
        const source = li.dataset.card; li.remove(); highlight = j.highlight; paint();
        if (source) document.querySelector(`.me-res[data-id="${CSS.escape(source)}"]`)?.classList.remove('in');
      } catch (err) { say(err.message, true); }
    } else if (e.target.closest('.me-use-btn')) {
      const btn = e.target.closest('.me-use-btn');
      const val = btn.dataset.use;
      const inp = li.querySelector('[data-value]');
      try {
        const j = await send(`/me/cards/${li.dataset.id}`, { action: 'value', value: Number(val) || 0 });
        worth(j); if (inp) inp.value = val;
        renderSugMe(j, li.querySelector('.me-c-sug'));
        say('Saved ✓');
      } catch (err) { say(err.message, true); }
    }
  });
  const worth = (j) => { const w = document.getElementById('meWorth'); if (w && j.worth) { w.textContent = j.worth; w.hidden = j.worth === '₱0'; } };
  binder.addEventListener('change', async (e) => {
    const box = e.target.closest('[data-value]');
    if (box) {
      try {
        const j = await send(`/me/cards/${box.closest('.me-c').dataset.id}`, { action: 'value', value: Number(box.value) || 0 });
        worth(j); if (j.card_market && j.card_market !== '₱0') box.placeholder = j.card_market; 
        renderSugMe(j, box.closest('.me-c').querySelector('.me-c-sug'));
        say('Saved ✓');
      } catch (err) { say(err.message, true); }
      return;
    }
    const sel = e.target.closest('[data-cond]'); if (!sel) return;
    try {
      const li = sel.closest('.me-c');
      const j = await send(`/me/cards/${li.dataset.id}`, { action: 'condition', cond: sel.value });
      worth(j); const vb = li.querySelector('[data-value]'); if (vb && j.card_market && j.card_market !== '₱0') vb.placeholder = j.card_market; 
      
      const vOwn = j.value_own || 0;
      if (j.suggested && !j.is_graded && !j.is_sealed && vOwn === 0 && vb) {
        // Auto-fill value
        const newV = Math.round(j.suggested);
        vb.value = String(newV);
        const j2 = await send(`/me/cards/${li.dataset.id}`, { action: 'value', value: newV });
        worth(j2); renderSugMe(j2, li.querySelector('.me-c-sug'));
      } else {
        renderSugMe(j, li.querySelector('.me-c-sug'));
      }
      say('Saved ✓');
    } catch (err) { say(err.message, true); }
  });
  if (window.dragSort) dragSort(binder, {
    item: '.me-c', key: (n) => n.dataset.id,
    onDrop: async ([ids]) => { try { await send('/me/cards/order', { ids: ids.map(Number) }); say('Order saved ✓'); } catch (err) { say(err.message, true); } },
  });

  /* ---------- finding cards ---------- */
  let lang = 'en', timer, seq = 0;
  const q = $('meQ'), results = $('meResults'), state = $('meState');
  $('meLang').addEventListener('click', (e) => {
    const b = e.target.closest('button'); if (!b) return;
    lang = b.dataset.lang;
    $('meLang').querySelectorAll('button').forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
    search();
  });
  q.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(search, 320); });
  q.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); clearTimeout(timer); search(); q.blur(); } });
  async function search() {
    const text = q.value.trim();
    const mine = ++seq;
    if (text.length < 2) { results.replaceChildren(); state.textContent = 'Type a name and tap the card you own.'; return; }
    state.textContent = 'Searching…';
    try {
      const r = await fetch(`/me/search?q=${encodeURIComponent(text)}&lang=${lang}`, { credentials: 'same-origin', headers: { Accept: 'application/json' } });
      const j = await r.json(); if (mine !== seq) return;
      const cards = j.cards || [];
      state.textContent = j.error && !cards.length ? j.error : cards.length ? `${cards.length} found. Tap the one you own.` : `No cards called “${text}”. Check the spelling.`;
      results.replaceChildren(...cards.slice(0, 120).map(result));
      paint();
    } catch (_) { if (mine === seq) state.textContent = 'Couldn’t search just now. Check your connection.'; }
  }
  function result(c) {
    const b = document.createElement('button'); b.type = 'button'; b.className = 'me-res'; b.dataset.id = c.id;
    const pic = document.createElement('span'); pic.className = 'me-res-pic';
    if (c.image) { const i = document.createElement('img'); i.src = c.image; i.alt = ''; i.loading = 'lazy'; i.width = 245; i.height = 342; i.onerror = () => i.remove(); pic.append(i); }
    const t = document.createElement('b'); t.textContent = c.name;
    const s = document.createElement('small'); s.textContent = [c.en, c.set, c.number].filter(Boolean).join(' · ');
    b.append(pic, t, s);
    b.addEventListener('click', async () => {
      if (b.classList.contains('in') || b.disabled) return;
      b.classList.add('busy');
      try {
        const j = await send('/me/cards', { id: c.id, image: c.image, image_large: c.image_large, en: c.en }); worth(j);
        binder.append(tile({ ...j.card, source: c.id }));
        if (j.card.highlight) highlight = j.card.id;
        b.classList.add('in'); paint(); say('Added ✓');
      } catch (err) { say(err.message, true); }
      b.classList.remove('busy');
    });
    return b;
  }

  /* ---------- a card from your own photo ---------- */
  const up = $('meUpload');
  if (up) {
    const file = $('meUpPhoto'), shot = $('meUpShot'), hint = $('meUpHint');
    file.addEventListener('change', () => {
      const f = file.files[0]; if (!f) return;
      shot.src = URL.createObjectURL(f); shot.hidden = false; hint.hidden = true;
    });
    up.addEventListener('submit', async (e) => {
      e.preventDefault();
      const f = file.files[0];
      if (!f) { say('Add a photo of the card first.', true); return; }
      const btn = up.querySelector('button[type=submit]');
      btn.disabled = true; btn.textContent = 'Adding…';
      try {
        const fd = new FormData(up);
        fd.set('photo', await shrink(f, 1600), 'card.jpg');
        const j = await send('/me/cards/upload', fd); worth(j);
        binder.append(tile(j.card));
        if (j.card.highlight) highlight = j.card.id;
        paint(); say('Added ✓');
        up.reset(); shot.hidden = true; hint.hidden = false;
      } catch (err) { say(err.message, true); }
      btn.disabled = false; btn.textContent = 'Add to my binder';
    });
  }

  /* ---------- account ---------- */
  const pass = $('mePass');
  pass.addEventListener('submit', async (e) => {
    e.preventDefault();
    try { await send('/me/password', { current: pass.elements.current.value, new: pass.elements.new.value }); pass.reset(); say('Password changed ✓'); } catch (err) { say(err.message, true); }
  });
  $('meDelete').addEventListener('click', async () => {
    if (!confirm('Delete your trainer, binder and vouch for good? This can’t be undone.')) return;
    try { await send('/me/delete'); location.href = '/trainers'; } catch (err) { say(err.message, true); }
  });

  paint();
})();
