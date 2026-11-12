/* The new look's frame: who is signed in, the tabs that show where you are, the menu, the account menu,
   notifications, the phone's search bar, and the switches for the look itself.
   (Today's look uses vault-core.js for the same jobs; a page loads one or the other.) */

// Who's here: asked once per page, shared by every script that needs it.
window.bbvWho = window.bbvWho || (() => (window.bbvWhoAnswer ||= fetch('/me/who', { credentials: 'same-origin', headers: { Accept: 'application/json' } })
  .then((r) => (r.ok ? r.json() : {})).catch(() => ({}))));

(() => {
  const root = document.documentElement;
  const $ = (id) => document.getElementById(id);

  /* ---------- where you are ---------- */
  const here = location.pathname;
  const match = (h) => h === here || (h !== '/' && here.startsWith(h + '/')) || (h === '/stores' && here.startsWith('/store/'))
    || (h === '/all' && /^\/(search|collections|products)(\/|$)/.test(here));
  document.querySelectorAll('.hd-tabs a, .bt a, .fb-left .mn-i').forEach((a) => { if (match(a.getAttribute('href'))) a.setAttribute('aria-current', 'page'); });

  /* ---------- who is signed in ---------- */
  const face = (node, me, admin) => {
    if (!node) return;
    node.replaceChildren();
    if (me && me.avatar) { const i = document.createElement('img'); i.src = me.avatar; i.alt = ''; node.append(i); }
    else node.textContent = me ? (me.initial || me.name[0].toUpperCase()) : admin ? '★' : '';
  };
  const paint = (me, admin) => {
    const on = !!(me && me.name) || admin;
    if (on) root.dataset.me = (me && me.role === 'admin') || admin ? 'admin' : (me && me.role === 'mod') ? 'admin' : '1'; else delete root.dataset.me;
    root.toggleAttribute('data-seller', !!(me && me.seller));
    root.toggleAttribute('data-full', !!admin);
    const chip = $('barMe'); if (chip) chip.hidden = !on;
    ['barMeAv', 'mnMeAv', 'meMenuAv'].forEach((id) => face($(id), me, admin));
    const name = me ? me.name : admin ? 'Admin' : '';
    ['mnMeT', 'meMenuT'].forEach((id) => { const n = $(id); if (n && name) n.textContent = name; });
    const login = $('barLogin'); if (login) login.hidden = on;
    // The owner signed in with the admin password alone has no trainer page: "my profile" goes to Admin.
    if (!me && admin) document.querySelectorAll('#mnMe, #meMenu a[href="/me"]').forEach((a) => { a.href = '/admin'; });
  };
  let last = null;
  try { last = JSON.parse(localStorage.getItem('bbv:me') || 'null'); } catch (_) {}
  if (last) paint(last.me, last.admin);
  window.bbvWho().then((j) => {
    if (!j || !('member' in j)) return;
    const me = j.member || null, admin = !!j.full;
    paint(me, admin || (!!j.admin && !me));
    root.toggleAttribute('data-full', admin);
    // Things only their owner should see (an Edit button on your own listing, say).
    document.querySelectorAll('[data-owner-only]').forEach((n) => { n.hidden = !(me && String(me.id) === n.dataset.ownerOnly); });
    if (admin) pencils();
    try { if (me || admin) localStorage.setItem('bbv:me', JSON.stringify({ me, admin })); else localStorage.removeItem('bbv:me'); } catch (_) {}
  });

  /* ---------- the site's name in the phone header: it shrinks until it fits beside the buttons, never cut ---------- */
  const word = document.querySelector('.hd-word');
  const fitName = () => {
    if (!word) return;
    word.style.fontSize = '';
    if (getComputedStyle(word).display === 'none') return;
    let px = parseFloat(getComputedStyle(word).fontSize);
    while (word.scrollWidth > word.clientWidth + 0.5 && px > 10.5) { px -= 0.5; word.style.fontSize = `${px}px`; }
  };
  fitName();
  addEventListener('resize', fitName);
  // The buttons beside it change when the site learns who is signed in, and the letters when the font arrives.
  const hdRight = document.querySelector('.hd-r');
  if (hdRight && window.ResizeObserver) new ResizeObserver(fitName).observe(hdRight);
  if (document.fonts && document.fonts.ready) document.fonts.ready.then(fitName);

  /* ---------- things that open: the menu, the account menu, the phone's search ---------- */
  const menuBtns = () => document.querySelectorAll('[data-menu-toggle]');
  const setMenu = (open) => { root.classList.toggle('menu-open', open); menuBtns().forEach((b) => b.setAttribute('aria-expanded', String(open))); };
  const mePop = $('meMenu'), meBtn = $('barMe');
  const setMe = (open) => { if (!mePop) return; mePop.hidden = !open; meBtn?.setAttribute('aria-expanded', String(open)); };
  document.addEventListener('click', (e) => {
    const t = e.target;
    if (t.closest('[data-menu-toggle]')) { setMenu(!root.classList.contains('menu-open')); setMe(false); return; }
    if (root.classList.contains('menu-open') && (!t.closest('#fbMenu') || t.closest('#fbMenu a'))) setMenu(false);
    if (t.closest('#barMe')) { setMe(mePop.hidden); return; }
    if (mePop && !mePop.hidden && !t.closest('#meMenu')) setMe(false);
    if (t.closest('[data-search-open]')) { root.classList.add('search-open'); document.querySelector('.hd-search input')?.focus(); return; }
    if (t.closest('[data-search-close]')) root.classList.remove('search-open');
  });
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    if (root.classList.contains('menu-open')) setMenu(false);
    if (mePop && !mePop.hidden) { setMe(false); meBtn?.focus(); }
    root.classList.remove('search-open');
  });
  // "/" or Ctrl+K jumps to the search box, as on today's site.
  document.addEventListener('keydown', (e) => {
    if ((e.key === 'k' && (e.ctrlKey || e.metaKey)) || (e.key === '/' && !/^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || '') && !document.activeElement?.isContentEditable)) {
      e.preventDefault(); root.classList.add('search-open'); document.querySelector('.hd-search input')?.focus();
    }
  });

  /* ---------- notifications ---------- */
  const btn = $('notifBtn'), menu = $('notifMenu'), badge = $('notifBadge'), list = $('notifList'), holder = $('notifDropdown'), tabBadge = $('btBellN');
  const ago = (t) => {
    const s = Math.max(0, Math.floor(Date.now() / 1000 - (Number(t) || 0)));
    if (s < 60) return 'Just now'; if (s < 3600) return `${Math.floor(s / 60)} min ago`; if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    if (s < 7 * 86400) return `${Math.floor(s / 86400)} d ago`;
    return new Date(Number(t) * 1000).toLocaleDateString('en-PH', { day: 'numeric', month: 'short' });
  };
  const count = (n) => [badge, tabBadge].forEach((b) => { if (!b) return; b.hidden = !n; b.textContent = n > 99 ? '99+' : String(n); });
  async function load() {
    try {
      const r = await fetch('/api/notifications', { credentials: 'same-origin', headers: { Accept: 'application/json' } });
      if (!r.ok) { if (holder) holder.hidden = true; return; }
      const j = await r.json();
      if (holder) holder.hidden = false;
      count(j.unread_count || 0);
      if (!list) return;
      list.replaceChildren();
      if (!j.notifications || !j.notifications.length) { const p = document.createElement('p'); p.className = 'notif-empty'; p.textContent = 'No notifications yet.'; list.append(p); return; }
      j.notifications.forEach((n) => {
        // Messages can hold what other people typed: text only, never HTML.
        const a = document.createElement('a'); a.className = 'notif-item' + (n.is_read ? '' : ' unread'); a.href = n.link || '#';
        const k = document.createElement('div'); k.className = 'notif-k'; k.textContent = (n.kind || '').replace('my_order', 'my order').replace('dm_watch', 'message');
        const m = document.createElement('div'); m.className = 'notif-m'; m.textContent = n.message || '';
        const t = document.createElement('div'); t.className = 'notif-t'; t.textContent = ago(n.created_at);
        a.append(k, m, t); list.append(a);
      });
    } catch (_) {}
  }
  btn?.addEventListener('click', (e) => { e.stopPropagation(); const open = menu.hidden; menu.hidden = !open; btn.setAttribute('aria-expanded', String(open)); if (open) load(); });
  document.addEventListener('click', (e) => { if (menu && !menu.hidden && !holder.contains(e.target)) { menu.hidden = true; btn.setAttribute('aria-expanded', 'false'); } });
  $('notifRead')?.addEventListener('click', async (e) => {
    e.stopPropagation();
    try { await fetch('/api/notifications/read', { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' }, body: '{}' }); count(0); list.querySelectorAll('.unread').forEach((n) => n.classList.remove('unread')); } catch (_) {}
  });
  window.bbvWho().then((j) => { if (j && j.member) load(); else if (holder) holder.hidden = true; });

  /* ---------- full admins: a pencil on every vault card, straight to that card's editor ---------- */
  let penWatch = null, penSoon = 0;
  function pencils() {
    if (!root.hasAttribute('data-full')) return;
    document.querySelectorAll('a.card[href^="/products/"]:not([data-pen])').forEach((a) => {
      a.dataset.pen = '1';
      const b = document.createElement('span'); b.className = 'card-pen staff-only'; b.setAttribute('role', 'link'); b.tabIndex = 0; b.title = 'Edit this card';
      b.setAttribute('aria-label', `Edit ${a.querySelector('.card-t')?.textContent || 'this card'}`);
      b.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/></svg>';
      (a.querySelector('.card-img') || a).append(b);
    });
    // Cards that arrive later (more results, a new search) get theirs too.
    if (!penWatch) { penWatch = new MutationObserver(() => { if (!penSoon) penSoon = setTimeout(() => { penSoon = 0; pencils(); }, 150); }); penWatch.observe(document.body, { childList: true, subtree: true }); }
  }
  const toEditor = (e) => { const p = e.target.closest && e.target.closest('.card-pen'); if (!p) return false; e.preventDefault(); e.stopPropagation(); location.href = `${p.closest('a.card').getAttribute('href')}#edit`; return true; };
  document.addEventListener('click', toEditor, true);
  document.addEventListener('keydown', (e) => { if (e.key === 'Enter') toEditor(e); }, true);

  /* ---------- Ask the Vault and the vault chat: chat windows, like a message, not pages of their own ---------- */
  const TALK = {
    ask: { title: 'Ask the Vault', sub: 'Prices, stock and ideas', part: '/ask?part=1', page: '/ask', mark: '[data-ask]' },
    chat: { title: 'Vault chat', sub: 'Everyone’s here', part: '/chat?part=1', page: '/chat', mark: '[data-chat]' },
  };
  const talks = {};
  const talkBox = (kind) => (talks[kind]?.win || document).querySelector(`${TALK[kind].mark} [data-text]`);
  function askSay(q) { const box = talkBox('ask'); if (!box || !q) return; box.value = q; box.closest('form').requestSubmit(); }
  function closeTalk(kind) {
    const t = talks[kind]; if (!t) return;
    // The vault chat counts who is here, so closing it really leaves; Ask keeps its conversation for next time.
    if (kind === 'chat') { t.win.remove(); delete talks[kind]; } else t.win.hidden = true;
    if (!Object.values(talks).some((x) => !x.win.hidden)) root.classList.remove('ask-open');
  }
  function openTalk(kind, q) {
    const k = TALK[kind];
    // On the chat's own page it is already there.
    if (!talks[kind] && document.querySelector(`.talk-page ${k.mark}`)) { talkBox(kind)?.focus(); if (kind === 'ask') askSay(q); return; }
    Object.keys(talks).forEach((other) => { if (other !== kind) closeTalk(other); });
    let t = talks[kind];
    if (!t) {
      const win = document.createElement('section'); win.className = 'ask-win'; win.dataset.kind = kind; win.setAttribute('role', 'dialog'); win.setAttribute('aria-label', k.title);
      win.innerHTML = `<header class="ask-win-h"><img src="/static/img/logo-sm-144.png?v=1" width="36" height="36" alt=""><span><b>${k.title}</b><small>${k.sub}</small></span><button type="button" class="ibtn" data-talk-x aria-label="Close ${k.title}">✕</button></header><div class="ask-win-b"><p class="ask-win-wait">Opening…</p></div>`;
      document.body.append(win);
      win.querySelector('[data-talk-x]').addEventListener('click', () => closeTalk(kind));
      // The same chat its page has (the server sends just that part), started by the chat's own script.
      const ready = fetch(k.part, { credentials: 'same-origin', headers: { Accept: 'text/html' } }).then((r) => { if (!r.ok) throw new Error(); return r.text(); })
        .then((html) => { win.querySelector('.ask-win-b').innerHTML = html; document.dispatchEvent(new Event('float:filled')); })
        .catch(() => { location.href = k.page; });
      t = talks[kind] = { win, ready };
    }
    t.win.hidden = false; root.classList.add('ask-open');
    t.ready.then(() => { if (matchMedia('(pointer: fine)').matches || q) talkBox(kind)?.focus({ preventScroll: true }); if (kind === 'ask') askSay(q); });
  }
  document.addEventListener('click', (e) => {
    const a = e.target.closest && e.target.closest('a[href="/ask"], a[href^="/ask?"], a[href="/chat"]');
    if (!a || e.defaultPrevented || e.button || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
    e.preventDefault(); setMenu(false);
    const kind = a.getAttribute('href').startsWith('/chat') ? 'chat' : 'ask';
    let q = ''; try { q = new URL(a.href, location.href).searchParams.get('q') || ''; } catch (_) {}
    openTalk(kind, q);
  });
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    Object.keys(talks).forEach((kind) => { const w = talks[kind].win; if (!w.hidden && w.contains(document.activeElement)) closeTalk(kind); });
  });

  /* ---------- a card's picture, up close (today's look does this inside its floating window) ---------- */
  document.addEventListener('click', (e) => {
    const z = e.target.closest('[data-zoom]'); if (!z) return;
    e.preventDefault();
    const o = document.createElement('div'); o.className = 'dm-lightbox zoomable'; o.setAttribute('role', 'dialog'); o.setAttribute('aria-label', 'The card, up close');
    const img = document.createElement('img'); img.src = z.dataset.zoom; img.alt = ''; o.append(img);
    const x = document.createElement('button'); x.type = 'button'; x.className = 'dm-ib dm-lb-x'; x.textContent = '✕'; x.setAttribute('aria-label', 'Close'); o.append(x);
    const close = () => { o.remove(); document.removeEventListener('keydown', esc); };
    const esc = (ev) => { if (ev.key === 'Escape') close(); };
    // A tap on the picture brings it closer, where the tap was; another tap steps back.
    img.addEventListener('click', (ev) => { const r = img.getBoundingClientRect(), fx = (ev.clientX - r.left) / r.width, fy = (ev.clientY - r.top) / r.height; const big = o.classList.toggle('in'); if (big) requestAnimationFrame(() => { o.scrollLeft = img.offsetWidth * fx - o.clientWidth / 2; o.scrollTop = img.offsetHeight * fy - o.clientHeight / 2; }); });
    o.addEventListener('click', (ev) => { if (ev.target === o || ev.target === x) close(); });
    document.addEventListener('keydown', esc);
    document.body.append(o); x.focus();
  });

  /* ---------- Marketplace: more cards arrive as you scroll, and the filters stay open on a wide screen ---------- */
  let more = 0;
  addEventListener('scroll', () => {
    if (more) return;
    more = setTimeout(() => { more = 0; const b = document.querySelector('.fd-more button'); if (b && b.getBoundingClientRect().top < innerHeight + 900) b.click(); }, 120);
  }, { passive: true });
  const openFilters = () => { const d = document.querySelector('.fd-filters'); if (d && matchMedia('(min-width: 900px)').matches) d.open = true; };
  new MutationObserver(openFilters).observe(root, { attributes: true, attributeFilter: ['data-finder'] });
  openFilters();

  /* ---------- the look itself: this device, and (full admins) everyone ---------- */
  // A page that saved something and reloaded says so once it is back.
  addEventListener('DOMContentLoaded', () => { try { const m = sessionStorage.getItem('bbv:toast'); if (m) { sessionStorage.removeItem('bbv:toast'); setTimeout(() => window.bbvToast(m), 250); } } catch (_) {} });
  const post = (url, body) => fetch(url, { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json', Accept: 'application/json' }, body: JSON.stringify(body) }).then((r) => r.json().then((j) => (r.ok ? j : Promise.reject(new Error(j.error || 'That didn’t work.')))));
  const toast = window.bbvToast = (msg, bad) => { let t = document.querySelector('.cart-toast'); if (!t) { t = document.createElement('div'); t.className = 'cart-toast'; t.setAttribute('role', 'status'); document.body.append(t); } t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on'); clearTimeout(toast.t); toast.t = setTimeout(() => t.classList.remove('on'), 3200); };
  document.addEventListener('click', async (e) => {
    const mine = e.target.closest('[data-look-set]'), all = e.target.closest('[data-look-all]'), auto = e.target.closest('[data-auto]');
    try {
      if (auto) { await post('/admin/feed/auto', { kind: auto.dataset.auto, on: auto.dataset.on === '1' }); location.reload(); }
      if (mine) { await post('/me/look', { look: mine.dataset.lookSet }); location.reload(); }
      if (all) {
        const on = all.dataset.lookAll === 'on';
        if (!confirm(on ? 'Show the new look to every visitor? Pages that aren’t converted yet stay as they are.' : 'Go back to today’s look for every visitor?')) return;
        await post('/admin/look/all', { on }); location.reload();
      }
    } catch (err) { toast(err.message, true); }
  });
})();
