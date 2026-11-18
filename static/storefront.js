// A seller's public store: find a card, order it or make an offer, follow, save.
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

  /* (Asking the seller is Message, in each card's window and on the store's header: dm.js. Offers: offer.js.) */

  /* (Replies to reviews are written in the Vouches window: store-head.js.) */

  /* (A binder on its own page, new look: its X and Escape are fb-shell.js's, the same for the vault's binders.) */
})();
