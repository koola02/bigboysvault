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
    if (r.status === 401) { signIn(); throw new Error('Please log in first.'); }
    if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
    return j;
  }
  // To the log-in page, which says why and brings the buyer back to this store afterwards.
  function signIn() { try { sessionStorage.setItem('bbv:after-login', location.pathname + location.search); sessionStorage.setItem('bbv:login-why', 'offer'); } catch (_) {} location.href = '/login'; }

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

  /* (Follow and Vouches are store-head.js's, the same on the vault's store page.) */

  /* (Bookmarking a card is bookmarks.js's, the same on every page.) */

  /* ----- the ask sheet (Offer and Message are in each card's window: offer.js, dm.js) ----- */
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
  /* ----- ask the seller ----- */
  const askSheet = $('askSheet'), askForm = $('askForm');
  $('askBtn')?.addEventListener('click', () => { if (!me) return signIn(); open(askSheet); });
  if (location.hash === '#ask' && me && askSheet) open(askSheet);
  askForm?.addEventListener('submit', async (e) => {
    e.preventDefault();
    try { await send(`/store/${slug}/ask`, { message: askForm.message.value }); close(askSheet); askForm.reset(); toast('Sent ✓ The reply shows in My orders and the bell.'); }
    catch (err) { toast(err.message, true); }
  });

  /* (Replies to reviews are written in the Vouches window: store-head.js.) */

  /* (A binder on its own page, new look: its X and Escape are fb-shell.js's, the same for the vault's binders.) */
})();
