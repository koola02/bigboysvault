// A seller's public store: find a card, reserve it or make an offer, ask the seller, follow, save.
(() => {
  const root = document.getElementById('storefront'); if (!root) return;
  const $ = (id) => document.getElementById(id);
  const slug = root.dataset.slug, me = root.dataset.me;
  const toast = (msg, bad) => {
    const t = $('toast'); if (!t) return;
    t.textContent = msg; t.className = 'toast on ' + (bad ? 'err' : 'ok'); t.hidden = false;
    clearTimeout(toast.t); toast.t = setTimeout(() => { t.hidden = true; t.classList.remove('on'); }, bad ? 5000 : 2600);
  };
  async function send(url, body) {
    let r;
    try { r = await fetch(url, { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify(body ?? {}) }); }
    catch (_) { throw new Error('No connection. Please try again.'); }
    const j = await r.json().catch(() => ({}));
    if (r.status === 401) { location.href = '/login'; throw new Error('Please sign in first.'); }
    if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
    return j;
  }
  const signIn = () => { location.href = '/login'; };

  /* ----- find a card in this store ----- */
  const grid = $('sfGrid');
  if (grid) {
    const cards = [...grid.querySelectorAll('.sf-card')];
    const norm = (s) => (s || '').toLowerCase();
    function paint() {
      const q = norm($('sfQ').value).split(/\s+/).filter(Boolean), cond = $('sfCond').value, sort = $('sfSort').value;
      let shown = cards.filter((c) => {
        const hay = norm(c.dataset.title + ' ' + c.dataset.set + ' ' + c.dataset.number + ' ' + c.dataset.cond);
        if (!q.every((w) => hay.includes(w))) return false;
        if (cond === 'Graded') return c.hasAttribute('data-graded');
        return !cond || c.dataset.cond === cond;
      });
      const by = { 'price-asc': (a, b) => +a.dataset.price - +b.dataset.price, 'price-desc': (a, b) => +b.dataset.price - +a.dataset.price, title: (a, b) => a.dataset.title.localeCompare(b.dataset.title), newest: (a, b) => +b.dataset.id - +a.dataset.id }[sort] || ((a, b) => +a.dataset.pos - +b.dataset.pos);
      shown = shown.sort(by);
      cards.forEach((c) => c.hidden = true);
      shown.forEach((c) => { c.hidden = false; grid.append(c); });
      $('sfN').textContent = shown.length;
    }
    ['sfQ', 'sfCond', 'sfSort'].forEach((id) => $(id).addEventListener('input', paint));
  }

  /* ----- follow ----- */
  $('followBtn')?.addEventListener('click', async (e) => {
    if (!me) return signIn();
    const b = e.currentTarget; b.disabled = true;
    try { const j = await send(`/store/${slug}/follow`); b.textContent = j.following ? 'Following ✓' : 'Follow'; b.classList.toggle('ghost', j.following); b.setAttribute('aria-pressed', String(j.following)); $('followerN').textContent = j.followers; toast(j.following ? 'You follow this store now ✓' : 'Unfollowed'); }
    catch (err) { toast(err.message, true); }
    b.disabled = false;
  });

  /* ----- save a card ----- */
  root.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-save]'); if (!b) return;
    if (!me) return signIn();
    const card = b.closest('.sf-card');
    try { const j = await send(`/api/cards/${card.dataset.id}/save`); b.setAttribute('aria-pressed', String(j.saved)); card.toggleAttribute('data-saved', j.saved); toast(j.saved ? 'Saved ♥' : 'Unsaved'); }
    catch (err) { toast(err.message, true); }
  });

  /* ----- reserve or make an offer ----- */
  const sheet = $('buySheet'), form = $('buyForm');
  let mode = 'reserve', cardId = 0;
  // A sheet slides up on phones and sits in the middle on big screens (the "open" class does that).
  // While it's open, Tab stays inside it, and closing it puts the keyboard back on the button that opened it.
  let back = null;
  const focusables = (s) => [...s.querySelectorAll('.sheet-panel input, .sheet-panel textarea, .sheet-panel select, .sheet-panel button, .sheet-panel a[href]')]
    .filter((n) => !n.disabled && !n.closest('[hidden]') && n.getClientRects().length > 0);
  const open = (s) => {
    if (!s) return;
    clearTimeout(s._t);
    back = document.activeElement;
    s.hidden = false; document.body.style.overflow = 'hidden';
    requestAnimationFrame(() => {
      s.classList.add('open');
      const first = focusables(s).find((n) => n.matches('input, textarea, select')) || focusables(s)[0];
      first?.focus({ preventScroll: true });
    });
  };
  const close = (s) => {
    if (!s || s.hidden || !s.classList.contains('open')) return;
    s.classList.remove('open'); document.body.style.overflow = '';
    s._t = setTimeout(() => { s.hidden = true; }, 320);
    if (back && back.isConnected) back.focus({ preventScroll: true });
    back = null;
  };
  document.querySelectorAll('[data-close]').forEach((n) => n.addEventListener('click', () => close(n.closest('.sheet'))));
  document.addEventListener('keydown', (e) => {
    const s = document.querySelector('.sheet.open'); if (!s) return;
    if (e.key === 'Escape') { e.preventDefault(); close(s); return; }
    if (e.key !== 'Tab') return;
    const f = focusables(s); if (!f.length) return;
    const first = f[0], last = f[f.length - 1];
    if (!s.contains(document.activeElement)) { e.preventDefault(); first.focus(); }
    else if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  });
  root.addEventListener('click', (e) => {
    const b = e.target.closest('[data-offer]'); if (!b) return;
    if (!me) return signIn();
    const card = b.closest('.sf-card');
    mode = 'offer'; cardId = Number(card.dataset.id);
    $('buyTitle').textContent = mode === 'reserve' ? 'Reserve this card' : 'Make an offer';
    $('buyImg').src = card.querySelector('img')?.src || ''; $('buyName').textContent = card.dataset.title;
    $('buyMeta').textContent = `${card.dataset.cond} · ${Number(card.dataset.price) > 0 ? '₱' + Math.round(card.dataset.price).toLocaleString('en-US') : 'no price yet'} · ${card.dataset.qty} available`;
    $('buyAmountRow').hidden = mode !== 'offer'; form.amount.required = mode === 'offer';
    form.qty.max = card.dataset.qty; form.qty.value = 1; form.message.value = ''; form.amount.value = '';
    $('buyHint').textContent = mode === 'reserve' ? 'The seller gets your reservation and accepts it. Then you’ll see how to pay them. The vault never handles the money.' : 'The seller can accept, counter or decline. If they accept, it becomes a reservation at your price.';
    $('buyGo').textContent = mode === 'reserve' ? 'Reserve' : 'Send offer';
    open(sheet);
  });
  form.addEventListener('submit', async (e) => {
    e.preventDefault(); const go = $('buyGo'); go.disabled = true;
    try {
      const body = { card_id: cardId, qty: Number(form.qty.value) || 1, message: form.message.value };
      if (mode === 'offer') body.amount = Number(form.amount.value);
      await send(`/store/${slug}/${mode}`, body);
      close(sheet);
      toast(mode === 'reserve' ? 'Reserved ✓ Watch the bell for the seller’s answer.' : 'Offer sent ✓ Watch the bell for the seller’s answer.');
      setTimeout(() => location.reload(), 1200);
    } catch (err) { toast(err.message, true); }
    go.disabled = false;
  });

  /* ----- ask the seller ----- */
  const askSheet = $('askSheet'), askForm = $('askForm');
  $('askBtn')?.addEventListener('click', () => { if (!me) return signIn(); open(askSheet); });
  if (location.hash === '#ask' && me && askSheet) open(askSheet);
  askForm?.addEventListener('submit', async (e) => {
    e.preventDefault();
    try { await send(`/store/${slug}/ask`, { message: askForm.message.value }); close(askSheet); askForm.reset(); toast('Sent ✓ The reply shows in My orders and the bell.'); }
    catch (err) { toast(err.message, true); }
  });

  /* ----- the seller replies to a review ----- */
  root.addEventListener('submit', async (e) => {
    const f = e.target.closest('[data-review-reply]'); if (!f) return;
    e.preventDefault();
    try { await send(`/me/reviews/${f.dataset.reviewReply}/reply`, { text: f.querySelector('input').value }); location.reload(); } catch (err) { toast(err.message, true); }
  });

  /* ----- a binder on its own page (new look): the X, or Escape, goes back to the store ----- */
  const bx = document.querySelector('[data-binder-close]');
  if (bx) {
    // Came here from this store's page? Then step back to it, so it opens where it was scrolled to. Otherwise the X is a plain link.
    const fromStore = () => {
      try {
        const r = new URL(document.referrer), s = new URL(bx.href, location.href), path = (u) => u.pathname.replace(/\/+$/, '');
        return history.length > 1 && r.origin === location.origin && path(r) === path(s);
      } catch (_) { return false; }
    };
    const leave = () => { if (fromStore()) history.back(); else location.href = bx.href; };
    bx.addEventListener('click', (e) => {
      if (e.button || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return; // a new tab or window: the link does that
      e.preventDefault(); leave();
    });
    // Escape too, unless something is open on top (a sheet, a chat, a menu, the cart) or someone is typing.
    const busy = () => {
      const a = document.activeElement, on = document.documentElement.classList;
      if (a && (a.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName) || a.closest('[role="dialog"], dialog, .hd, .cart'))) return true;
      if (['menu-open', 'search-open', 'cardwin-open', 'sheet-open'].some((c) => on.contains(c)) || document.body.classList.contains('cart-open')) return true;
      return !!document.querySelector('.sheet:not([hidden]), .cardwin:not([hidden]), dialog[open], .dm-lightbox, .cc, .dm-people, .dm-call-ui, [aria-expanded="true"]');
    };
    // Listened for before anything else hears the key, so a sheet or menu that Escape is about to close still counts as open.
    addEventListener('keydown', (e) => { if (e.key === 'Escape' && !e.defaultPrevented && !e.repeat && !busy()) leave(); }, true);
  }
})();
