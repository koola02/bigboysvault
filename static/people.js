/* People in a window: who follows a trainer or a store, who reacted to a post (the owner's plan, 18 Nov 2026: a count
   you tap lists the people). And Follow on a trainer's page: the same switch as their store's Follow; whoever follows
   hears when they post or put up cards.
   - window.bbvPeople(title, url) opens the window from anywhere (feed.js uses it for a post's reactions).
   - [data-followers="url"] (with data-followers-title) opens it on a tap.
   - [data-follow-trainer="id"] is a Follow button; [data-follower-n="id"] shows how many follow them. */
(() => {
  const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const who = () => (window.bbvWho ? window.bbvWho() : Promise.resolve({}));
  const toast = (msg, bad) => { if (window.bbvToast) window.bbvToast(msg, bad); };
  const toLogin = () => {
    try { sessionStorage.setItem('bbv:after-login', location.pathname + location.search); sessionStorage.setItem('bbv:login-why', 'follow'); } catch (_) {}
    location.href = '/login';
  };
  async function call(url, method) {
    let r;
    const init = { method: method || 'GET', credentials: 'same-origin', headers: { Accept: 'application/json' } };
    if (init.method === 'POST') { init.headers['Content-Type'] = 'application/json'; init.body = '{}'; }
    try { r = await fetch(url, init); } catch (_) { throw new Error('No connection. Please try again.'); }
    const j = await r.json().catch(() => ({}));
    if (r.status === 401) { toLogin(); throw new Error('Please log in first.'); }
    if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
    return j;
  }

  /* ---------- the window ---------- */
  let open = null;
  function people(title, url) {
    if (open) open.close();
    const dlg = el('dialog', 'pp'); dlg.setAttribute('aria-label', title);
    const head = el('header', 'pp-h'); head.append(el('b', '', title));
    const x = el('button', 'ibtn pp-x', '✕'); x.type = 'button'; x.setAttribute('aria-label', 'Close'); head.append(x);
    const state = el('p', 'pp-state', 'Loading…'), list = el('ul', 'pp-list');
    dlg.append(head, state, list); document.body.append(dlg);
    const back = document.activeElement;
    const close = () => { if (!dlg.isConnected) return; dlg.close(); dlg.remove(); if (open === api) open = null; if (back && back.isConnected) back.focus({ preventScroll: true }); };
    const api = { close };
    open = api;
    x.addEventListener('click', close);
    dlg.addEventListener('click', (e) => { if (e.target === dlg) close(); });
    dlg.addEventListener('cancel', (e) => { e.preventDefault(); close(); });
    dlg.showModal();
    call(url).then((j) => {
      const ppl = j.people || [];
      state.hidden = ppl.length > 0; state.textContent = ppl.length ? '' : 'Nobody yet.';
      list.replaceChildren(...ppl.map((p) => {
        const li = el('li'), a = el('a', 'pp-row'); a.href = p.url;
        const av = el('span', 'av sm');
        if (p.avatar) { const i = el('img'); i.src = p.avatar; i.alt = ''; i.loading = 'lazy'; av.append(i); } else av.textContent = p.initial || '•';
        a.append(av, el('span', 'pp-name', p.name));
        if (p.emoji) a.append(el('span', 'pp-emoji', p.emoji));
        li.append(a); return li;
      }));
    }).catch((err) => { state.textContent = err.message; });
  }
  window.bbvPeople = people;
  document.addEventListener('click', (e) => {
    const b = e.target.closest && e.target.closest('[data-followers]'); if (!b) return;
    e.preventDefault(); people(b.dataset.followersTitle || 'Followers', b.dataset.followers);
  });

  /* ---------- Follow on a trainer's page, and how many follow them ---------- */
  const count = (id, n) => document.querySelectorAll(`[data-follower-n="${id}"]`).forEach((c) => {
    c.textContent = n;
    const box = c.closest('[data-followers]'); if (box) { box.hidden = false; const w = box.querySelector('[data-follower-w]'); if (w) w.textContent = n === 1 ? 'Follower' : 'Followers'; }
  });
  const seen = new Set();
  document.querySelectorAll('[data-follow-trainer]').forEach((btn) => {
    const id = btn.dataset.followTrainer; seen.add(id);
    const paint = (on, n) => { btn.textContent = on ? 'Following ✓' : 'Follow'; btn.classList.toggle('ghost', on); btn.setAttribute('aria-pressed', String(on)); if (n != null) count(id, n); };
    call(`/trainers/${id}/follow`).then((j) => paint(j.following, j.followers)).catch(() => {});
    btn.addEventListener('click', async () => {
      const w = await who(); if (!w || !w.member) { toLogin(); return; }
      btn.disabled = true;
      try { const j = await call(`/trainers/${id}/follow`, 'POST'); paint(j.following, j.followers); toast(j.following ? 'You follow them now ✓ You hear when they post or put up cards.' : 'Unfollowed'); }
      catch (err) { toast(err.message, true); }
      btn.disabled = false;
    });
  });
  // Your own page has no Follow button: the count still shows.
  document.querySelectorAll('[data-follower-n]').forEach((n) => {
    const id = n.dataset.followerN; if (seen.has(id)) return; seen.add(id);
    call(`/trainers/${id}/follow`).then((j) => count(id, j.followers)).catch(() => {});
  });
})();
