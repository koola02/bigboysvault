// The Manager's Showcase: the hand-picked cards in the row at the top of the home page.
// Tap a card to put it in or take it out, drag (or use the arrows) to change the order. Every change saves by itself,
// and the strip at the top of the page shows what visitors will see.
(() => {
  const root = document.getElementById('shw'); if (!root) return;
  const A = window.Admin;
  const $ = (id) => document.getElementById(id);
  const list = $('shwList'), results = $('shwResults'), q = $('shwQ'), count = $('shwN'), none = $('shwNone');
  const prev = $('shwPrev'), prevNone = $('shwPrevNone'), state = $('shwState'), found = $('shwFound');
  const MAX = Number(root.dataset.max) || 24;
  const el = (tag, cls, txt) => { const n = document.createElement(tag); if (cls) n.className = cls; if (txt != null) n.textContent = txt; return n; };
  const plural = (n) => `${n} card${n === 1 ? '' : 's'}`;

  // What is known about each card: the ones in the showcase come with the page, the rest arrive with a search.
  const cards = new Map();
  let ids = [...list.querySelectorAll('.shw-c')].map((li) => {
    const c = { id: Number(li.dataset.id), title: li.dataset.title, image: li.dataset.image, price: li.dataset.price, off: li.dataset.off || '' };
    cards.set(c.id, c); return c.id;
  });
  let shown = [], saveT = 0, asked = 0, sent = 0;

  const pic = (c, cls) => { const s = el('span', cls); if (c.image) { const i = el('img'); i.src = c.image; i.alt = ''; i.loading = 'lazy'; i.draggable = false; s.append(i); } return s; };

  function tile(id, i) {
    const c = cards.get(id) || { id, title: 'Card', image: '', price: '', off: '' };
    const li = el('li', 'shw-c'); li.dataset.id = id;
    const acts = el('span', 'shw-acts'); acts.dataset.nodrag = '';
    const back = el('button', null, '‹'); back.type = 'button'; back.disabled = i === 0; back.setAttribute('aria-label', `Move ${c.title} earlier`); back.onclick = () => move(id, -1);
    const on = el('button', null, '›'); on.type = 'button'; on.disabled = i === ids.length - 1; on.setAttribute('aria-label', `Move ${c.title} later`); on.onclick = () => move(id, 1);
    const out = el('button', 'shw-x', '×'); out.type = 'button'; out.setAttribute('aria-label', `Take ${c.title} out of the showcase`); out.onclick = () => toggle(c);
    acts.append(back, on, out);
    li.append(pic(c, 'shw-pic'), el('b', null, c.title), el('small', null, c.price));
    if (c.off) li.append(el('small', 'shw-off', c.off));
    li.append(acts);
    return li;
  }

  // The strip visitors see: only the cards that can be shown (in stock, with a photo), drawn the way the home page draws them.
  function story(id) {
    const c = cards.get(id); if (!c || c.off || !c.image) return null;
    const s = el('span', 'story');
    const i = el('img'); i.src = c.image; i.alt = ''; i.loading = 'lazy';
    const av = el('span', 'av sm'); const logo = el('img'); logo.src = '/static/img/logo-sm-144.png?v=1'; logo.alt = ''; av.append(logo);
    s.append(i, av, el('b', null, c.title)); return s;
  }

  function draw() {
    list.replaceChildren(...ids.map(tile));
    count.textContent = ids.length; none.hidden = ids.length > 0;
    const strip = ids.map(story).filter(Boolean);
    prev.replaceChildren(...strip); prev.hidden = !strip.length; prevNone.hidden = strip.length > 0;
    mark();
  }
  const mark = () => results.querySelectorAll('.shw-r').forEach((b) => {
    const on = ids.includes(Number(b.dataset.id));
    b.classList.toggle('on', on); b.setAttribute('aria-pressed', String(on));
  });

  function drawResults() {
    results.replaceChildren(...shown.map((c) => {
      const b = el('button', 'shw-r'); b.type = 'button'; b.dataset.id = c.id;
      b.append(pic(c, 'shw-rpic'), el('b', null, c.title), el('small', null, c.price));
      b.onclick = () => toggle(c);
      return b;
    }));
    mark();
  }

  function changed() {
    draw();
    state.textContent = 'Saving…';
    clearTimeout(saveT); saveT = setTimeout(save, 450);
  }
  function toggle(c) {
    if (ids.includes(c.id)) ids = ids.filter((v) => v !== c.id);
    else {
      if (ids.length >= MAX) { A.toast(`The showcase holds ${MAX} cards. Take one out first.`, { kind: 'err' }); return; }
      ids = [...ids, c.id];
    }
    changed();
  }
  function move(id, by) {
    const i = ids.indexOf(id), j = i + by;
    if (i < 0 || j < 0 || j >= ids.length) return;
    const next = [...ids]; next.splice(j, 0, next.splice(i, 1)[0]); ids = next;
    changed();
    // Keep the keyboard where it was: on the same card's same arrow (or its other one, at an end).
    const li = list.querySelector(`.shw-c[data-id="${id}"]`);
    const btns = li ? [...li.querySelectorAll('.shw-acts button')] : [];
    const want = btns[by < 0 ? 0 : 1];
    (want && !want.disabled ? want : btns.find((b) => !b.disabled))?.focus({ preventScroll: true });
  }

  async function save() {
    const my = ++sent, order = [...ids];
    try {
      const j = await A.postJSON('/admin/showcase', { ids: order });
      if (my !== sent) return; // a newer change is on its way
      // The server's word on each card (a card that sold out says so), without undoing anything done since.
      (j.cards || []).forEach((c) => cards.set(c.id, c));
      if (order.join() === ids.join()) draw();
      state.textContent = 'Saved ✓';
      A.toast(`Showcase saved ✓ ${plural(order.length)}`, { kind: 'ok' });
    } catch (err) {
      if (my !== sent) return;
      state.textContent = 'Not saved';
      A.toast(err.message || 'That didn’t save. Check your connection and try again.', { kind: 'err' });
    }
  }

  async function load(text) {
    const my = ++asked;
    try {
      const r = await fetch('/admin/pick?q=' + encodeURIComponent(text), { headers: { Accept: 'application/json' }, credentials: 'same-origin' });
      const j = await r.json(); if (my !== asked) return;
      shown = j.cards || [];
      // A card already in the showcase keeps what the page knows about it (why it isn't shown, if it isn't).
      shown.forEach((c) => { if (!cards.has(c.id)) cards.set(c.id, { id: c.id, title: c.title, image: c.image, price: c.price, off: '' }); });
      found.textContent = text.trim() ? (shown.length ? `${plural(shown.length)} found. Tap one to add it.` : 'No card matches that. Try part of the name, or its number.') : 'Your most valuable cards first. Type a name or number to find any card.';
      drawResults();
    } catch (_) { if (my === asked) found.textContent = 'Couldn’t load your cards. Check your connection.'; }
  }

  let typing = 0;
  q.addEventListener('input', () => { clearTimeout(typing); typing = setTimeout(() => load(q.value), 250); });
  if (window.dragSort) window.dragSort(list, { item: '.shw-c', key: (n) => Number(n.dataset.id), onDrop: ([order]) => { ids = order; changed(); } });
  // Leaving in the half second before a change is sent would lose it: send it at once.
  addEventListener('pagehide', () => { if (state.textContent === 'Saving…') navigator.sendBeacon?.('/admin/showcase', new Blob([JSON.stringify({ ids })], { type: 'application/json' })); });
  draw(); load('');

  // The welcome line: what a visitor who isn't logged in reads under the site's name. Saved a moment after you stop typing.
  const box = $('shwWelcome'), see = $('shwWSee'), wState = $('shwWState'), holder = $('shwWelcomeBox');
  if (box && see && holder) {
    const said = () => (box.value.replace(/\s+/g, ' ').trim() || box.placeholder).split('{count}').join(holder.dataset.count || '0');
    const show = () => { see.textContent = said(); };
    let kept = box.value, wT = 0, wSent = 0;
    async function saveLine() {
      const text = box.value; if (text === kept) { wState.textContent = ''; return; }
      const my = ++wSent;
      try {
        const j = await A.postJSON('/admin/welcome', { text });
        if (my !== wSent) return;
        kept = text;
        // What was kept is one line: show it that way once you have left the box, never while you are typing in it.
        if (document.activeElement !== box && typeof j.text === 'string') { box.value = j.text; kept = j.text; show(); }
        wState.textContent = 'Saved ✓';
      } catch (err) {
        if (my !== wSent) return;
        wState.textContent = 'Not saved';
        A.toast(err.message || 'That didn’t save. Check your connection and try again.', { kind: 'err' });
      }
    }
    box.addEventListener('input', () => { show(); wState.textContent = 'Saving…'; clearTimeout(wT); wT = setTimeout(saveLine, 700); });
    box.addEventListener('blur', () => { clearTimeout(wT); saveLine(); });
    addEventListener('pagehide', () => { if (box.value !== kept) navigator.sendBeacon?.('/admin/welcome', new Blob([JSON.stringify({ text: box.value })], { type: 'application/json' })); });
    show();
  }

  // The site's name: saved with its button.
  const brand = $('shwBrand'), bState = $('shwBState');
  if (brand) brand.addEventListener('submit', async (e) => {
    e.preventDefault();
    bState.textContent = 'Saving…';
    try {
      const r = await fetch('/admin/settings', { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json', 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams(new FormData(brand)) });
      const j = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(j.error || 'That didn’t save. Check your connection and try again.');
      bState.textContent = 'Saved ✓';
    } catch (err) {
      bState.textContent = 'Not saved';
      A.toast(err.message, { kind: 'err' });
    }
  });
})();
