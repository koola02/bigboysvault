// My cards: tapping a card opens its edit form in a floating window. Tap outside to close; unsaved changes are checked first.
// The address still changes, so an edit link can be shared, and opening it directly shows the normal edit page.
(() => {
  const list = document.getElementById('cardsRoot'); if (!list) return;
  const A = window.Admin;
  const SEL = 'a[href^="/admin/edit/"]';
  let dialog, panel, dirty = false, baseUrl = null, baseTitle = '', asking = 0;

  function build() {
    dialog = document.createElement('dialog');
    dialog.className = 'float';
    dialog.innerHTML = '<div class="float-panel" tabindex="-1"></div><p class="float-hint">Tap outside the card to close</p>';
    panel = dialog.firstChild;
    document.body.append(dialog);
    // Closes only when the press both starts and ends outside, so selecting text past the edge doesn't shut it.
    let downOutside = false;
    dialog.addEventListener('pointerdown', (e) => { downOutside = !panel.contains(e.target); });
    dialog.addEventListener('click', (e) => { if (downOutside && !panel.contains(e.target)) leave(); downOutside = false; });
    dialog.addEventListener('cancel', (e) => { e.preventDefault(); leave(); }); // the Esc key
  }

  const okToLeave = () => !dirty || confirm('Discard your changes to this card?');

  function hide() {
    if (!dialog?.open) return;
    asking++;
    dialog.close();
    panel.replaceChildren();
    dirty = false;
    delete document.documentElement.dataset.float;
    document.documentElement.style.removeProperty('--sbw');
    document.title = baseTitle;
  }

  // Closing goes back one step in the browser's history, and the browser does that a moment later.
  // A card tapped in that moment waits for the close to finish, instead of being cancelled by it.
  let closing = null, closed = null;
  function goBack() {
    if (!closing) {
      closing = new Promise((ok) => { closed = ok; });
      setTimeout(() => { if (closed) { if (dialog?.open) hide(); done(); } }, 700);
    }
    history.back();
  }
  const done = () => { const ok = closed; closing = null; closed = null; ok?.(); };

  function leave() {
    if (!okToLeave()) return;
    dirty = false;
    if (history.state?.editCard) goBack(); else hide();
  }

  addEventListener('popstate', (e) => {
    if (e.state?.editCard || !dialog?.open) { done(); return; }
    // The Back button: same check as tapping outside. Saying no stays on the card.
    if (!closing && !okToLeave()) { history.pushState({ editCard: true }, '', panel.dataset.href); return; }
    hide();
    done();
  });

  // The list underneath catches up without a page reload, so you keep your place.
  async function refreshList() {
    try {
      const html = await (await fetch(baseUrl, { credentials: 'same-origin' })).text();
      const fresh = new DOMParser().parseFromString(html, 'text/html').getElementById('rows-wrap');
      if (fresh) { document.getElementById('rows-wrap').replaceWith(fresh); document.dispatchEvent(new Event('htmx:afterSwap')); }
    } catch (_) { location.reload(); }
  }
  function finish(message) {
    dirty = false;
    if (history.state?.editCard) goBack(); else hide();
    refreshList().then(() => A.toast(message, { kind: 'ok' }));
  }

  /* ---------- every copy of the card, each with its own finish, price and condition ---------- */
  const CONDS = ['Near Mint', 'LP-NM', 'Lightly Played', 'MP-LP', 'Moderately Played', 'HP-MP', 'Heavily Played', 'Damaged-HP', 'Damaged', 'Sealed', 'PSA 10', 'PSA 9', 'CGC 10', 'CGC 9.5', 'BGS 9.5'];
  const FINISHES = [['normal', 'Normal'], ['holofoil', 'Holofoil'], ['reverseHolofoil', 'Reverse Holo'], ['1stEditionHolofoil', '1st Ed. Holo'], ['1stEditionNormal', '1st Ed.'], ['full-art', 'Full art'], ['illustration', 'Illustration']];
  const peso = (n) => '₱' + Math.round(n || 0).toLocaleString('en-US');
  const make = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };

  function copies(wrap, ids, solo) {
    const form = wrap.querySelector('#entry');
    const field = (id) => wrap.querySelector('#' + id);
    const start = { finish: field('finishKey').value, price: field('price').value.replace(/[^\d.]/g, ''), cond: field('variant').value.trim() };
    let rarity = '';
    // Older imports keep everything in one line: "near-mint / reverse-holofoil / holo-rare". Show it as proper choices.
    const pretty = (t) => t.replace(/[-_]+/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase()).trim();
    const parts = start.cond.split('/').map((t) => t.trim());
    const known = CONDS.find((c) => c.toLowerCase() === (parts[0] || '').replace(/-/g, ' ').toLowerCase());
    if (known && parts.length > 1) {
      start.cond = known;
      const f = (parts[1] || '').toLowerCase();
      if (!start.finish || start.finish === 'market') start.finish = { 'normal': 'normal', 'holofoil': 'holofoil', 'reverse-holofoil': 'reverseHolofoil', 'reverse-holo': 'reverseHolofoil' }[f] || parts[1];
      if (parts[2]) rarity = pretty(parts[2]);
    } else if (known) start.cond = known;
    if (start.finish === 'market') start.finish = '';
    const rows = ids.map((id) => ({ id, ...start, market: false }));
    const removed = [];
    let finishes = [];
    // Notes, photo, binders and so on, to tell whether a copy edited on its own really differs.
    const sharedNow = () => { const f = new FormData(form), out = []; for (const [k, v] of f.entries()) if (!['id', 'variant', 'price', 'finish', 'price_src', 'rarity'].includes(k)) out.push(k + '=' + (v instanceof File ? v.name + v.size : v)); return out.join('&'); };
    const sharedAtStart = sharedNow();

    const box = make('div', 'step fc');
    const head = make('div', 'fc-head');
    const title = make('p', 'step-n');
    const qty = make('div', 'qa-qty');
    const minus = make('button', null, '−'); minus.type = 'button'; minus.setAttribute('aria-label', 'One copy fewer');
    const n = make('span');
    const plus = make('button', null, '+'); plus.type = 'button'; plus.setAttribute('aria-label', 'One more copy');
    qty.append(minus, n, plus);
    head.append(title); if (!solo) head.append(qty);
    const note = make('p', 'fc-note');
    const list = make('ol', 'qa-copies');
    box.append(head, note, list);
    form.prepend(box);
    const shared = make('p', 'fc-shared');
    box.after(shared);

    // The preview and the hidden fields follow the first copy.
    const sync = () => { field('variant').value = rows[0].cond; field('price').value = rows[0].price; field('finishKey').value = rows[0].finish; };
    const changed = () => { dirty = true; };

    function paint() {
      n.textContent = rows.length;
      title.textContent = solo ? 'Just this card' : rows.length === 1 ? 'This card' : `Your ${rows.length} copies`;
      note.textContent = solo ? 'Anything you change here is for this one card. If it ends up different, it becomes its own card.'
        : 'Give a copy its own condition, finish or price and it becomes its own card when you save.';
      shared.textContent = solo ? '' : rows.length > 1 ? `The name, photo, binders and notes below are for all ${rows.length} copies.` : '';
      shared.hidden = !shared.textContent;
      list.replaceChildren(...rows.map((c, i) => {
        const row = make('li', 'qa-copy');
        const label = make('span', 'qa-copy-n', solo ? 'This card' : `Card ${i + 1}`);
        const fin = document.createElement('select'); fin.setAttribute('aria-label', `Finish of card ${i + 1}`);
        if (!c.finish) fin.append(new Option('Pick a finish', '', true, true));
        finishes.forEach((f) => fin.append(new Option(`${f.label} · ${peso(f.php)}`, f.key, false, f.key === c.finish)));
        const known = new Set(finishes.map((f) => f.key));
        FINISHES.filter(([k]) => !known.has(k)).forEach(([k, l]) => { known.add(k); fin.append(new Option(l, k, false, k === c.finish)); });
        if (c.finish && !known.has(c.finish)) fin.append(new Option(pretty(c.finish), c.finish, true, true));
        const pr = document.createElement('input'); pr.className = 'qa-price'; pr.inputMode = 'decimal'; pr.placeholder = '₱0'; pr.value = c.price; pr.setAttribute('aria-label', `Price of card ${i + 1} in pesos`);
        fin.onchange = () => { c.finish = fin.value; const f = finishes.find((x) => x.key === fin.value); if (f) { c.price = String(Math.round(f.php)); c.market = true; pr.value = c.price; } else c.market = false; changed(); sync(); };
        pr.oninput = () => { c.price = pr.value.replace(/[^\d.]/g, ''); c.market = false; changed(); sync(); };
        const cond = document.createElement('select'); cond.setAttribute('aria-label', `Condition of card ${i + 1}`);
        if (!c.cond) cond.append(new Option('Pick a condition', '', true, true));
        else if (!CONDS.includes(c.cond)) cond.append(new Option(c.cond, c.cond, true, true));
        CONDS.forEach((v) => cond.append(new Option(v, v, false, v === c.cond)));
        cond.append(new Option('Type my own…', '__own'));
        cond.onchange = () => {
          if (cond.value === '__own') { const v = (prompt('Condition or grade') || '').trim().slice(0, 80); if (v) c.cond = v; paint(); }
          else c.cond = cond.value;
          changed(); sync();
        };
        const acts = make('span', 'fc-acts');
        if (!solo && rows.length > 1 && c.id) {
          const one = make('button', 'fc-one', '✎ Just this one'); one.type = 'button'; one.setAttribute('aria-label', `Edit only card ${i + 1}: its name, photo and more`);
          one.onclick = () => { if (!okToLeave()) return; dirty = false; open(`/admin/edit/${c.id}`, [c.id], true); };
          acts.append(one);
        }
        if (rows.length > 1) {
          const x = make('button', 'qa-copy-x', '✕'); x.type = 'button'; x.setAttribute('aria-label', `Take out card ${i + 1}`);
          x.onclick = () => { rows.splice(rows.indexOf(c), 1); if (c.id) removed.push(c.id); changed(); sync(); paint(); };
          acts.append(x);
        }
        row.append(label, acts, fin, cond, pr);
        return row;
      }));
      minus.disabled = rows.length < 2;
    }
    minus.onclick = () => { if (rows.length < 2) return; const c = rows.pop(); if (c.id) removed.push(c.id); changed(); sync(); paint(); };
    plus.onclick = () => { const last = rows[rows.length - 1]; rows.push({ ...last, id: null }); changed(); paint(); };
    paint();

    // Market prices for the finish list, when this is a catalogue card.
    const api = field('apiId').value;
    if (api) fetch('/admin/price?id=' + encodeURIComponent(api), { headers: { Accept: 'application/json' }, credentials: 'same-origin' }).then((r) => r.json()).then((j) => {
      const m = j.matches?.[0]; if (!m || !wrap.isConnected) return;
      finishes = (m.finishes || []).map((f) => ({ key: f.key, label: f.label, php: f.php }));
      rarity = m.rarity || rarity;
      if (!wrap.contains(document.activeElement) || document.activeElement === panel) paint();
    }).catch(() => {});

    form.saveCopies = async (fd) => {
      const ownThings = solo && sharedNow() !== sharedAtStart;
      const newPhoto = fd.get('photo')?.size > 0 || !!fd.get('photo_url');
      let first = null;
      for (const c of rows) {
        // A new copy starts as a true copy, picture and binders included.
        if (!c.id) { const j = await A.post('/admin/dup/' + ids[0]); c.id = j.id; }
        const f = new FormData();
        for (const [k, v] of fd.entries()) f.append(k, v);
        // One new photo is stored once and shared by every copy.
        if (newPhoto && first) { f.delete('photo'); f.delete('photo_url'); f.set('image_from', first); }
        first = first || c.id;
        f.set('id', c.id); f.set('variant', c.cond); f.set('price', String(Number(c.price) || 0)); f.set('finish', c.finish); f.set('price_src', c.market ? 'market' : '');
        if (rarity) f.set('rarity', rarity);
        if (ownThings) f.set('solo', '1');
        const r = await fetch('/admin/save', { method: 'POST', body: f, headers: { Accept: 'application/json' }, credentials: 'same-origin' });
        const j = await r.json().catch(() => ({}));
        if (!r.ok || !j.ok) throw new Error(j.error || 'Couldn’t save. Your changes are still here. Try again.');
      }
      for (const id of removed.splice(0)) await A.post('/admin/delete/' + id);
    };
  }

  async function open(href, ids, solo = false) {
    if (closing) await closing;
    const mine = ++asking;
    let doc;
    try {
      const r = await fetch(href, { credentials: 'same-origin' });
      if (!r.ok || r.redirected) throw new Error('status');
      doc = new DOMParser().parseFromString(await r.text(), 'text/html');
    } catch (_) { location.href = href; return; }
    const source = doc.querySelector('[data-editing]'), grid = doc.querySelector('.a-grid');
    if (!source || !grid) { location.href = href; return; }
    if (mine !== asking) return;
    // Filtering the list can swap out part of the page and take the window with it: then it's made again.
    if (!dialog || !dialog.isConnected) { if (dialog?.open) hide(); build(); }

    const wrap = document.createElement('section');
    wrap.className = 'wrap float-edit';
    wrap.dataset.editing = source.dataset.editing;
    wrap.dataset.id = source.dataset.id;
    const h = document.createElement('h1'); h.className = 'h1'; h.textContent = 'Edit card';
    const note = doc.getElementById('toast');
    wrap.append(h, document.adoptNode(grid));
    if (note) wrap.append(document.adoptNode(note));
    panel.replaceChildren(wrap);
    panel.dataset.href = href;
    panel.scrollTop = 0;

    dirty = false;
    if (dialog.open) history.replaceState({ editCard: true }, '', href);
    else {
      baseUrl = location.href; baseTitle = document.title;
      document.documentElement.style.setProperty('--sbw', `${innerWidth - document.documentElement.clientWidth}px`);
      document.documentElement.dataset.float = '1';
      // The list's filters leave htmx history entries behind. Stepping back onto one makes htmx put back an old copy
      // of the whole page, which drops this window and leaves the buttons dead. So the entry under the window is an ordinary one.
      if (history.state?.htmx) history.replaceState({ mcBase: true }, '', location.href);
      history.pushState({ editCard: true }, '', href);
      dialog.showModal();
    }
    document.title = doc.title;
    panel.focus({ preventScroll: true });
    window.mountCardForm();
    wrap.querySelectorAll('[data-tag-box]').forEach((b) => delete b.dataset.ready);
    copies(wrap, ids, solo);

    const form = wrap.querySelector('#entry');
    form.addEventListener('input', () => { dirty = true; });
    form.addEventListener('change', () => { dirty = true; });
    form.addEventListener('card-saved', () => finish('Saved ✓'));

    // Delete stays in the window too.
    const del = wrap.querySelector('form.danger');
    if (del) {
      del.removeAttribute('onsubmit');
      del.addEventListener('submit', async (e) => {
        e.preventDefault();
        if (!confirm('Delete this card? This can’t be undone.')) return;
        try { await A.post(del.getAttribute('action')); finish('Card deleted'); }
        catch (err) { A.toast(err.message, { kind: 'err' }); }
      });
    }
  }

  // Show more adds the next cards under the ones already there. The page doesn't reload, so you stay where you are,
  // and the address keeps up, so a refresh or an edit brings back the same cards.
  document.addEventListener('click', async (e) => {
    const more = e.target.closest?.('a.mc-more');
    if (!more || e.button !== 0 || e.metaKey || e.ctrlKey) return;
    e.preventDefault();
    if (more.dataset.busy) return;
    more.dataset.busy = '1';
    try {
      const html = await (await fetch(more.href, { credentials: 'same-origin' })).text();
      const doc = new DOMParser().parseFromString(html, 'text/html');
      const freshRows = doc.getElementById('rows'), have = document.getElementById('rows');
      if (!freshRows || !have) throw new Error('page');
      const seen = new Set([...have.children].map((li) => li.dataset.id));
      const y = scrollY;
      [...freshRows.children].filter((li) => !seen.has(li.dataset.id)).forEach((li) => have.append(document.adoptNode(li)));
      const nextBtn = doc.querySelector('a.mc-more');
      if (nextBtn) { more.href = nextBtn.getAttribute('href'); more.innerHTML = '<i aria-hidden="true"></i>Loading more cards…'; delete more.dataset.busy; nearEnd.unobserve(more); nearEnd.observe(more); }
      else { const end = doc.querySelector('.mc-end'); const p = more.closest('p'); if (end) p?.replaceWith(document.adoptNode(end)); else p?.remove(); }
      const count = doc.getElementById('countLine'); if (count) document.getElementById('countLine')?.replaceWith(document.adoptNode(count));
      history.replaceState(history.state, '', more.href.replace(/page=\d+/, `page=${Math.max(0, Number((more.href.match(/page=(\d+)/) || [0, 1])[1]) - 1)}`));
      scrollTo(0, y);
      document.dispatchEvent(new Event('htmx:afterSwap'));
    } catch (_) { location.href = more.href; }
  });

  // No button to press: the next cards arrive as you near the end of the list.
  const nearEnd = new IntersectionObserver((list) => {
    for (const x of list) if (x.isIntersecting) x.target.click();
  }, { rootMargin: '1200px 0px' });
  const watchMore = () => document.querySelectorAll('a.mc-more:not([data-watched])').forEach((a) => { a.dataset.watched = '1'; nearEnd.observe(a); });
  watchMore();
  document.addEventListener('htmx:afterSwap', watchMore);
  new MutationObserver(watchMore).observe(document.getElementById('cardsRoot'), { childList: true, subtree: true });

  document.addEventListener('click', (e) => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    if (dialog?.contains(e.target)) return;
    const a = e.target.closest?.(SEL);
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute('href'), own = Number(href.split('/').pop());
    const ids = (a.closest('.ctile')?.dataset.ids || '').split(',').filter(Boolean).map(Number);
    open(href, ids.length ? ids : [own]);
  });
})();
