// The top of every store page, the vault's and a trainer's alike: Follow, the Vouches window, and the border colour
// a store's owner picks in "Edit store" (a full admin, for the vault's). Loaded by store.html and store_vault.html.
(() => {
  const root = document.getElementById('storefront'); if (!root) return;
  const slug = root.dataset.slug, name = root.dataset.name || 'this store';
  const make = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const button = (cls, text, fn) => { const b = make('button', cls, text); b.type = 'button'; if (fn) b.addEventListener('click', fn); return b; };
  const X_ICON = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>';

  const toast = (msg, bad) => {
    const t = document.getElementById('toast');
    if (!t) return;
    (document.querySelector('dialog[open]:last-of-type') || document.body).append(t); // above any open window
    t.textContent = msg; t.className = 'toast on ' + (bad ? 'err' : 'ok'); t.hidden = false;
    clearTimeout(toast.t); toast.t = setTimeout(() => { t.hidden = true; t.classList.remove('on'); }, bad ? 5000 : 2600);
  };
  async function send(url, body, method = 'POST') {
    let r;
    const opts = { method, credentials: 'same-origin', headers: { Accept: 'application/json' } };
    if (method === 'POST') { opts.headers['Content-Type'] = 'application/json'; opts.body = JSON.stringify(body ?? {}); }
    try { r = await fetch(url, opts); } catch (_) { throw new Error('No connection. Please try again.'); }
    const j = await r.json().catch(() => ({}));
    if (r.status === 401) { signIn('follow'); throw new Error('Please log in first.'); }
    if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
    return j;
  }
  // To the log-in page, which says why and brings the visitor back to this store afterwards.
  function signIn(why) {
    try { sessionStorage.setItem('bbv:after-login', location.pathname + location.search); sessionStorage.setItem('bbv:login-why', why); } catch (_) {}
    location.href = '/login';
  }
  // One answer per page about who is looking, shared with the other scripts.
  const who = () => (window.bbvWho ? window.bbvWho() : (window.bbvWhoAnswer ||= fetch('/me/who', { credentials: 'same-origin', headers: { Accept: 'application/json' } }).then((r) => (r.ok ? r.json() : {})).catch(() => ({}))));

  /* ----- Follow ----- */
  const followBtn = document.getElementById('followBtn');
  const paintFollow = (on, n) => {
    followBtn.textContent = on ? 'Following ✓' : 'Follow';
    followBtn.classList.toggle('ghost', on);
    followBtn.setAttribute('aria-pressed', String(on));
    const count = document.getElementById('followerN');
    if (count && n != null) count.textContent = n;
  };
  if (followBtn) {
    // The vault's page is the same for everyone, so whether this visitor follows it is asked once it is open.
    // A trainer's store page already says so.
    if (slug === 'vault') who().then((j) => { if (j && j.member) send(`/store/${slug}/follow`, null, 'GET').then((s) => paintFollow(s.following, s.followers)).catch(() => {}); });
    followBtn.addEventListener('click', async () => {
      const j0 = await who();
      if (!j0 || !j0.member) return signIn('follow');
      followBtn.disabled = true;
      try {
        const j = await send(`/store/${slug}/follow`);
        paintFollow(j.following, j.followers);
        toast(j.following ? 'You follow this store now ✓' : 'Unfollowed');
      } catch (err) { toast(err.message, true); }
      followBtn.disabled = false;
    });
  }

  /* ----- the Vouches window ----- */
  async function vouches(btn) {
    const d = make('dialog', 'float sv');
    d.setAttribute('aria-label', `Vouches for ${name}`);
    const panel = make('div', 'float-panel sv-panel'); panel.tabIndex = -1;
    const head = make('header', 'sv-h');
    const titles = make('div', 'sv-t');
    titles.append(make('h2', null, 'Vouches'), make('p', null, name));
    const x = button('sv-x', null, () => close()); x.setAttribute('aria-label', 'Close'); x.innerHTML = X_ICON;
    head.append(titles, x);
    const body = make('div', 'sv-scroll'); body.append(make('p', 'sv-wait', 'Loading…'));
    panel.append(head, body); d.append(panel); document.body.append(d);
    const close = () => {
      const t = document.getElementById('toast'); if (t && d.contains(t)) document.body.append(t);
      d.close(); d.remove(); btn.focus({ preventScroll: true });
    };
    // Escape closes a photo opened big inside the window first (fb-shell.js does that), then the window.
    d.addEventListener('cancel', (e) => { e.preventDefault(); if (!d.querySelector('.dm-lightbox')) close(); });
    let downOutside = false;
    d.addEventListener('pointerdown', (e) => { downOutside = e.target === d; });
    d.addEventListener('click', (e) => { if (downOutside && e.target === d) close(); downOutside = false; });
    d.showModal(); panel.focus({ preventScroll: true });

    async function fill() {
      let html;
      try {
        const r = await fetch(`/store/${slug}/vouches`, { credentials: 'same-origin' });
        if (!r.ok) throw new Error();
        html = await r.text();
      } catch (_) { body.replaceChildren(make('p', 'sv-wait', 'The vouches couldn’t load just now. Close this and try again.')); return; }
      body.innerHTML = html; // the site's own page part
      const box = body.querySelector('.sv-body');
      // vouches.js puts Reply, Hide and Delete on each vouch, as it does everywhere else.
      document.dispatchEvent(new CustomEvent('float:filled', { detail: body }));
      const write = box && box.querySelector('[data-sv-write]');
      if (!write) return;
      const store = Number(box.dataset.store);
      const j = await who();
      const me = j && j.member;
      // Anyone may vouch for a store but the person who runs it.
      write.hidden = !!(me && store !== 0 && me.id === store);
      write.querySelector('[data-sv-new]').addEventListener('click', () => {
        if (!me) return signIn('vouch');
        if (!window.bbvVouch) { toast('Please reload the page and try again.', true); return; }
        window.bbvVouch.write({ store, name: box.dataset.name, grid: box.querySelector('.sv-list') });
      });
    }
    // The store's owner answers a review right here.
    body.addEventListener('submit', async (e) => {
      const f = e.target.closest('[data-review-reply]'); if (!f) return;
      e.preventDefault();
      try { await send(`/me/reviews/${f.dataset.reviewReply}/reply`, { text: f.querySelector('input').value }); await fill(); toast('Reply posted ✓'); }
      catch (err) { toast(err.message, true); }
    });
    fill();
  }
  root.addEventListener('click', (e) => {
    const b = e.target.closest('[data-vouches]'); if (!b) return;
    vouches(b);
  });
  // Arriving from "… left you a review" (the bell): the Vouches window, where reviews are now.
  if (location.hash === '#vouches') { const b = root.querySelector('[data-vouches]'); if (b) { history.replaceState(null, '', location.pathname + location.search); vouches(b); } }

  /* ----- the border colour ----- */
  // Every store starts gold; any colour that shows up on the dark page can be picked. The server checks the same rule.
  const BORDERS = [['Gold', ''], ['Silver', '#c0c6d4'], ['Bronze', '#c98a55'], ['Red', '#e5534b'], ['Orange', '#f0883e'], ['Green', '#46c37b'], ['Teal', '#2bb6a8'], ['Blue', '#4c8dff'], ['Purple', '#a371f7'], ['Pink', '#f778ba']];
  const GOLD = '#efc873';
  const lum = (hex) => {
    const ch = (i) => { const x = parseInt(hex.slice(i, i + 2), 16) / 255; return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4; };
    return 0.2126 * ch(1) + 0.7152 * ch(3) + 0.0722 * ch(5);
  };
  const shows = (hex) => !hex || ((lum(hex) + 0.05) / (lum('#0b0d14') + 0.05)) >= 2;
  // A row of colour buttons, plus "Other colour". Picking one shows it on the store's header straight away.
  function picker(value) {
    const head = document.querySelector('.sf-head');
    const was = head ? head.style.getPropertyValue('--sborder') : '';
    let current = (value || '').toLowerCase();
    const wrap = make('div', 'bp');
    wrap.append(make('p', 'bp-l', 'Border colour'));
    const row = make('div', 'bp-row'); row.setAttribute('role', 'radiogroup'); row.setAttribute('aria-label', 'Border colour');
    const note = make('small', 'bp-note', 'Shown around your store here and on the Stores page.');
    const custom = make('label', 'bp-sw bp-other');
    const input = make('input'); input.type = 'color'; input.value = current || GOLD; input.setAttribute('aria-label', 'Other colour');
    custom.append(input, make('span', null, 'Other'));
    const swatches = BORDERS.map(([label, hex]) => {
      const b = button('bp-sw', null, () => set(hex));
      b.setAttribute('role', 'radio'); b.setAttribute('aria-label', label); b.title = label;
      b.style.setProperty('--c', hex || GOLD); b.dataset.hex = hex;
      return b;
    });
    row.append(...swatches, custom);
    function set(hex) {
      hex = (hex || '').toLowerCase();
      if (!shows(hex)) { note.textContent = 'That colour is too dark to see on the page. Pick a lighter one.'; note.classList.add('bad'); return; }
      note.textContent = 'Shown around your store here and on the Stores page.'; note.classList.remove('bad');
      current = hex;
      const preset = BORDERS.some(([, h]) => h === hex);
      swatches.forEach((b) => b.setAttribute('aria-checked', String(b.dataset.hex === hex)));
      custom.classList.toggle('on', !preset); if (!preset) custom.style.setProperty('--c', hex);
      if (head) head.style.setProperty('--sborder', hex || 'var(--gold-edge)');
    }
    input.addEventListener('input', () => set(input.value));
    wrap.append(row, note);
    set(current);
    return { node: wrap, value: () => current, undo: () => { if (head) { if (was) head.style.setProperty('--sborder', was); else head.style.removeProperty('--sborder'); } } };
  }
  window.bbvBorder = { picker };

  // (The vault's store is set up in Vault Manager › Store settings, /admin/store, like a seller's Store settings.)
})();
