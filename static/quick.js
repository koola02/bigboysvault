/* Quick add and Add by set: find cards by picture (by name, or by opening a set), tap into one shared tray, save the whole tray in one go.
   The tray and binders survive a refresh (localStorage). Prices load when a card is tapped.
   Every copy in the tray is its own row with its own finish, price and condition, so copies of one card can all be different. */
(() => {
  const root = document.getElementById('qa'); if (!root || root.dataset.ready) return; root.dataset.ready = '1';
  const $ = (id) => document.getElementById(id);
  const A = window.Admin || { toast: () => {} };
  const peso = (n) => '₱' + Math.round(n || 0).toLocaleString('en-US');
  const KEY = 'bbv:quick:v1';
  const el = { q: $('qaQ'), state: $('qaState'), grid: $('qaGrid'), more: $('qaMore'), bar: $('qaBar'), thumbs: $('qaThumbs'), count: $('qaCount'), value: $('qaValue'),
    open: $('qaOpen'), sheet: $('qaSheet'), list: $('qaList'), binders: $('qaBinders'), save: $('qaSave'), prog: $('qaProg'), total: $('qaTotal') };

  /* ---------- state ---------- */
  const NEAR_MINT = 'Near Mint';
  // Tray item: { id, name, en, number, total, set, image, image_large, rarity, finishes, priced, copies: [{ finish, price, market, cond, err }] }
  let st = { tray: [], binders: [] };
  try {
    const s = JSON.parse(localStorage.getItem(KEY) || 'null');
    if (s && Array.isArray(s.tray)) {
      // A tray saved by the earlier version: one row per card with a count. Open it out, joining rows of the same card.
      const merged = [];
      for (const t of s.tray) {
        const copies = t.copies || Array.from({ length: t.qty || 1 }, () => ({ finish: t.finish || '', price: t.price ?? '', market: !!t.market, cond: t.cond || s.cond || NEAR_MINT }));
        const same = merged.find((m) => m.id === t.id);
        if (same) same.copies.push(...copies); else { const { qty, finish, price, market, cond, err, ...rest } = t; merged.push({ ...rest, copies }); }
      }
      st = { tray: merged, binders: s.binders || [] };
    }
  } catch (_) {}
  const persist = () => { try { localStorage.setItem(KEY, JSON.stringify(st)); } catch (_) {} };
  const find = (id) => st.tray.find((t) => t.id === id);
  const newCopy = (it) => {
    const f = (it.edFinish && it.finishes.find((x) => x.key === it.edFinish)) || (it.edFinish ? null : it.finishes[0]);
    // A new copy starts as Near Mint, at the suggested price for its finish.
    const price = f ? (A.suggest(f.php, NEAR_MINT) ?? Math.round(f.php)) : '';
    return { finish: f ? f.key : (it.edFinish || ''), price, market: !!f, cond: NEAR_MINT };
  };
  const shown = new Map();          // id -> card from the last search
  const priceCache = new Map();     // id -> Promise<match|null>

  /* ---------- search ---------- */
  const setMode = root.dataset.mode === 'set';
  let seq = 0, t, openSet = null, setQuery = '';   // set mode: the set being shown, and the set search to return to
  const onType = () => (setMode ? (openSet ? filterSet() : searchSets()) : search());
  el.q.addEventListener('input', () => { clearTimeout(t); t = setTimeout(onType, openSet ? 120 : 320); });
  el.q.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); clearTimeout(t); onType(); el.q.blur(); } });
  const getJSON = (url) => fetch(url, { headers: { Accept: 'application/json' }, credentials: 'same-origin' }).then((r) => r.json());

  /* ---------- card language: English by default; Japanese and Chinese on request ---------- */
  const LANGS = { en: 'English', ja: 'Japanese', zh: 'Chinese' };   // Chinese covers both the Traditional and Simplified prints
  const qs0 = new URLSearchParams(location.search);
  let lang = qs0.get('lang') || (() => { try { return localStorage.getItem('bbv:lang'); } catch (_) { return null; } })() || 'en';
  if (lang === 'zh-tw' || lang === 'zh-cn') lang = 'zh';
  if (!LANGS[lang]) lang = 'en';
  const langBox = $('qaLang');
  const placeholder = () => (setMode ? (openSet ? 'Filter this set by name (English works) or number' : (lang === 'en' ? 'Set name, e.g. 30th, Ascended Heroes, Pitch Black' : 'Set name in English, e.g. Surging Sparks, or a code like SV8'))
    : (lang === 'en' ? 'Card name, e.g. Slaking or Pikachu 57' : 'Card name in English, e.g. Snorlax or Ultra Ball'));
  function paintLang() {
    langBox.querySelectorAll('button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.l === lang)));
    el.q.placeholder = placeholder();
  }
  langBox.addEventListener('click', (e) => {
    const b = e.target.closest('button[data-l]'); if (!b || b.dataset.l === lang) return;
    lang = b.dataset.l; try { localStorage.setItem('bbv:lang', lang); } catch (_) {}
    paintLang();
    if (setMode) { if (openSet) backToSets(true); else searchSets(); } else search();
  });
  const L = () => '&lang=' + encodeURIComponent(lang);

  /* ---------- add by set: search sets, open one, tap its cards ---------- */
  const S = setMode ? { list: $('qaSets'), head: $('qaSetHead'), back: $('qaBack'), logo: $('qaSetLogo'), name: $('qaSetName'), meta: $('qaSetMeta') } : null;
  // A vintage set comes in printings: big buttons to see just one kind (1st Edition, Unlimited...).
  const ED_KINDS = [['', 'Every printing'], ['first', '1st Edition'], ['unlimited', 'Unlimited'], ['shadowless', 'Shadowless'], ['other', 'Other prints']];
  const edKind = (e) => (!e ? '' : /^1st/.test(e) ? 'first' : e === 'Unlimited' ? 'unlimited' : /^Shadowless/.test(e) ? 'shadowless' : 'other');
  let edPick = '';
  let edBar = null;
  function paintEdBar(cards) {
    edBar?.remove(); edBar = null; edPick = '';
    if (!cards.some((c) => c.edition)) return;
    edBar = document.createElement('div'); edBar.className = 'qa-eds'; edBar.setAttribute('role', 'group'); edBar.setAttribute('aria-label', 'Show one printing');
    for (const [k, label] of ED_KINDS) {
      const n = k ? cards.filter((c) => edKind(c.edition) === k).length : cards.length;
      if (!n) continue;
      const b = document.createElement('button'); b.type = 'button'; b.dataset.k = k; b.setAttribute('aria-pressed', String(k === ''));
      b.innerHTML = '<span></span><small></small>'; b.firstChild.textContent = label; b.lastChild.textContent = n;
      b.addEventListener('click', () => { edPick = k; edBar.querySelectorAll('button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); filterSet(); });
      edBar.append(b);
    }
    S.meta.after(edBar);
  }
  async function searchSets() {
    const q = el.q.value.trim(), my = ++seq;
    el.state.textContent = 'Looking for sets…';
    try {
      const j = await getJSON('/admin/sets/search?q=' + encodeURIComponent(q) + L()); if (my !== seq) return;
      const sets = j.sets || [];
      if (!j.ready) { el.state.textContent = 'The set list isn’t answering right now. Try again in a moment.'; S.list.replaceChildren(); return; }
      el.state.textContent = q ? (sets.length ? `${sets.length} set${sets.length === 1 ? '' : 's'} match “${q}”. Tap one to see every card in it.` : `No set called “${q}”. Try part of the name, like “151” or “Crown”.`) : (lang === 'en' ? `All ${sets.length} sets, newest first. Type part of a name to narrow it down.` : `All ${sets.length} ${LANGS[lang]} sets, newest first. Type a set name in English (e.g. Surging Sparks) or a code like SV8.`);
      S.list.replaceChildren(...sets.map(setTile));
    } catch (_) { if (my === seq) el.state.textContent = 'Couldn’t load sets just now. Check your connection.'; }
  }
  function setTile(s) {
    const b = document.createElement('button'); b.type = 'button'; b.className = 'qa-set';
    const pic = document.createElement('span'); pic.className = 'qa-set-logo';
    if (s.logo) { const i = document.createElement('img'); i.src = s.logo; i.alt = ''; i.loading = 'lazy'; i.onerror = () => { i.remove(); pic.textContent = s.name; }; pic.append(i); } else pic.textContent = s.name;
    const t1 = document.createElement('b'); t1.textContent = s.name;
    const t2 = document.createElement('small'); t2.textContent = `${s.count} cards · ${s.id.toUpperCase()}`;
    b.append(pic, t1);
    if (s.en_name) { const t3 = document.createElement('span'); t3.className = 'qa-set-en'; t3.textContent = 'English: ' + s.en_name; b.append(t3); }
    b.append(t2); b.addEventListener('click', () => showSet(s.id, true, s.cat)); return b;
  }
  async function showSet(id, push, cat) {
    cat = cat || (lang === 'zh' ? 'zh-tw' : lang);
    const my = ++seq; if (!openSet) setQuery = el.q.value;
    openSet = { id, cat, cards: [] }; S.list.hidden = true; S.head.hidden = false; el.grid.replaceChildren(); el.q.value = '';
    el.q.placeholder = placeholder(); S.name.textContent = 'Loading…'; S.meta.textContent = ''; S.logo.hidden = true;
    el.state.textContent = 'Loading every card in this set…';
    if (push) history.pushState({ set: id }, '', '/admin/sets?set=' + encodeURIComponent(id) + '&cat=' + encodeURIComponent(cat) + L());
    window.scrollTo({ top: 0 });
    try {
      const j = await getJSON('/admin/sets/cards?id=' + encodeURIComponent(id) + '&lang=' + encodeURIComponent(cat)); if (my !== seq) return;
      const cards = j.cards || [];
      if (!cards.length) { el.state.textContent = j.error ? 'The card list isn’t answering right now. Try again in a moment.' : 'No cards listed for this set.'; S.name.textContent = j.set?.name || id; return; }
      openSet.cards = cards; S.name.textContent = j.set.en_name ? `${j.set.name} · ${j.set.en_name}` : j.set.name;
      const have = cards.filter((c) => c.owned).length;
      const split = cards.some((c) => c.edition);
      S.meta.textContent = split ? `${cards.length} printings of ${j.set.total || new Set(cards.map((c) => c.api)).size} cards (1st Edition, Unlimited and more) · you have ${have}`
        : `${cards.length} cards${j.set.total && j.set.total < cards.length ? ` (${j.set.total} + ${cards.length - j.set.total} secret)` : ''} · you have ${have}`;
      if (j.set.logo) { S.logo.src = j.set.logo; S.logo.hidden = false; S.logo.onerror = () => { S.logo.hidden = true; }; }
      shown.clear(); cards.forEach((c) => shown.set(c.id, c));
      el.grid.replaceChildren(...cards.map(tile));
      paintEdBar(cards);
      el.state.textContent = 'Tap the cards you have. Cards marked “Have” are already in your collection.';
    } catch (_) { if (my === seq) el.state.textContent = 'Couldn’t load this set. Check your connection.'; }
  }
  function filterSet() {
    const q = el.q.value.trim().toLowerCase().replace(/^#/, ''), num = q.replace(/^0+/, '');
    let n = 0;
    el.grid.querySelectorAll('.qa-card').forEach((card) => {
      const c = shown.get(card.dataset.id); if (!c) return;
      const hit = (!q || c.name.toLowerCase().includes(q) || (c.en || '').toLowerCase().includes(q) || (c.edition || '').toLowerCase().includes(q) || String(c.number).toLowerCase().replace(/^0+/, '') === num) && (!edPick || edKind(c.edition) === edPick);
      card.hidden = !hit; if (hit) n += 1;
    });
    el.state.textContent = q ? `${n} card${n === 1 ? '' : 's'} match “${el.q.value.trim()}”.` : edPick ? `${n} card${n === 1 ? '' : 's'} in this printing. Tap the ones you have.` : 'Tap the cards you have. Cards marked “Have” are already in your collection.';
  }
  function backToSets(push) {
    ++seq; openSet = null; S.head.hidden = true; S.list.hidden = false; el.grid.replaceChildren(); shown.clear(); edBar?.remove(); edBar = null; edPick = '';
    el.q.placeholder = placeholder(); el.q.value = setQuery;
    if (push) history.pushState({}, '', '/admin/sets' + (lang === 'en' ? '' : '?lang=' + lang));
    searchSets();
  }
  if (setMode) {
    S.back.addEventListener('click', () => backToSets(true));
    window.addEventListener('popstate', () => { const p = new URLSearchParams(location.search), id = p.get('set'); if (id) showSet(id, false, p.get('cat')); else if (openSet) backToSets(false); });
  }
  async function search() {
    const q = el.q.value.trim();
    if (startEl) startEl.hidden = q.length >= 2;
    if (q.length < 2) { el.grid.replaceChildren(); el.more.hidden = true; el.state.textContent = q ? 'Type at least two letters.' : 'Type a name, or start from one of these:'; return; }
    const my = ++seq; el.state.textContent = 'Searching…'; el.grid.setAttribute('aria-busy', 'true');
    try {
      const j = await getJSON('/admin/catalog?q=' + encodeURIComponent(q) + L()); if (my !== seq) return;
      if (j.error && !j.cards?.length) { el.state.textContent = 'The card list isn’t answering right now. Try again in a moment, or add the card by hand.'; return; }
      const cards = j.cards || [];
      el.state.textContent = cards.length ? `${cards.length} ${lang === 'en' ? '' : LANGS[lang] + ' '}printing${cards.length === 1 ? '' : 's'}. Tap the one you have.`
        : j.hint === 'no-translation' ? `No Pokémon or item called “${q}”. Trainer and supporter names (like Iono) can’t be translated yet: find those through Add by set, or type the ${LANGS[lang]} name.`
        : `No ${lang === 'en' ? '' : LANGS[lang] + ' '}cards called “${q}”. Check the spelling, or add it by hand.`;
      shown.clear(); cards.forEach((c) => shown.set(c.id, c));
      el.grid.replaceChildren(...cards.map(tile));
    } catch (_) { if (my === seq) el.state.textContent = 'Couldn’t search just now. Check your connection.'; }
    finally { if (my === seq) el.grid.removeAttribute('aria-busy'); }
  }
  const no = (c) => c.total ? `#${c.number}/${c.total}` : `#${c.number}`;
  function tile(c) {
    const b = document.createElement('button'); b.type = 'button'; b.className = 'qa-tile'; b.dataset.id = c.id;
    b.setAttribute('aria-label', `${c.name}${c.edition ? ', ' + c.edition : ''}, ${c.set} ${no(c)}. Add to tray`);
    const fig = document.createElement('span'); fig.className = 'qa-pic';
    const im = document.createElement('img'); im.src = c.image; im.alt = ''; im.loading = 'lazy'; im.decoding = 'async'; im.width = 245; im.height = 342;
    im.onerror = () => { im.remove(); fig.classList.add('noimg'); fig.prepend(c.name); };
    if (!c.image) { im.removeAttribute('src'); queueMicrotask(im.onerror); }
    // An Unlimited picture can take a moment the first time it's made: a soft shimmer until it arrives.
    if (String(c.image || '').startsWith('/admin/cardimg/') && !im.complete) { fig.classList.add('wait'); im.addEventListener('load', () => fig.classList.remove('wait'), { once: true }); }
    const n = document.createElement('span'); n.className = 'qa-n'; fig.append(im, n);
    if (c.edition) { const ed = document.createElement('span'); ed.className = 'qa-ed' + (/^1st/.test(c.edition) ? ' first' : ''); ed.textContent = c.edition; fig.append(ed); }
    if (c.owned) { const o = document.createElement('span'); o.className = 'qa-own'; o.textContent = `Have ${c.owned}`; fig.append(o); }
    const cap = document.createElement('span'); cap.className = 'qa-cap';
    cap.innerHTML = '<b></b><small></small>'; cap.firstChild.textContent = c.name; cap.lastChild.textContent = `${c.en ? c.en + ' · ' : ''}${c.set || 'Unknown set'} · ${no(c)}`;
    b.append(fig, cap); b.addEventListener('click', () => add(c, card));
    // − / + under every card, so an accidental tap is undone right where it happened.
    const step = document.createElement('div'); step.className = 'qa-step';
    const minus = document.createElement('button'); minus.type = 'button'; minus.className = 'qa-minus'; minus.textContent = '−'; minus.setAttribute('aria-label', `One fewer ${c.name}`);
    const cnt = document.createElement('span'); cnt.className = 'qa-cnt'; cnt.setAttribute('aria-live', 'polite');
    const plus = document.createElement('button'); plus.type = 'button'; plus.className = 'qa-plus'; plus.textContent = '+'; plus.setAttribute('aria-label', `One more ${c.name}`);
    minus.addEventListener('click', () => remove(c, card)); plus.addEventListener('click', () => add(c, card));
    step.append(minus, cnt, plus);
    const card = document.createElement('div'); card.className = 'qa-card'; card.dataset.id = c.id;
    card.append(b, step); paintTile(card); return card;
  }
  function paintTile(card) {
    const q = find(card.dataset.id)?.copies.length || 0;
    card.classList.toggle('in', q > 0);
    card.querySelector('.qa-n').textContent = q ? `×${q}` : '';
    card.querySelector('.qa-cnt').textContent = q ? `${q} in tray` : 'Add';
    card.querySelector('.qa-minus').disabled = q === 0;
  }
  const paintTiles = () => el.grid.querySelectorAll('.qa-card').forEach(paintTile);

  /* ---------- tray ---------- */
  function add(c, card) {
    let it = find(c.id);
    if (!it) { it = { id: c.id, api: c.api || c.id, edition: c.edition || '', edFinish: c.finish || '', name: c.name, en: c.en || '', number: c.number, total: c.total, set: c.set, image: c.image, image_large: c.image_large, rarity: '', finishes: [], priced: false, copies: [] }; st.tray.push(it); loadPrice(it); }
    it.copies.push(newCopy(it));
    persist(); paintBar(); if (card) { paintTile(card); card.classList.remove('pop'); void card.offsetWidth; card.classList.add('pop'); }
    if (navigator.vibrate) navigator.vibrate(12);
  }
  function remove(c, card) {
    const it = find(c.id); if (!it) return;
    it.copies.pop(); if (!it.copies.length) st.tray = st.tray.filter((x) => x !== it);
    persist(); paintBar(); if (card) paintTile(card);
    if (navigator.vibrate) navigator.vibrate(8);
  }
  function getPrice(id) {
    if (!priceCache.has(id)) priceCache.set(id, fetch('/admin/price?id=' + encodeURIComponent(id), { headers: { Accept: 'application/json' }, credentials: 'same-origin' })
      .then((r) => r.json()).then((j) => j.matches?.[0] || null).catch(() => { priceCache.delete(id); return null; }));
    return priceCache.get(id);
  }
  async function loadPrice(it) {
    const m = await getPrice(it.api || it.id); const cur = it; if (!st.tray.includes(cur)) return;
    cur.priced = true;
    if (m) {
      cur.finishes = (m.finishes || []).map((f) => ({ key: f.key, label: f.label, php: f.php })); cur.source = m.source || ''; cur.updated = m.updated || '';
      if (m.total && !cur.total) cur.total = m.total; if (m.set && !cur.set) cur.set = m.set; if (m.rarity) cur.rarity = m.rarity;
      const first = (cur.edFinish && cur.finishes.find((f) => f.key === cur.edFinish)) || (cur.edFinish ? null : cur.finishes[0]);
      if (first) for (const c of cur.copies) if (!c.finish || (c.finish === first.key && c.price === '')) { c.finish = first.key; if (c.price === '') { c.price = A.suggest(first.php, c.cond) ?? Math.round(first.php); c.market = true; } }
    }
    persist(); paintBar(); if (!el.sheet.hidden) paintList();
  }
  const count = () => st.tray.reduce((n, t) => n + t.copies.length, 0);
  const value = () => st.tray.reduce((n, t) => n + t.copies.reduce((v, c) => v + (Number(c.price) || 0), 0), 0);
  function paintBar() {
    const n = count(); el.bar.hidden = n === 0; document.body.classList.toggle('qa-has-tray', n > 0);
    el.count.textContent = `${n} card${n === 1 ? '' : 's'}`;
    el.value.textContent = st.tray.some((t) => !t.priced) ? `${peso(value())} · pricing…` : peso(value());
    el.thumbs.replaceChildren(...st.tray.filter((t) => t.image).slice(-5).map((t) => { const i = document.createElement('img'); i.src = t.image; i.alt = ''; return i; }));
    el.save.textContent = `Save all ${n} card${n === 1 ? '' : 's'}`;
  }

  /* ---------- review sheet ---------- */
  const CONDS = ['Near Mint', 'LP-NM', 'Lightly Played', 'MP-LP', 'Moderately Played', 'HP-MP', 'Heavily Played', 'Damaged-HP', 'Damaged', 'Sealed', 'PSA 10', 'PSA 9', 'CGC 10', 'CGC 9.5', 'BGS 9.5'];
  // Every finish can be chosen, including ones with no market price to look up.
  const FINISHES = [['normal', 'Normal'], ['holofoil', 'Holofoil'], ['reverseHolofoil', 'Reverse Holo'], ['1stEditionHolofoil', '1st Ed. Holo'], ['1stEditionNormal', '1st Ed.'], ['full-art', 'Full art'], ['illustration', 'Illustration']];
  function paintDefaults() {
    el.binders.querySelectorAll('input').forEach((i) => { i.checked = st.binders.includes(i.value); });
  }
  el.binders.addEventListener('change', () => { st.binders = [...el.binders.querySelectorAll('input:checked')].map((i) => i.value); persist(); });

  const afterChange = () => { persist(); paintBar(); paintTiles(); if (!st.tray.length) closeSheet(); else paintList(); };
  function copyRow(it, c, i) {
    const row = document.createElement('li'); row.className = 'qa-copy' + (c.err ? ' err' : '');
    const n = document.createElement('span'); n.className = 'qa-copy-n'; n.textContent = `Card ${i + 1}`;
    const fin = document.createElement('select'); fin.setAttribute('aria-label', `Finish of card ${i + 1}`);
    if (!c.finish) fin.append(new Option(it.priced ? 'Pick a finish' : 'Pricing…', '', true, true));
    // Each finish shows its suggested price for this copy's condition (graded and sealed: the Near Mint price).
    const finLabel = (f) => `${f.label} · ${peso(A.suggest(f.php, c.cond) ?? f.php)}`;
    it.finishes.forEach((f) => fin.append(new Option(finLabel(f), f.key, false, f.key === c.finish)));
    // A finish the price list already shows under the same name isn't offered twice.
    FINISHES.filter(([k, label]) => !it.finishes.some((f) => f.key === k || (f.label === label && k !== c.finish))).forEach(([k, label]) => fin.append(new Option(label, k, false, k === c.finish)));
    const sug = document.createElement('p'); sug.className = 'qa-sug';
    const pr = document.createElement('input'); pr.className = 'qa-price'; pr.inputMode = 'decimal'; pr.placeholder = '₱0'; pr.value = c.price; pr.setAttribute('aria-label', `Value of card ${i + 1} in pesos`);
    const cond = document.createElement('select'); cond.setAttribute('aria-label', `Condition of card ${i + 1}`);
    const known = CONDS.includes(c.cond) ? CONDS : [c.cond, ...CONDS];
    known.forEach((v) => cond.append(new Option(v, v, false, v === c.cond)));
    const market = () => it.finishes.find((x) => x.key === c.finish);
    const usePrice = (v) => { c.price = String(v); c.market = true; pr.value = c.price; persist(); paintBar(); paintSug(); };
    const paintSug = () => {
      const f = market();
      A.sugLine(sug, { nm: f?.php || 0, cond: c.cond, label: f?.label, source: it.source, updated: it.updated, price: c.price, onUse: usePrice });
      [...fin.options].forEach((o) => { const g = it.finishes.find((x) => x.key === o.value); if (g) o.textContent = finLabel(g); });
    };
    // A price the person typed is never replaced. A suggested one follows the finish and condition.
    const follow = () => {
      const f = market();
      if (!c.market) return;
      if (!f) { c.market = false; return; }
      const v = A.suggest(f.php, c.cond);
      if (v != null) { c.price = String(v); if (document.activeElement !== pr) pr.value = c.price; }
    };
    fin.onchange = () => { c.finish = fin.value; follow(); persist(); paintBar(); paintSug(); };
    pr.oninput = () => { c.price = pr.value.replace(/[^\d.]/g, ''); c.market = false; persist(); paintBar(); paintSug(); };
    cond.onchange = () => { c.cond = cond.value; follow(); persist(); paintBar(); paintSug(); };
    document.addEventListener('bbv:shares', () => { if (row.isConnected) { follow(); paintSug(); } }, { once: true });
    const x = document.createElement('button'); x.type = 'button'; x.className = 'qa-copy-x'; x.textContent = '✕'; x.setAttribute('aria-label', `Take out card ${i + 1}`);
    x.onclick = () => { it.copies.splice(it.copies.indexOf(c), 1); if (!it.copies.length) st.tray = st.tray.filter((t) => t !== it); afterChange(); };
    row.append(n, fin, pr, cond, x, sug);
    if (c.err) { const e = document.createElement('p'); e.className = 'qa-err'; e.textContent = c.err; row.append(e); }
    paintSug();
    return row;
  }
  function paintList() {
    el.list.replaceChildren(...st.tray.map((it) => {
      const li = document.createElement('li'); li.className = 'qa-row' + (it.copies.some((c) => c.err) ? ' err' : '');
      const im = document.createElement('img'); if (it.image) im.src = it.image; im.alt = ''; im.width = 60; im.height = 84;
      const info = document.createElement('div'); info.className = 'qa-info';
      const h = document.createElement('b'); h.textContent = it.name;
      const s = document.createElement('small'); s.textContent = `${it.edition ? it.edition + ' · ' : ''}${it.set || ''} · ${no(it)}`;
      info.append(h, s);
      const q = document.createElement('div'); q.className = 'qa-qty';
      const minus = document.createElement('button'); minus.type = 'button'; minus.textContent = it.copies.length > 1 ? '−' : '✕'; minus.setAttribute('aria-label', it.copies.length > 1 ? 'One fewer' : 'Remove');
      const n = document.createElement('span'); n.textContent = it.copies.length;
      const plus = document.createElement('button'); plus.type = 'button'; plus.textContent = '+'; plus.setAttribute('aria-label', 'One more');
      minus.onclick = () => { it.copies.pop(); if (!it.copies.length) st.tray = st.tray.filter((x) => x !== it); afterChange(); };
      plus.onclick = () => { it.copies.push(newCopy(it)); afterChange(); };
      q.append(minus, n, plus);
      const list = document.createElement('ol'); list.className = 'qa-copies';
      list.append(...it.copies.map((c, i) => copyRow(it, c, i)));
      const acts = document.createElement('div'); acts.className = 'qa-row-acts';
      const all = document.createElement('button'); all.type = 'button'; all.className = 'qa-all'; all.textContent = 'Remove all'; all.setAttribute('aria-label', `Remove all of ${it.name}`);
      all.onclick = () => { st.tray = st.tray.filter((x) => x !== it); afterChange(); };
      acts.append(q, all);
      li.append(im, info, acts, list); return li;
    }));
  }
  let lastFocus = null;
  function openSheet() { lastFocus = document.activeElement; paintDefaults(); paintList(); el.prog.textContent = ''; el.sheet.hidden = false; requestAnimationFrame(() => el.sheet.classList.add('open')); document.body.style.overflow = 'hidden'; el.save.focus({ preventScroll: true }); }
  function closeSheet() { el.sheet.classList.remove('open'); document.body.style.overflow = ''; setTimeout(() => { el.sheet.hidden = true; }, 220); if (lastFocus) lastFocus.focus({ preventScroll: true }); }
  el.open.addEventListener('click', openSheet);
  $('qaClear').addEventListener('click', () => {
    const n = count();
    if (!n || !confirm(`Remove all ${n} card${n === 1 ? '' : 's'} from your tray?`)) return;
    st.tray = []; afterChange(); A.toast('Tray emptied', { kind: 'ok' });
  });
  el.sheet.addEventListener('click', (e) => { if (e.target.closest('[data-close]')) closeSheet(); });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !el.sheet.hidden) closeSheet(); });

  /* ---------- save all ---------- */
  const pad = (n) => (/^\d+$/.test(String(n)) ? String(n).padStart(3, '0') : String(n));
  function form(it, c) {
    const f = new FormData(), num = it.total ? `${pad(it.number)}/${it.total}` : pad(it.number);
    // Japanese / Chinese cards keep their printed name plus the English one, so the site's own search finds them in English.
    const named = (it.en && it.en !== it.name ? `${it.name} (${it.en})` : it.name) + (it.edition ? ` (${it.edition})` : '');
    f.set('title', [named, num, it.set].filter(Boolean).join(' - '));
    f.set('card_no', num); f.set('card_set', it.set || ''); f.set('api_id', it.api || it.id); f.set('finish', c.finish || ''); f.set('rarity', it.rarity || '');
    f.set('price', String(c.price || '')); f.set('price_src', c.market ? 'suggested' : ''); f.set('variant', c.cond || NEAR_MINT);
    const l = /^tcgdex-(ja|zh)/.exec(it.id); if (l) f.set('tags', l[1] === 'ja' ? 'Japanese' : 'Chinese');
    f.set('photo_url', it.image_large || ''); st.binders.forEach((b) => f.append('binder', b));
    return f;
  }
  let saving = false;
  el.save.addEventListener('click', async () => {
    if (saving || !st.tray.length) return;
    if (st.tray.some((t) => !t.priced)) { el.prog.textContent = 'Still pricing a card or two. One second…'; await Promise.all(st.tray.map((t) => getPrice(t.api || t.id))); }
    saving = true; el.save.disabled = true;
    const jobs = st.tray.flatMap((it) => it.copies.map((c) => ({ it, c })));
    const total = jobs.length; let done = 0, failed = 0;
    const savedBy = new Map();
    el.prog.textContent = `Saving 0 of ${total}…`;
    let i = 0;
    async function worker() {
      while (i < jobs.length) {
        const { it, c } = jobs[i++];
        try {
          const r = await fetch('/admin/save', { method: 'POST', body: form(it, c), headers: { Accept: 'application/json' }, credentials: 'same-origin' });
          const j = await r.json().catch(() => ({}));
          if (!r.ok || !j.ok) throw new Error(j.error || 'Not saved');
          it.copies.splice(it.copies.indexOf(c), 1); c.err = '';
          savedBy.set(it.id, (savedBy.get(it.id) || 0) + 1);
        } catch (e) { failed += 1; c.err = e.message || 'Not saved'; }
        done += 1; el.prog.textContent = `Saving ${done} of ${total}…`;
      }
    }
    await Promise.all([worker(), worker()]);
    st.tray = st.tray.filter((t) => t.copies.length);
    persist(); saving = false; el.save.disabled = false;
    const saved = total - failed;
    if (el.total) el.total.textContent = (Number(el.total.textContent.replace(/\D/g, '')) + saved).toLocaleString('en-US');
    paintBar(); paintTiles();
    shown.forEach((c) => { const n = savedBy.get(c.id) || 0; if (n > 0) c.owned = (c.owned || 0) + n; });
    if (!failed) { closeSheet(); A.toast(`Saved ${saved} card${saved === 1 ? '' : 's'} ✓`, { kind: 'ok' }); if (shown.size) { el.grid.replaceChildren(...[...shown.values()].map(tile)); if (openSet) filterSet(); } }
    else { el.prog.textContent = `Saved ${saved}. ${failed} didn’t save; they’re still in the tray. Tap “Save all” to try again.`; paintList(); }
  });

  /* ---------- start ---------- */
  st.tray.forEach((t) => { t.copies.forEach((c) => { c.err = ''; }); if (!t.priced) loadPrice(t); });
  paintBar();
  /* ---------- Quick add start screen: name ideas, your recent cards, newest sets ---------- */
  const startEl = $('qaStart');
  if (startEl) {
    startEl.addEventListener('click', (e) => {
      const chip = e.target.closest('[data-q]'); if (!chip) return;
      el.q.value = chip.dataset.q; search(); el.q.scrollIntoView({ behavior: 'smooth', block: 'start' });
    });
    (async () => {
      try {
        const j = await getJSON('/admin/pick?recent=true');
        const seen = new Set();
        const cards = (j.cards || []).filter((c) => { const k = c.title.split(' - ')[0]; if (seen.has(k)) return false; seen.add(k); return true; }).slice(0, 8);
        $('qsRecent').replaceChildren(...cards.map((c) => {
          const b = document.createElement('button'); b.type = 'button'; b.className = 'qs-card'; b.dataset.q = c.title.split(' - ')[0].replace(/\s*\(.*\)$/, '');
          const pic = document.createElement('span'); pic.className = 'qs-pic';
          if (c.image) { const i = document.createElement('img'); i.src = c.image; i.alt = ''; i.loading = 'lazy'; pic.append(i); }
          if (c.qty > 1) { const q = document.createElement('span'); q.className = 'qty'; q.textContent = '×' + c.qty; pic.append(q); }
          const t = document.createElement('span'); t.className = 'qs-t'; t.textContent = c.title;
          b.append(pic, t); return b;
        }));
        if (!cards.length) $('qsRecent').textContent = 'Nothing yet. Your first cards will show up here.';
      } catch (_) {}
      try {
        const j = await getJSON('/admin/sets/search?q=' + L());
        $('qsSets').replaceChildren(...(j.sets || []).slice(0, 8).map((st) => {
          const a = document.createElement('a'); a.className = 'qs-set'; a.href = `/admin/sets?set=${encodeURIComponent(st.id)}&cat=${encodeURIComponent(st.cat)}&lang=${lang}`;
          const pic = document.createElement('span'); pic.className = 'qs-set-pic';
          if (st.logo) { const i = document.createElement('img'); i.src = st.logo; i.alt = ''; i.loading = 'lazy'; i.onerror = () => { i.remove(); pic.textContent = st.name; }; pic.append(i); } else pic.textContent = st.name;
          const t = document.createElement('span'); t.className = 'qs-t'; t.textContent = st.en_name ? `${st.name} · ${st.en_name}` : st.name;
          a.append(pic, t); return a;
        }));
      } catch (_) {}
    })();
  }

  paintLang();
  const qs = new URLSearchParams(location.search), q0 = qs.get('q');   // /admin/quick?q=slaking or /admin/sets?set=sv08 open straight onto results
  if (setMode) { if (qs.get('set')) showSet(qs.get('set'), false, qs.get('cat')); else { if (q0) el.q.value = q0; searchSets(); } }
  else if (q0) { el.q.value = q0; search(); } else if (!st.tray.length) el.q.focus({ preventScroll: true });
})();
