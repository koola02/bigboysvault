// Bookmarks: the Bookmark buttons on stores and cards (data-bm="store" | "product" | "card", data-id), the number
// beside Bookmarks in the menus (data-bm-count), and the Bookmarks page (/me/bookmarks): take one off, or put a whole
// store's cards in the cart at once. Bookmarking needs an account; a visitor is sent to log in, and comes back.
(() => {
  const KEY = { store: 'stores', product: 'products', card: 'cards' };
  let mine = null; // { stores, products, cards } as Sets, once the site has said
  let member = null, known = false, loading = null;

  const who = () => (window.bbvWho ? window.bbvWho() : (window.bbvWhoAnswer ||= fetch('/me/who', { credentials: 'same-origin', headers: { Accept: 'application/json' } }).then((r) => (r.ok ? r.json() : {})).catch(() => ({}))));
  const say = (msg, bad) => {
    let t = document.querySelector('.me-toast');
    if (!t) { t = document.createElement('div'); t.className = 'me-toast'; t.setAttribute('role', 'status'); }
    (document.querySelector('dialog[open]:last-of-type') || document.body).append(t);
    t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on');
    clearTimeout(say.t); say.t = setTimeout(() => t.classList.remove('on'), bad ? 5000 : 2600);
  };
  const count = (n) => document.querySelectorAll('[data-bm-count]').forEach((b) => { b.textContent = n; b.hidden = !n; });

  function paint(root = document) {
    if (!mine) return;
    root.querySelectorAll('[data-bm]').forEach((b) => {
      const on = mine[KEY[b.dataset.bm]]?.has(Number(b.dataset.id)) || false;
      b.setAttribute('aria-pressed', String(on));
      const l = b.querySelector('.bm-l'); if (l) l.textContent = on ? 'Bookmarked' : 'Bookmark';
    });
  }
  // What this person bookmarked: asked only on pages that have a Bookmark button (or once one appears).
  function load() {
    if (!member) return Promise.resolve();
    return (loading ||= fetch('/me/bookmarks/ids', { credentials: 'same-origin', headers: { Accept: 'application/json' } })
      .then((r) => (r.ok ? r.json() : null))
      .then((j) => { if (!j) return; mine = { stores: new Set(j.stores), products: new Set(j.products), cards: new Set(j.cards) }; count(j.count); paint(); })
      .catch(() => {}));
  }

  document.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-bm]'); if (!b) return;
    e.preventDefault(); e.stopPropagation();
    if (!known) await who().then((j) => { member = j && j.member; known = true; });
    if (!member) {
      try { sessionStorage.setItem('bbv:after-login', location.pathname + location.search); sessionStorage.setItem('bbv:login-why', 'bookmark'); } catch (_) {}
      location.href = '/login';
      return;
    }
    const kind = b.dataset.bm, id = Number(b.dataset.id);
    b.disabled = true;
    try {
      const r = await fetch('/me/bookmarks', { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify({ kind, id }) });
      const j = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
      if (!mine) mine = { stores: new Set(), products: new Set(), cards: new Set() };
      if (j.on) mine[KEY[kind]].add(id); else mine[KEY[kind]].delete(id);
      paint(); count(j.count);
      // On the Bookmarks page, a card or store taken off goes away (and its store's heading, once it's empty).
      const row = !j.on && b.closest('.bm-page') && b.closest('.bm-card, .bm-store');
      if (row) {
        const group = row.closest('.bm-group'); row.remove();
        if (group && !group.querySelector('.bm-card')) group.remove();
      }
      say(j.on ? 'Bookmarked ✓ Find it under Bookmarks in the menu.' : 'Taken Out Of Your Bookmarks');
    } catch (err) { say(err.message, true); }
    b.disabled = false;
  }, true);

  // "Add all to cart": every card of that store that isn't in the cart yet (cart.js takes each one).
  document.addEventListener('click', (e) => {
    const b = e.target.closest('[data-bm-all]'); if (!b) return;
    const group = b.closest('.bm-group');
    const todo = [...group.querySelectorAll('.cart-add:not(.in)')];
    todo.forEach((x) => x.click());
    say(todo.length ? `${todo.length} card${todo.length === 1 ? '' : 's'} put in your cart ✓` : 'They are all in your cart already');
  });

  // A card window filled in later has its own Bookmark button.
  document.addEventListener('float:filled', (e) => { if (e.detail && e.detail.querySelector && e.detail.querySelector('[data-bm]')) load().then(() => paint(e.detail)); });

  who().then((j) => {
    member = j && j.member; known = true;
    if (!member) return;
    if (typeof j.bookmarks === 'number') count(j.bookmarks);
    if (document.querySelector('[data-bm]')) load();
  });
})();
