/* Add cards (/me/add; src/store.rs add_page): the one place a card is added (the owner, 18 Nov 2026: "keep the add a
   card tool in a dedicated single page"). Four ways, chosen at the top:
   - Search: the full card list by name (GET /me/search), in English, Japanese or Chinese.
   - Scan a card (sellers): static/scan.js reads a photo; its tiles open "Add to your store".
   - By set: pick a set (GET /me/sets/search), then tap its cards (GET /me/sets/cards).
   - Not in the list: a card from your own photo (POST /me/cards/upload).
   A seller's card goes through "Add to your store" (add-card.js: condition, price, picture, how many): nothing is added
   or priced by itself. A collector's card joins their binder straight away (POST /me/cards).
   From a seller's binder (?binder=id) the store's own cards come first (each opens the window filled in and goes into
   the binder), and every new card goes into that binder too. "Added just now" lists what this visit added. */
(() => {
  const root = document.getElementById('addPage');
  if (!root) return;
  const $ = (id) => document.getElementById(id);
  const seller = root.dataset.seller === 'true';
  const binderId = root.dataset.binder ? Number(root.dataset.binder) : null;
  const make = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const toast = (msg, bad) => { if (window.Admin && window.Admin.toast) window.Admin.toast(msg, { kind: bad ? 'err' : 'ok' }); };
  const toLogin = () => { location.href = '/login?next=' + encodeURIComponent(location.pathname + location.search); };
  async function getJSON(url) {
    let r;
    try { r = await fetch(url, { credentials: 'same-origin', headers: { Accept: 'application/json' } }); } catch (_) { throw new Error('No connection. Please try again.'); }
    const j = await r.json().catch(() => ({}));
    if (r.status === 401) { toLogin(); throw new Error('Please log in again.'); }
    if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
    return j;
  }
  async function send(url, body) {
    const opts = { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' } };
    if (body instanceof FormData) opts.body = body; else { opts.headers['Content-Type'] = 'application/json'; opts.body = JSON.stringify(body ?? {}); }
    let r;
    try { r = await fetch(url, opts); } catch (_) { throw new Error('No connection. Please try again.'); }
    const j = await r.json().catch(() => ({}));
    if (r.status === 401) { toLogin(); throw new Error('Please log in again.'); }
    if (!r.ok) throw new Error(j.error || 'That didn’t save. Please try again.');
    return j;
  }
  // Phone photos are huge: smaller here first, so they go up quickly on mobile data.
  async function shrink(file, max = 1600) {
    const url = URL.createObjectURL(file);
    try {
      const img = new Image(); img.src = url; await img.decode();
      const k = Math.min(1, max / Math.max(img.naturalWidth, img.naturalHeight));
      const c = document.createElement('canvas'); c.width = Math.round(img.naturalWidth * k); c.height = Math.round(img.naturalHeight * k);
      c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
      return await new Promise((res) => c.toBlob(res, 'image/jpeg', 0.9));
    } catch (_) { return file; } finally { URL.revokeObjectURL(url); }
  }
  const numberOf = (c) => (c.number ? (/^\d+$/.test(String(c.number)) && c.total ? `${c.number}/${c.total}` : String(c.number)) : '');

  /* ---------- added just now ---------- */
  const addedBox = $('adAdded'), addedRow = $('adAddedRow'), addedN = $('adAddedN');
  let addedCount = 0;
  function added(card) {
    if (!card) return;
    addedCount += 1;
    const a = make('div', 'ad-added-c');
    const pic = make('span', 'ad-added-pic');
    if (card.image) { const i = make('img'); i.src = card.image; i.alt = ''; i.loading = 'lazy'; i.onerror = () => i.remove(); pic.append(i); }
    a.append(pic, make('small', null, card.title || card.name || 'Card'));
    addedRow.prepend(a);
    addedBox.hidden = false;
    addedN.textContent = String(addedCount);
  }
  // Every card "Add to your store" adds, whichever way it came (add-card.js says so).
  document.addEventListener('bbv:added', (e) => added(e.detail));

  /* ---------- a card from the card list: tapped ---------- */
  async function take(c, b) {
    if (b.classList.contains('in') || b.classList.contains('busy')) return;
    if (seller) {
      if (!window.bbvAddCard) { toast('Please reload the page and try again.', true); return; }
      window.bbvAddCard.open({ card: c, binderId, onAdded: () => { b.classList.add('in'); } });
      return;
    }
    b.classList.add('busy');
    try {
      const j = await send('/me/cards', { id: c.id, image: c.image, image_large: c.image_large, en: c.en });
      b.classList.add('in');
      added(j.card);
      toast(`${c.name || 'Card'} is in your binder ✓`);
    } catch (err) { toast(err.message, true); }
    b.classList.remove('busy');
  }
  function tile(c) {
    const b = make('button', 'me-res'); b.type = 'button';
    b.cardData = c;
    const pic = make('span', 'me-res-pic');
    if (c.image) { const i = make('img'); i.src = c.image; i.alt = ''; i.loading = 'lazy'; i.decoding = 'async'; i.width = 245; i.height = 342; i.onerror = () => i.remove(); pic.append(i); }
    const sub = [c.en && c.en !== c.name ? c.en : '', c.edition || '', c.set || '', numberOf(c)].filter(Boolean).join(' · ');
    b.append(pic, make('b', null, c.name || 'Card'));
    if (sub) b.append(make('small', null, sub));
    b.setAttribute('aria-label', `Add ${c.name || 'this card'}${sub ? `, ${sub}` : ''}`);
    b.addEventListener('click', () => take(c, b));
    return b;
  }

  /* ---------- the four ways ---------- */
  const ways = [...root.querySelectorAll('[data-way]')];
  const panes = [...root.querySelectorAll('[data-pane]')];
  const langBox = $('adLang');
  let setsShown = false;
  function choose(way, focus) {
    if (!ways.some((b) => b.dataset.way === way)) way = 'search';
    ways.forEach((b) => { const on = b.dataset.way === way; b.classList.toggle('on', on); b.setAttribute('aria-selected', String(on)); b.tabIndex = on ? 0 : -1; });
    panes.forEach((p) => { p.hidden = p.dataset.pane !== way; });
    langBox.hidden = !(way === 'search' || way === 'set');
    // The address says which way is open, so Back and a shared link come back to it.
    try { history.replaceState(history.state, '', location.pathname + location.search + (way === 'search' ? '' : `#${way}`)); } catch (_) {}
    if (way === 'set' && !setsShown) { setsShown = true; searchSets(); }
    if (focus === 'key') ways.find((b) => b.dataset.way === way)?.focus();
  }
  ways.forEach((b) => b.addEventListener('click', () => choose(b.dataset.way)));
  // The arrow keys move between the four, as tabs do.
  root.querySelector('.ad-ways')?.addEventListener('keydown', (e) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
    const i = ways.findIndex((b) => b.classList.contains('on'));
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? ways.length - 1 : (i + (e.key === 'ArrowRight' ? 1 : -1) + ways.length) % ways.length;
    e.preventDefault(); choose(ways[next].dataset.way, 'key');
  });

  /* ---------- card language ---------- */
  const LANGS = ['en', 'ja', 'zh'];
  let lang = 'en';
  try { const saved = localStorage.getItem('bbv:lang'); if (LANGS.includes(saved)) lang = saved; } catch (_) {}
  const paintLang = () => langBox.querySelectorAll('button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.lang === lang)));
  langBox.addEventListener('click', (e) => {
    const b = e.target.closest('button[data-lang]'); if (!b || b.dataset.lang === lang) return;
    lang = b.dataset.lang;
    try { localStorage.setItem('bbv:lang', lang); } catch (_) {}
    paintLang();
    if (panes.find((p) => p.dataset.pane === 'set')?.hidden === false) { closeSet(false); searchSets(); } else search();
  });

  /* ---------- Search ---------- */
  const q = $('adQ'), results = $('adResults'), state = $('adState');
  let seq = 0, timer = 0;
  q.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(search, 320); });
  q.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); clearTimeout(timer); search(); q.blur(); } });
  async function search() {
    const text = q.value.trim(), mine = ++seq;
    if (text.length < 2) { results.replaceChildren(); state.textContent = 'Type a name and tap your card.'; return; }
    state.textContent = 'Searching…';
    try {
      const j = await getJSON(`/me/search?q=${encodeURIComponent(text)}&lang=${lang}`);
      if (mine !== seq) return;
      const cards = j.cards || [];
      const more = (j.total ?? cards.length) > cards.length ? ' Showing the first 120: type more of the name, or the card number.' : '';
      state.textContent = j.error && !cards.length ? j.error : cards.length ? `${j.total ?? cards.length} found. Tap yours.${more}` : `No cards called “${text}”. Check the spelling, or try By set.`;
      results.replaceChildren(...cards.slice(0, 120).map(tile));
    } catch (err) { if (mine === seq) state.textContent = err.message; }
  }

  /* ---------- By set ---------- */
  const setQ = $('adSetQ'), setsBox = $('adSets'), setCards = $('adSetCards'), setState = $('adSetState'), head = $('adSetHead');
  const setLogo = $('adSetLogo'), setName = $('adSetName'), setMeta = $('adSetMeta');
  const SET_HINT = setQ.placeholder;
  let openSet = null, setSeq = 0, setTimer = 0, setQuery = '';
  setQ.addEventListener('input', () => { clearTimeout(setTimer); setTimer = setTimeout(() => (openSet ? filterSet() : searchSets()), openSet ? 120 : 320); });
  setQ.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); clearTimeout(setTimer); if (openSet) filterSet(); else searchSets(); setQ.blur(); } });
  async function searchSets() {
    const text = setQ.value.trim(), mine = ++setSeq;
    setState.textContent = 'Looking for sets…';
    try {
      const j = await getJSON(`/me/sets/search?q=${encodeURIComponent(text)}&lang=${lang}`);
      if (mine !== setSeq) return;
      const sets = j.sets || [];
      if (j.ready === false) { setState.textContent = 'The set list isn’t answering right now. Try again in a moment.'; setsBox.replaceChildren(); return; }
      setState.textContent = text ? (sets.length ? `${sets.length} set${sets.length === 1 ? '' : 's'} match “${text}”. Tap one.` : `No set called “${text}”. Try part of the name, like “151”.`) : `${sets.length} sets, newest first. Tap one.`;
      setsBox.replaceChildren(...sets.map(setTile));
    } catch (err) { if (mine === setSeq) setState.textContent = err.message; }
  }
  function setTile(s) {
    const b = make('button', 'qa-set'); b.type = 'button';
    const pic = make('span', 'qa-set-logo');
    if (s.logo) { const i = make('img'); i.src = s.logo; i.alt = ''; i.loading = 'lazy'; i.onerror = () => { i.remove(); pic.textContent = s.name; }; pic.append(i); } else pic.textContent = s.name;
    b.append(pic, make('b', null, s.name));
    if (s.en_name) b.append(make('span', 'qa-set-en', `English: ${s.en_name}`));
    b.append(make('small', null, `${s.count} cards`));
    b.addEventListener('click', () => showSet(s));
    return b;
  }
  async function showSet(s) {
    const mine = ++setSeq;
    if (!openSet) setQuery = setQ.value;
    openSet = { id: s.id };
    setsBox.hidden = true; head.hidden = false; setCards.replaceChildren();
    setQ.value = ''; setQ.placeholder = 'Find a card in this set: its name or number';
    setName.textContent = s.en_name ? `${s.name} · ${s.en_name}` : s.name; setMeta.textContent = 'Loading…'; setLogo.hidden = true;
    setState.textContent = 'Loading every card in this set…';
    head.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    try {
      const cat = s.cat || (lang === 'zh' ? 'zh-tw' : lang);
      const j = await getJSON(`/me/sets/cards?id=${encodeURIComponent(s.id)}&lang=${encodeURIComponent(cat)}`);
      if (mine !== setSeq) return;
      const cards = j.cards || [], set = j.set || {};
      if (!cards.length) { setMeta.textContent = ''; setState.textContent = j.error ? 'The card list isn’t answering right now. Try again in a moment.' : 'No cards listed for this set.'; return; }
      if (set.name) setName.textContent = set.en_name ? `${set.name} · ${set.en_name}` : set.name;
      setMeta.textContent = `${cards.length} card${cards.length === 1 ? '' : 's'}`;
      if (set.logo) { setLogo.src = set.logo; setLogo.hidden = false; setLogo.onerror = () => { setLogo.hidden = true; }; }
      setCards.replaceChildren(...cards.map((c) => tile({ ...c, set: c.set || set.name || s.name })));
      setState.textContent = 'Tap your card.';
    } catch (err) { if (mine === setSeq) { setMeta.textContent = ''; setState.textContent = err.message; } }
  }
  function filterSet() {
    const text = setQ.value.trim().toLowerCase().replace(/^#/, ''), num = text.replace(/^0+/, '');
    let n = 0;
    setCards.querySelectorAll('.me-res').forEach((b) => {
      const c = b.cardData || {};
      const hit = !text || String(c.name || '').toLowerCase().includes(text) || String(c.en || '').toLowerCase().includes(text) || String(c.edition || '').toLowerCase().includes(text) || String(c.number || '').toLowerCase().replace(/^0+/, '') === num;
      b.hidden = !hit; if (hit) n += 1;
    });
    setState.textContent = text ? `${n} card${n === 1 ? '' : 's'} match “${setQ.value.trim()}”.` : 'Tap your card.';
  }
  function closeSet(search) {
    ++setSeq; openSet = null; head.hidden = true; setsBox.hidden = false; setCards.replaceChildren();
    setQ.placeholder = SET_HINT; setQ.value = setQuery;
    if (search) searchSets();
  }
  $('adSetBack').addEventListener('click', () => { closeSet(true); setQ.focus({ preventScroll: true }); });

  /* ---------- Not in the list: your own photo ---------- */
  const own = $('ownForm');
  if (own) {
    const file = $('ownPhoto'), shot = $('ownShot'), hint = $('ownHint');
    file.addEventListener('change', () => { const f = file.files[0]; if (!f) return; shot.src = URL.createObjectURL(f); shot.hidden = false; hint.hidden = true; });
    own.addEventListener('submit', async (e) => {
      e.preventDefault();
      const f = file.files[0];
      if (!f) { toast('Add a photo of the card first.', true); return; }
      const btn = own.querySelector('button[type=submit]'), was = btn.textContent;
      btn.disabled = true; btn.textContent = 'Adding…';
      try {
        const fd = new FormData(own);
        fd.set('photo', await shrink(f, 1600), 'card.jpg');
        const j = await send('/me/cards/upload', fd);
        if (seller && fd.get('flag')) await send(`/me/listings/${j.card.id}`, { flag: true });
        if (seller && binderId) await send('/me/listings/move', { ids: [j.card.id], binder_id: binderId });
        added(j.card);
        toast(seller ? (binderId ? 'Added to your store and this binder ✓' : 'Added To Your Store ✓') : 'Added To Your Binder ✓');
        own.reset(); shot.hidden = true; hint.hidden = false;
      } catch (err) { toast(err.message, true); }
      btn.disabled = false; btn.textContent = was;
    });
  }

  /* ---------- from a binder: the store's own cards first ---------- */
  const mineBox = $('adMine');
  if (mineBox && binderId) {
    const mq = $('adMineQ'), list = $('adMineList'), mstate = $('adMineState');
    let mseq = 0, mt = 0;
    const mineTile = (c) => {
      const b = make('button', 'me-res'); b.type = 'button';
      const pic = make('span', 'me-res-pic');
      if (c.image) { const i = make('img'); i.src = c.image; i.alt = ''; i.loading = 'lazy'; i.onerror = () => i.remove(); pic.append(i); }
      b.append(pic, make('b', null, c.title || 'Card'));
      const sub = [c.sub, c.price].filter(Boolean).join(' · ');
      if (sub) b.append(make('small', null, sub));
      if (c.inside) b.append(make('small', 'ad-moves', `In “${c.inside}”: Moves Here`));
      b.addEventListener('click', async () => {
        if (b.classList.contains('in') || !window.bbvAddCard) return;
        b.classList.add('busy');
        try {
          const j = await getJSON(`/me/listings/${encodeURIComponent(c.id)}`);
          window.bbvAddCard.open({ listing: j.card, binderId, onAdded: () => { b.classList.add('in'); } });
        } catch (err) { toast(err.message, true); }
        b.classList.remove('busy');
      });
      return b;
    };
    async function mine() {
      const text = mq.value.trim(), my = ++mseq;
      mstate.textContent = 'Looking…';
      try {
        const j = await getJSON(`/me/binders/${binderId}/choices?q=${encodeURIComponent(text)}`);
        if (my !== mseq) return;
        const rows = j.cards || [];
        mstate.textContent = rows.length ? 'Tap a card to check it: it goes into this binder.' : text ? `None of your other cards match “${text}”.` : 'Every card in your store is in this binder already.';
        list.replaceChildren(...rows.map(mineTile));
      } catch (err) { if (my === mseq) mstate.textContent = err.message; }
    }
    mq.addEventListener('input', () => { clearTimeout(mt); mt = setTimeout(mine, 250); });
    mq.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); clearTimeout(mt); mine(); } });
    mine();
  }

  /* ---------- start ---------- */
  paintLang();
  choose((location.hash || '').replace('#', '') || 'search');
})();
