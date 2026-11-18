// The cart: cards from the vault and from sellers' stores, kept in this browser until the order is sent.
// Sending it makes one order for the vault's cards (the admins answer it) and a reservation per seller's card.
(() => {
  const KEY = 'bbv:cart';
  const $ = (id) => document.getElementById(id);
  const fab = $('cartFab'), n = $('cartN'), drawer = $('cartDrawer'), list = $('cartList'), note = $('cartNote'), go = $('cartGo'), total = $('cartTotal'), empty = $('cartEmpty');
  if (!fab || !drawer) return;
  const peso = (v) => '₱' + Math.round(Number(v) || 0).toLocaleString('en-US');
  const el = (tag, cls, txt) => { const x = document.createElement(tag); if (cls) x.className = cls; if (txt != null) x.textContent = txt; return x; };
  const toast = (msg, bad) => {
    let t = document.querySelector('.cart-toast');
    if (!t) { t = el('div', 'cart-toast'); t.setAttribute('role', 'status'); document.body.append(t); }
    t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on');
    clearTimeout(toast.t); toast.t = setTimeout(() => t.classList.remove('on'), bad ? 5000 : 2600);
  };

  let lines = [];
  try { lines = JSON.parse(localStorage.getItem(KEY) || '[]'); } catch (_) {}
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(lines)); } catch (_) {} badge(); };
  const count = () => lines.reduce((a, l) => a + (l.qty || 1), 0);
  function badge() {
    const c = count();
    n.textContent = c; fab.hidden = c === 0 && drawer.hidden;
    fab.classList.toggle('has', c > 0);
    // The cart lives in the menu (under Orders): the menu buttons carry its number on every screen.
    document.querySelectorAll('[data-cart-count]').forEach((b) => { b.textContent = c; b.hidden = c === 0; });
    document.querySelectorAll('[data-cart-vault], [data-cart-card]').forEach((b) => {
      const kind = b.dataset.cartVault != null ? 'vault' : 'card', id = Number(b.dataset.cartVault ?? b.dataset.cartCard);
      const inCart = lines.some((l) => l.kind === kind && l.id === id);
      if (b.classList.contains('in') === inCart) return;
      b.classList.toggle('in', inCart);
      // The small round button says what pressing it does now (screen readers and the tooltip).
      if (b.classList.contains('cart-mini')) {
        const t = b.dataset.title || 'this card';
        b.setAttribute('aria-label', inCart ? `Take ${t} out of your cart` : `Add ${t} to your cart`); b.title = inCart ? 'In your cart: tap to take it out' : 'Add to cart';
      }
    });
  }

  /* ----- adding ----- */
  // A button adds one copy. Once the card is in the cart the button says so, and pressing it again does what it
  // shows: the small round one (drawn as "−") takes the card out, the big "In cart ✓" one opens the cart. How many
  // copies is changed in the cart itself, where '+' stops at the copies there are.
  function add(kind, id, snap) {
    lines.push({ kind, id, qty: 1, ...snap });
    save();
    toast(`${snap.title || 'Card'} is in your cart`);
    fab.classList.remove('bump'); void fab.offsetWidth; fab.classList.add('bump');
  }
  function takeOut(kind, id) {
    const l = lines.find((x) => x.kind === kind && x.id === id);
    lines = lines.filter((x) => x !== l); save(); if (!drawer.hidden) paint();
    toast(`${(l && l.title) || 'Card'} is out of your cart`);
  }
  document.addEventListener('click', (e) => {
    const b = e.target.closest('[data-cart-vault], [data-cart-card]'); if (!b) return;
    e.preventDefault(); e.stopPropagation();
    const kind = b.dataset.cartVault != null ? 'vault' : 'card';
    const id = Number(b.dataset.cartVault ?? b.dataset.cartCard);
    if (!id) return;
    if (lines.some((l) => l.kind === kind && l.id === id)) {
      if (b.classList.contains('cart-mini')) takeOut(kind, id); else { opener = b; open(); }
      return;
    }
    add(kind, id, { title: b.dataset.title || '', price: Number(b.dataset.price) || 0, image: b.dataset.image || '', url: b.dataset.url || '', seller_name: b.dataset.seller || '', seller_slug: b.dataset.sellerSlug || '' });
  }, true);
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    const b = e.target.closest && e.target.closest('.cart-mini'); if (!b) return;
    e.preventDefault(); b.click();
  });

  /* ----- the drawer ----- */
  // Opening moves focus into the drawer; closing brings it back to what opened it (the cart button, or the
  // "In cart ✓" button), so keyboard users don't get lost.
  // A see-through layer behind the drawer: a tap beside the cart closes it and does nothing else (before, the tap
  // went through to the page and could open a card behind the open cart).
  const scrim = el('div', 'cart-scrim'); scrim.hidden = true; drawer.before(scrim);
  scrim.addEventListener('click', (e) => { e.preventDefault(); e.stopPropagation(); close(); });
  let opener = null;
  const open = async () => {
    // Opened from the menu on a phone or tablet: the menu steps aside for it.
    if (document.documentElement.classList.contains('menu-open')) { document.documentElement.classList.remove('menu-open'); document.querySelectorAll('[data-menu-toggle]').forEach((b) => b.setAttribute('aria-expanded', 'false')); }
    drawer.hidden = false; scrim.hidden = false; fab.hidden = false; fab.setAttribute('aria-expanded', 'true'); document.body.classList.add('cart-open'); paint();
    const x = drawer.querySelector('[data-cart-close]'); if (x) x.focus();
    await refresh();
  };
  const close = () => {
    const inside = drawer.contains(document.activeElement);
    drawer.hidden = true; scrim.hidden = true; fab.setAttribute('aria-expanded', 'false'); document.body.classList.remove('cart-open'); badge();
    let back = opener && opener.isConnected ? opener : fab; opener = null;
    if (!back.getClientRects().length) back = [...document.querySelectorAll('[data-menu-toggle]')].find((b) => b.getClientRects().length) || back;
    if (inside && !back.hidden) back.focus();
  };
  fab.addEventListener('click', () => { if (drawer.hidden) { opener = fab; open(); } else close(); });
  drawer.querySelectorAll('[data-cart-close]').forEach((b) => b.addEventListener('click', close));
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !drawer.hidden) close(); });

  async function refresh() {
    const v = lines.filter((l) => l.kind === 'vault').map((l) => l.id), c = lines.filter((l) => l.kind === 'card').map((l) => l.id);
    if (!v.length && !c.length) return;
    try {
      const r = await fetch(`/cart/lines?v=${v.join(',')}&c=${c.join(',')}`, { credentials: 'same-origin', headers: { Accept: 'application/json' } });
      const j = await r.json();
      for (const fresh of j.lines || []) {
        const l = lines.find((x) => x.kind === fresh.kind && x.id === fresh.id); if (!l) continue;
        Object.assign(l, { title: fresh.title, price: fresh.price, image: fresh.image || l.image, url: fresh.url, ok: fresh.ok, why: fresh.why, seller_name: fresh.seller_name, seller_slug: fresh.seller_slug, left: fresh.left });
        if (fresh.left > 0 && l.qty > fresh.left) l.qty = fresh.left;
      }
      save(); paint();
    } catch (_) {}
  }
  // A line the vault was short of becomes sendable again once its count changes. A card with none left stays off.
  const unmark = (l) => { if (l.kind === 'vault' && l.ok === false && l.left !== 0) { delete l.ok; delete l.why; } };
  function paint() {
    list.replaceChildren();
    // Each card sits under the store it came from, with a link back to that store (the vault's is a store too).
    const groups = new Map();
    const site = document.querySelector('meta[property="og:site_name"]')?.content || 'Big Boys Vault';
    for (const l of lines) {
      const slug = l.kind === 'vault' ? 'vault' : (l.seller_slug || '');
      const k = l.kind === 'vault' ? 'vault' : `s:${slug || l.id}`;
      if (!groups.has(k)) groups.set(k, { name: l.seller_name || (l.kind === 'vault' ? site : 'a seller'), slug, lines: [] });
      groups.get(k).lines.push(l);
    }
    let sum = 0, okCount = 0;
    for (const g of groups.values()) {
      const h = el('h3', 'cart-g', 'From ');
      if (g.slug) { const a = el('a', 'cart-store', g.name); a.href = `/store/${g.slug}`; h.append(a); } else h.append(g.name);
      list.append(h);
      for (const l of g.lines) {
        const li = el('li', 'cart-line' + (l.ok === false ? ' off' : ''));
        const pic = el('a', 'cart-pic'); pic.href = l.url || '#'; if (l.image) { const i = el('img'); i.src = l.image; i.alt = ''; i.width = 48; i.height = 67; pic.append(i); }
        const body = el('div', 'cart-body');
        const name = el('a', 'cart-t', l.title || 'Card'); name.href = l.url || '#'; body.append(name);
        if (l.ok === false) body.append(el('small', 'cart-why', l.why || 'Not available'));
        else body.append(el('small', null, `${peso(l.price)} each` + (l.left > 0 && (l.qty || 1) >= l.left ? ` · ${l.left === 1 ? 'only 1 there' : `all ${l.left} there`}` : '')));
        const qty = el('div', 'cart-qty');
        const minus = el('button', null, '−'); minus.type = 'button'; minus.setAttribute('aria-label', 'One fewer');
        const num = el('span', null, String(l.qty || 1));
        const plus = el('button', null, '+'); plus.type = 'button'; plus.setAttribute('aria-label', 'One more');
        // Changing how many clears "not available", so the line can be sent again (the server checks it once more).
        minus.onclick = () => { l.qty = (l.qty || 1) - 1; if (l.qty <= 0) lines = lines.filter((x) => x !== l); unmark(l); save(); paint(); };
        // '+' never lowers the count, even when fewer are left than asked for (the line then stays as it is).
        plus.onclick = () => { const q = l.qty || 1; l.qty = Math.min(99, l.left ? Math.max(q, Math.min(l.left, q + 1)) : q + 1); unmark(l); save(); paint(); };
        // Every copy there is is already in the cart: '+' is switched off and the line says so.
        const atMost = l.left > 0 && (l.qty || 1) >= l.left;
        if (atMost) { plus.disabled = true; plus.setAttribute('aria-label', `Only ${l.left} there`); plus.title = `Only ${l.left} there`; }
        qty.append(minus, num, plus);
        const line = el('b', 'cart-sum', peso((l.price || 0) * (l.qty || 1)));
        const x = el('button', 'cart-x', '✕'); x.type = 'button'; x.setAttribute('aria-label', `Take ${l.title} out`); x.onclick = () => { lines = lines.filter((y) => y !== l); save(); paint(); };
        li.append(pic, body, qty, line, x);
        list.append(li);
        if (l.ok !== false) { sum += (l.price || 0) * (l.qty || 1); okCount += 1; }
      }
    }
    total.textContent = peso(sum);
    empty.hidden = lines.length > 0;
    go.disabled = okCount === 0;
    go.textContent = okCount ? `Send order · ${peso(sum)}` : 'Send order';
    badge();
  }

  /* ----- sending ----- */
  go.addEventListener('click', async () => {
    const items = lines.filter((l) => l.ok !== false).map((l) => ({ kind: l.kind, id: l.id, qty: l.qty || 1 }));
    if (!items.length) return;
    go.disabled = true; go.textContent = 'Sending…';
    try {
      const r = await fetch('/cart/checkout', { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify({ items, message: note.value }) });
      if (r.status === 401) {
        // After signing in, the sign-in page brings the buyer back here with the cart open.
        const back = /[?&]cart=1(&|$)/.test(location.search) ? location.pathname + location.search : location.pathname + (location.search ? location.search + '&' : '?') + 'cart=1';
        try { sessionStorage.setItem('bbv:after-login', back); sessionStorage.setItem('bbv:login-why', 'order'); } catch (_) {}
        location.href = '/login'; return;
      }
      const j = await r.json();
      if (!r.ok) throw new Error(j.error || 'That didn’t go through. Please try again.');
      const placedIds = new Set();
      for (const p of j.placed || []) { if (p.kind === 'vault') (p.ids || []).forEach((id) => placedIds.add('vault:' + id)); else placedIds.add('card:' + p.id); }
      lines = lines.filter((l) => !placedIds.has(l.kind + ':' + l.id));
      for (const f of j.failed || []) {
        if (f.kind === 'vault') {
          // Only the lines the vault is really short of are greyed out, each with its own reason. With no ids the
          // order as a whole was refused (too many cards, or orders already open): the toast below says why.
          (f.ids || []).forEach((id) => {
            const l = lines.find((x) => x.kind === 'vault' && x.id === id); if (!l) return;
            const d = (f.lines || []).find((x) => x.id === id);
            l.ok = false; l.why = (d && d.why) || f.error;
            // As refresh() does: the count comes down to what's left, so '+' never lowers it afterwards.
            if (d && typeof d.left === 'number') { l.left = d.left; if (d.left > 0 && (l.qty || 1) > d.left) l.qty = d.left; }
          });
        } else { const l = lines.find((x) => x.kind === 'card' && x.id === f.id); if (l) { l.ok = false; l.why = f.error; } }
      }
      if ((j.placed || []).length) note.value = '';
      save(); paint();
      // The message stays in the drawer (paint() isn't run again after it), with the way to the orders.
      if ((j.placed || []).length) {
        toast('Order sent ✓ You’ll see how to pay once it’s accepted.');
        const done = el('p', 'cart-done'); done.innerHTML = 'Order sent. Follow it under <a href="/me/orders?tab=buying">My orders</a>.'; list.prepend(done);
        // The seller's answer can reach this phone even with the site closed: offered here, once, if alerts are off.
        const ask = el('div', 'alerts-card'); ask.dataset.alerts = 'order'; ask.hidden = true; done.after(ask);
        if (window.bbvAlerts) window.bbvAlerts.paint();
      }
      if ((j.failed || []).length) toast((j.failed[0].error || 'Some lines could not be ordered.') + (j.failed.length > 1 ? ` (+${j.failed.length - 1} more)` : ''), true);
    } catch (err) { toast(err.message, true); paint(); }
  });

  badge();
  // Buttons that arrive later (a floating card, more cards loading in) get their "In cart" mark too.
  let mo; new MutationObserver(() => { clearTimeout(mo); mo = setTimeout(badge, 120); }).observe(document.body, { childList: true, subtree: true });
  if (/[?&]cart=1/.test(location.search)) open();
})();
