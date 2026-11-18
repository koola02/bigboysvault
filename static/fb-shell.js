/* The new look's frame: who is signed in, the tabs that show where you are, the menu, the account menu,
   notifications, the phone's search bar, and the switches for the look itself.
   */

// Who's here: asked once per page, shared by every script that needs it.
window.bbvWho = window.bbvWho || (() => (window.bbvWhoAnswer ||= fetch('/me/who', { credentials: 'same-origin', headers: { Accept: 'application/json' } })
  .then((r) => (r.ok ? r.json() : {})).catch(() => ({}))));

(() => {
  const root = document.documentElement;
  const $ = (id) => document.getElementById(id);

  /* ---------- where you are ---------- */
  const here = location.pathname;
  // Stores is lit wherever cards are shopped: every store, the vault's binders (they belong to the vault's own store),
  // a card's page, and the search's list of cards (The Vault has no tab of its own).
  const match = (h) => h === here || (h !== '/' && here.startsWith(h + '/'))
    || (h === '/stores' && (/^\/(store|collections)\//.test(here) || /^\/(all|search|products)(\/|$)/.test(here)));
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
    // Your name opens your own page (not its settings): straight to it once the site knows who you are, so the line
    // lights up while you are there. Until then the link is /me/page, which forwards. The owner signed in with the
    // admin password alone has no trainer page: there it goes to Admin.
    const mine = me && me.id ? `/trainers/${me.id}` : (!me && admin) ? '/admin' : '';
    if (mine) document.querySelectorAll('#mnMe, #meMenuMe').forEach((a) => {
      a.href = mine;
      if (a.id === 'mnMe') { if (here === mine) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current'); }
    });
    // "My store" opens the store itself, the same way (until then /me/store/page forwards).
    if (me && me.store) document.querySelectorAll('[data-my-store]').forEach((a) => {
      a.href = me.store;
      if (a.classList.contains('mn-i')) { if (here === me.store || here.startsWith(`${me.store}/`)) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current'); }
    });
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

  // A plain tap on Log in or Register comes back to this page afterwards, as the buttons that need an account already do.
  document.addEventListener('click', (e) => {
    const a = e.target.closest && e.target.closest('a[href="/login"], a[href="/join"]');
    if (!a || e.button || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || /^\/(login|join)(\/|$)/.test(here)) return;
    try { sessionStorage.setItem('bbv:after-login', location.pathname + location.search); sessionStorage.removeItem('bbv:login-why'); } catch (_) {}
  });

  /* ---------- the site's name in the phone header: it shrinks until it fits beside the buttons, never cut ---------- */
  const word = document.querySelector('.hd-word');
  const fitName = () => {
    if (!word) return;
    word.style.fontSize = '';
    if (getComputedStyle(word).display === 'none') return;
    let px = parseFloat(getComputedStyle(word).fontSize);
    while (word.scrollWidth > word.clientWidth + 0.5 && px > 12) { px -= 0.5; word.style.fontSize = `${px}px`; }
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
  const seen = (sel) => [...document.querySelectorAll(sel)].find((n) => n.getClientRects().length > 0);
  const mePop = $('meMenu'), meBtn = $('barMe');
  const setMe = (open) => { if (!mePop) return; mePop.hidden = !open; meBtn?.setAttribute('aria-expanded', String(open)); };
  document.addEventListener('click', (e) => {
    const t = e.target;
    if (t.closest('[data-menu-toggle]')) {
      const open = !root.classList.contains('menu-open');
      setMenu(open); setMe(false);
      // Opened from the keyboard (a click with no pointer behind it): start on the menu's first line.
      if (open && e.detail === 0) seen('#fbMenu .mn-i')?.focus();
      return;
    }
    if (root.classList.contains('menu-open') && (!t.closest('#fbMenu') || t.closest('#fbMenu a'))) setMenu(false);
    if (t.closest('#barMe')) { setMe(mePop.hidden); return; }
    if (mePop && !mePop.hidden && !t.closest('#meMenu')) setMe(false);
    if (t.closest('[data-search-open]')) { root.classList.add('search-open'); document.querySelector('.hd-search input')?.focus(); return; }
    if (t.closest('[data-search-close]')) root.classList.remove('search-open');
  });
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    if (root.classList.contains('menu-open')) {
      const inside = $('fbMenu')?.contains(document.activeElement);
      setMenu(false);
      if (inside || document.activeElement === document.body) seen('[data-menu-toggle]')?.focus();
    }
    if (mePop && !mePop.hidden) { setMe(false); meBtn?.focus(); }
    root.classList.remove('search-open');
  });
  // "/" or Ctrl+K jumps to the search box, as on today's site.
  document.addEventListener('keydown', (e) => {
    if ((e.key === 'k' && (e.ctrlKey || e.metaKey)) || (e.key === '/' && !/^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || '') && !document.activeElement?.isContentEditable)) {
      e.preventDefault(); root.classList.add('search-open'); document.querySelector('.hd-search input')?.focus();
    }
  });

  /* ---------- notifications: the bell in the header (members only) ----------
     From 700px it drops the latest down beside it. A phone has no room for a panel: there the bell opens the
     Notifications page, as Messages does (dm.js). Its number is live: the messages' connection (dm.js) brings it within
     seconds of anything new (bbv:notes). Each line shows who did it; tapping one reads it. */
  const btn = $('notifBtn'), menu = $('notifMenu'), badge = $('notifBadge'), list = $('notifList'), holder = $('notifDropdown');
  const bellPhone = () => matchMedia('(max-width: 699px), (max-height: 520px)').matches;
  const onNotifs = /^\/me\/notifications(\/|$)/.test(here);
  if (btn && onNotifs) btn.setAttribute('aria-current', 'page');
  const ago = (t) => {
    const s = Math.max(0, Math.floor(Date.now() / 1000 - (Number(t) || 0)));
    if (s < 60) return 'Just now'; if (s < 3600) return `${Math.floor(s / 60)} min ago`; if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    if (s < 7 * 86400) return `${Math.floor(s / 86400)} d ago`;
    return new Date(Number(t) * 1000).toLocaleDateString('en-PH', { day: 'numeric', month: 'short' });
  };
  let unread = 0;
  const count = (n) => { unread = Math.max(0, n); if (!badge) return; badge.hidden = !unread; badge.textContent = unread > 99 ? '99+' : String(unread); };
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
      j.notifications.slice(0, 30).forEach((n) => {
        // Messages can hold what other people typed: text only, never HTML.
        const a = document.createElement('a'); a.className = 'notif-item' + (n.is_read ? '' : ' unread'); a.href = n.link || '#'; a.dataset.id = n.id;
        const av = document.createElement('span'); av.className = 'av sm notif-av';
        if (n.avatar) { const i = document.createElement('img'); i.src = n.avatar; i.alt = ''; i.loading = 'lazy'; av.append(i); } else av.textContent = n.initial || '•';
        const body = document.createElement('span'); body.className = 'notif-body';
        const k = document.createElement('span'); k.className = 'notif-k'; k.textContent = n.label || n.kind || '';
        const m = document.createElement('span'); m.className = 'notif-m'; m.textContent = n.message || '';
        const t = document.createElement('span'); t.className = 'notif-t'; t.textContent = ago(n.created_at);
        body.append(k, m, t); a.append(av, body); list.append(a);
      });
    } catch (_) {}
  }
  const setNotif = (open) => { if (!menu) return; menu.hidden = !open; btn.setAttribute('aria-expanded', String(open)); if (open) load(); };
  btn?.addEventListener('click', (e) => {
    // The whole page instead: on a phone, in a new tab, and on the Notifications page (it is already on the screen).
    if (!menu || bellPhone() || e.button || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || onNotifs) return;
    e.preventDefault(); setNotif(menu.hidden);
  });
  document.addEventListener('click', (e) => { if (menu && !menu.hidden && !holder.contains(e.target)) setNotif(false); });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && menu && !menu.hidden) { setNotif(false); btn.focus(); } });
  // A line tapped is read: the number goes down at once, and the site hears it even as the next page opens.
  list?.addEventListener('click', (e) => {
    const a = e.target.closest('.notif-item'); if (!a || !a.classList.contains('unread')) return;
    a.classList.remove('unread'); count(unread - 1);
    fetch('/api/notifications/read', { method: 'POST', credentials: 'same-origin', keepalive: true, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id: Number(a.dataset.id) }) }).catch(() => {});
  });
  // Something new (or read on another page): the number changes, and an open list is drawn again.
  document.addEventListener('bbv:notes', (e) => {
    const n = Number(e.detail) || 0;
    if (n === unread) return;
    count(n);
    if (menu && !menu.hidden) load();
  });
  $('notifRead')?.addEventListener('click', async (e) => {
    e.stopPropagation();
    try { await fetch('/api/notifications/read', { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' }, body: '{}' }); count(0); list.querySelectorAll('.unread').forEach((n) => n.classList.remove('unread')); } catch (_) {}
  });
  // The bell shows as soon as a member is known (with Messages, so the site's name is fitted once), then its count.
  window.bbvWho().then((j) => { if (holder) holder.hidden = !(j && j.member); if (j && j.member) load(); });

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

  /* ---------- a card opens in a window over the page, with "More like this" under it ----------
     (Its own page stays for shared links, search engines and the admin's editor; the window needs no button to it.) */
  let cw = null, cwStop = null, cwBase = null, cwTools = null, cwSeq = 0, cwTalk = null, cwFrom = null;
  // A vault card, or a card a store has for sale (its ?part=1 is the same .detail).
  const cardPath = (a) => (a && a.origin === location.origin && /^\/(products\/[^/]+|trainers\/\d+\/cards\/\d+)$/.test(a.pathname) && !a.hash && !a.target ? a.pathname : '');
  const cwDepth = () => (history.state && history.state.cw ? history.state.depth || 1 : 0);
  const cwClose = () => { if (cwDepth()) history.go(-cwDepth()); else cwHide(); };
  function cwBuild() {
    cw = document.createElement('div'); cw.className = 'cardwin'; cw.hidden = true;
    cw.setAttribute('role', 'dialog'); cw.setAttribute('aria-modal', 'true'); cw.setAttribute('aria-label', 'Card');
    cw.innerHTML = '<div class="cardwin-in" tabindex="-1"><header class="cardwin-h"><button type="button" class="ibtn" data-cw-x aria-label="Close">✕</button><b class="cardwin-t"></b>'
      + '<a class="btn soft sm staff-only" data-full-only data-cw-edit>Edit</a></header><div class="cardwin-b"></div></div>';
    document.body.append(cw);
    let down = false;
    cw.addEventListener('mousedown', (e) => { down = e.target === cw; });
    cw.addEventListener('click', (e) => { if ((e.target === cw && down) || e.target.closest('[data-cw-x]')) cwClose(); down = false; });
  }
  async function cwShow(href, push) {
    if (!cw) cwBuild();
    const mine = ++cwSeq, body = cw.querySelector('.cardwin-b'), box = cw.querySelector('.cardwin-in');
    if (cw.hidden) {
      cwBase = { title: document.title };
      // Where the person was (the card they opened): focus goes back there when the window closes.
      cwFrom = document.activeElement && document.activeElement !== document.body ? document.activeElement : null;
      // On a phone a chat window fills the screen: it steps aside for the card and comes back afterwards.
      if (matchMedia('(max-width: 699px)').matches) { cwTalk = Object.keys(talks).find((k) => !talks[k].win.hidden) || null; if (cwTalk) talks[cwTalk].win.hidden = true; }
    }
    cw.hidden = false; root.classList.add('cardwin-open'); body.classList.add('busy');
    if (push) history.pushState({ cw: href, depth: cwDepth() + 1 }, '', href);
    let html;
    try {
      const r = await fetch(`${href}${href.includes('?') ? '&' : '?'}part=1`, { credentials: 'same-origin', headers: { Accept: 'text/html' } });
      if (!r.ok) throw new Error();
      html = await r.text();
    } catch (_) { location.href = href; return; } // the card's own page still works
    if (mine !== cwSeq) return;
    if (cwStop) { cwStop(); cwStop = null; }
    body.innerHTML = html; body.classList.remove('busy'); cw.scrollTop = 0;
    const detail = body.querySelector('.detail'), name = body.querySelector('.detail-info .h1')?.textContent.trim() || 'Card';
    cw.setAttribute('aria-label', name); cw.querySelector('.cardwin-t').textContent = name;
    document.title = `${name} · ${cwBase.title.split(' · ').pop()}`;
    // Only a vault card has an editor behind #edit: a store's card is the seller's to change.
    const edit = cw.querySelector('[data-cw-edit]'); edit.href = `${href}#edit`; edit.hidden = !href.startsWith('/products/');
    if (detail && detail.dataset.hit && navigator.sendBeacon) navigator.sendBeacon(`/hit/${detail.dataset.hit}`);
    document.dispatchEvent(new CustomEvent('float:filled', { detail: body }));
    box.focus({ preventScroll: true });
    try {
      cwTools = cwTools || Promise.all([import('/static/holo.js?v=6'), import('/static/card-lang.js?v=4')]);
      const [holo, lang] = await cwTools;
      if (mine !== cwSeq || cw.hidden) return;
      cwStop = holo.mountHolo(body); lang.mountLang(body);
    } catch (_) {}
  }
  function cwHide() {
    if (!cw || cw.hidden) return;
    cwSeq++; cw.hidden = true; root.classList.remove('cardwin-open');
    if (cwStop) { cwStop(); cwStop = null; }
    cw.querySelector('.cardwin-b').innerHTML = '';
    if (cwBase) document.title = cwBase.title;
    if (cwTalk && talks[cwTalk]) { talks[cwTalk].win.hidden = false; root.classList.add('ask-open'); }
    cwTalk = null;
    if (cwFrom && cwFrom.isConnected) cwFrom.focus({ preventScroll: true });
    cwFrom = null;
  }
  // The card window remembers the tile that opened it (a click doesn't always focus a link, so note it here).
  document.addEventListener('pointerdown', (e) => { const a = e.target.closest && e.target.closest('a.card, a.story, a.attach, a[href^="/products/"]'); if (a && (!cw || cw.hidden)) cwFrom = a; }, true);
  document.addEventListener('click', (e) => {
    if (e.defaultPrevented || e.button || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
    const a = e.target.closest && e.target.closest('a[href]'), href = cardPath(a);
    if (!href || a.hasAttribute('data-full-page')) return;
    // The small buttons on a tile (add to cart, message, the admin's pencil) do their own thing.
    if (e.target.closest('.cart-mini, .dm-mini, .card-pen, button, [role="button"]')) return;
    // On a card's own page the links simply go on to the next page.
    if (!a.closest('.cardwin') && document.querySelector('main .detail')) return;
    e.preventDefault();
    cwShow(href, true);
  });
  addEventListener('popstate', (e) => { if (e.state && e.state.cw) cwShow(e.state.cw, false); else cwHide(); });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && cw && !cw.hidden && !document.querySelector('.dm-lightbox, .cc, .sheet:not([hidden]), .dm-people')) cwClose(); });

  /* ---------- rows that slide sideways (a store's binders and highlighted cards on Stores) ----------
     A finger swipes them; a mouse can't, so on a computer an arrow shows at each end that has more to see. */
  if (matchMedia('(hover: hover) and (pointer: fine)').matches) {
    document.querySelectorAll('.st-brow, .seller-cards-row').forEach((row) => {
      const wrap = document.createElement('div'); wrap.className = 'slide-wrap';
      row.before(wrap); wrap.append(row);
      const arrow = (back) => {
        const b = document.createElement('button'); b.type = 'button'; b.className = `slide-btn ${back ? 'slide-prev' : 'slide-next'}`; b.hidden = true;
        b.setAttribute('aria-label', back ? 'Show the ones before' : 'Show more'); b.tabIndex = -1;   // keyboard users tab along the row itself
        b.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="${back ? 'M15 5l-7 7 7 7' : 'M9 5l7 7-7 7'}"/></svg>`;
        b.addEventListener('click', () => row.scrollBy({ left: (back ? -1 : 1) * Math.max(160, row.clientWidth * 0.8), behavior: 'smooth' }));
        wrap.append(b); return b;
      };
      const prev = arrow(true), next = arrow(false);
      const sync = () => { prev.hidden = row.scrollLeft <= 2; next.hidden = row.scrollLeft + row.clientWidth >= row.scrollWidth - 2; };
      row.addEventListener('scroll', sync, { passive: true });
      new ResizeObserver(sync).observe(row);
      sync();
    });
  }

  /* ---------- a card's picture, up close ---------- */
  // Tapping the big picture itself does what "Inspect up close" does: people expect a tap on a picture to bring it closer.
  document.addEventListener('click', (e) => {
    const pic = e.target.closest && e.target.closest('.detail-img img'); if (!pic) return;
    const z = pic.closest('.detail')?.querySelector('[data-zoom]'); if (z) { e.preventDefault(); z.click(); }
  });
  // "‹ Back" on a card's own page goes back to where you were (the same store, binder or search, at the same place),
  // when that is where you came from; otherwise to Stores.
  document.addEventListener('click', (e) => {
    const a = e.target.closest && e.target.closest('a[data-back]'); if (!a || e.ctrlKey || e.metaKey || e.shiftKey || e.button) return;
    let from = null; try { from = document.referrer ? new URL(document.referrer) : null; } catch (_) {}
    if (from && from.origin === location.origin && /^\/(all|search|collections|stores?)(\/|$)/.test(from.pathname) && history.length > 1) { e.preventDefault(); history.back(); }
  });
  document.addEventListener('click', (e) => {
    const z = e.target.closest('[data-zoom]'); if (!z) return;
    e.preventDefault();
    const o = document.createElement('div'); o.className = 'dm-lightbox zoomable'; o.setAttribute('role', 'dialog'); o.setAttribute('aria-label', 'The card, up close');
    const img = document.createElement('img'); img.src = z.dataset.zoom; img.alt = ''; o.append(img);
    const x = document.createElement('button'); x.type = 'button'; x.className = 'dm-ib dm-lb-x'; x.textContent = '✕'; x.setAttribute('aria-label', 'Close'); o.append(x);
    const close = () => { o.remove(); document.removeEventListener('keydown', esc); if (z.isConnected) z.focus({ preventScroll: true }); };
    const esc = (ev) => { if (ev.key === 'Escape') close(); };
    // A tap on the picture brings it closer, where the tap was; another tap steps back.
    img.addEventListener('click', (ev) => { const r = img.getBoundingClientRect(), fx = (ev.clientX - r.left) / r.width, fy = (ev.clientY - r.top) / r.height; const big = o.classList.toggle('in'); if (big) requestAnimationFrame(() => { o.scrollLeft = img.offsetWidth * fx - o.clientWidth / 2; o.scrollTop = img.offsetHeight * fy - o.clientHeight / 2; }); });
    o.addEventListener('click', (ev) => { if (ev.target === o || ev.target === x) close(); });
    document.addEventListener('keydown', esc);
    // Inside an open window (a store's Vouches), the photo goes in that window, or it would sit behind it.
    (z.closest('dialog[open]') || document.body).append(o); x.focus();
  });

  /* ---------- Marketplace: more cards arrive as you scroll, and the filters stay open on a wide screen ---------- */
  let more = 0;
  addEventListener('scroll', () => {
    if (more) return;
    more = setTimeout(() => { more = 0; const b = document.querySelector('.fd-more button'); if (b && b.getBoundingClientRect().top < innerHeight + 900) b.click(); }, 120);
  }, { passive: true });
  // The card finder names its page "All cards" each time the search changes; here that page is called The Vault.
  const vaultH = document.querySelector('.mk > .h1');
  if (vaultH) {
    const rename = () => { if (vaultH.textContent.trim() === 'All cards') vaultH.textContent = 'The Vault'; };
    new MutationObserver(rename).observe(vaultH, { childList: true, characterData: true, subtree: true });
    rename();
  }
  // The finder counts "1,296 cards · 2,218 copies"; everywhere else on the site "cards" means copies ("2,213 cards for
  // sale"). Said the site's way: "2,218 cards (1,296 different)", and "187 cards match (179 different)" when filtered.
  // Both are drawn by the finder once it has loaded (root gets data-finder then), so they are set up from there.
  const finderBits = () => {
    const countEl = document.querySelector('.fd-count');
    if (!countEl || countEl.dataset.said) return;
    countEl.dataset.said = '1';
    {
      const say = () => {
        for (const n of countEl.childNodes) {
          if (n.nodeType !== 3 || /different|match/.test(n.data)) continue;
          let m = n.data.match(/^\s*([\d,]+) of [\d,]+ cards? · ([\d,]+) cop(?:y|ies)\s*$/);
          if (m) { n.data = `${m[2]} card${m[2] === '1' ? '' : 's'} match (${m[1]} different)`; continue; }
          m = n.data.match(/^\s*([\d,]+) cards? · ([\d,]+) cop(?:y|ies)\s*$/);
          if (m) { n.data = `${m[2]} cards (${m[1]} different)`; continue; }
          m = n.data.match(/^\s*([\d,]+) of [\d,]+ cards?\s*$/);
          if (m) n.data = `${m[1]} card${m[1] === '1' ? '' : 's'} match`;
        }
      };
      new MutationObserver(say).observe(countEl, { childList: true, characterData: true, subtree: true });
      say();
    }
    // On a phone the filters open as a panel above the cards: a button at its foot shows how many cards match right
    // now, and takes you to them (before, a tap on a filter changed the count somewhere below, out of sight).
    const fdf = document.querySelector('.fd-filters');
    if (fdf && countEl) {
      const go = document.createElement('button'); go.type = 'button'; go.className = 'btn fd-show';
      const paint = () => { const m = (countEl.textContent || '').match(/([\d,]+) cards?/); go.textContent = m ? (m[1] === '0' ? 'No cards match' : `Show ${m[1]} card${m[1] === '1' ? '' : 's'}`) : 'Show the cards'; go.disabled = !!m && m[1] === '0'; };
      go.addEventListener('click', () => { fdf.open = false; countEl.scrollIntoView({ block: 'start', behavior: 'smooth' }); });
      fdf.append(go); paint();
      new MutationObserver(paint).observe(countEl, { childList: true, characterData: true, subtree: true });
    }
  };
  new MutationObserver(finderBits).observe(root, { attributes: true, attributeFilter: ['data-finder'] });
  finderBits();
  const openFilters = () => { const d = document.querySelector('.fd-filters'); if (d && matchMedia('(min-width: 900px)').matches) d.open = true; };
  new MutationObserver(openFilters).observe(root, { attributes: true, attributeFilter: ['data-finder'] });
  openFilters();

  /* ---------- saved-and-reloaded notes, and the switches for the feed's automatic posts ---------- */
  // A page that saved something and reloaded says so once it is back.
  addEventListener('DOMContentLoaded', () => { try { const m = sessionStorage.getItem('bbv:toast'); if (m) { sessionStorage.removeItem('bbv:toast'); setTimeout(() => window.bbvToast(m), 250); } } catch (_) {} });
  const post = (url, body) => fetch(url, { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json', Accept: 'application/json' }, body: JSON.stringify(body) }).then((r) => r.json().then((j) => (r.ok ? j : Promise.reject(new Error(j.error || 'That didn’t work.')))));
  /* ---------- "Are you sure?" in the site's own look ----------
     window.bbvConfirm(question, { yes, no, danger }) answers true or false. It replaces the browser's own box, which
     looks like a system warning (on a phone it says the site's address over it) and stops the page until answered. */
  window.bbvConfirm = (question, opt = {}) => new Promise((done) => {
    const back = document.activeElement;
    const o = document.createElement('div'); o.className = 'bbv-ask'; o.setAttribute('role', 'alertdialog'); o.setAttribute('aria-modal', 'true');
    const box = document.createElement('div'); box.className = 'bbv-ask-in';
    const p = document.createElement('p'); p.className = 'bbv-ask-q'; p.id = 'bbvAskQ' + Date.now(); p.textContent = question; o.setAttribute('aria-labelledby', p.id);
    const row = document.createElement('div'); row.className = 'bbv-ask-acts';
    const no = document.createElement('button'); no.type = 'button'; no.className = 'btn ghost'; no.textContent = opt.no || 'Keep it';
    const yes = document.createElement('button'); yes.type = 'button'; yes.className = 'btn' + (opt.danger === false ? '' : ' bbv-ask-danger'); yes.textContent = opt.yes || 'Yes';
    // Inside an open modal dialog when there is one (the vouch form): the rest of the page can't be clicked then.
    const host = [...document.querySelectorAll('dialog[open]')].pop() || document.body;
    row.append(no, yes); box.append(p, row); o.append(box); host.append(o);
    const end = (v) => { o.remove(); document.removeEventListener('keydown', key, true); if (back && back.isConnected) back.focus({ preventScroll: true }); done(v); };
    // Escape says no, and stops there (a window under it stays open); Tab stays between the two buttons.
    const key = (ev) => { if (ev.key === 'Escape') { ev.preventDefault(); ev.stopImmediatePropagation(); end(false); } else if (ev.key === 'Tab') { ev.preventDefault(); (document.activeElement === no ? yes : no).focus(); } };
    document.addEventListener('keydown', key, true);
    no.addEventListener('click', () => end(false)); yes.addEventListener('click', () => end(true));
    o.addEventListener('click', (ev) => { if (ev.target === o) end(false); });
    (opt.danger === false ? yes : no).focus();
  });
  const toast = window.bbvToast = (msg, bad) => { let t = document.querySelector('.cart-toast'); if (!t) { t = document.createElement('div'); t.className = 'cart-toast'; t.setAttribute('role', 'status'); document.body.append(t); } t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on'); clearTimeout(toast.t); toast.t = setTimeout(() => t.classList.remove('on'), 3200); };
  document.addEventListener('click', async (e) => {
    const auto = e.target.closest('[data-auto]');
    try {
      if (auto) { await post('/admin/feed/auto', { kind: auto.dataset.auto, on: auto.dataset.on === '1' }); location.reload(); }
    } catch (err) { toast(err.message, true); }
  });
})();

/* ---------- a binder on its own page (a trainer's store binder, or one of the vault's): the X, or Escape, goes back to its store ---------- */
(() => {
  const bx = document.querySelector('[data-binder-close]');
  if (!bx) return;
  const root = document.documentElement, path = (u) => u.pathname.replace(/\/+$/, '');
  // The history entry this page opened on, marked. A search on the page adds entries of its own (htmx), and from those
  // a step back would only undo the search.
  const mark = `${Date.now()}.${Math.random()}`;
  // How many entries this tab had when the page first opened (kept across a reload): with only this page in the tab
  // (opened in a new tab), there is nothing to step back to, whatever a card window added since.
  const prev = history.state || {};
  const firstLen = typeof prev.binderLen === 'number' ? prev.binderLen : history.length;
  try { history.replaceState(Object.assign({}, prev, { binder: mark, binderLen: firstLen }), ''); } catch (_) {}
  const canBack = () => (window.navigation && typeof navigation.canGoBack === 'boolean' ? navigation.canGoBack : firstLen > 1);
  // Came here from the store this X points to, and still where the page opened? Then step back to the store, so it opens
  // where it was scrolled to. Otherwise the X is a plain link.
  const fromStore = () => {
    try {
      const r = new URL(document.referrer), s = new URL(bx.href, location.href);
      return canBack() && r.origin === location.origin && path(r) === path(s) && (history.state || {}).binder === mark;
    } catch (_) { return false; }
  };
  // Step back to the store; if that goes nowhere within a moment, follow the link instead.
  const leave = () => {
    if (!fromStore()) { location.href = bx.href; return; }
    let gone = false;
    const away = () => { gone = true; };
    addEventListener('pagehide', away, { once: true });
    addEventListener('popstate', away, { once: true });
    history.back();
    setTimeout(() => { if (!gone) location.href = bx.href; }, 400);
  };
  bx.addEventListener('click', (e) => {
    if (e.button || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return; // a new tab or window: the link does that
    e.preventDefault(); leave();
  });
  // Escape too, unless something is open on top (a sheet, a card, a chat, a menu, the cart) or someone is typing.
  const busy = () => {
    const a = document.activeElement, on = root.classList;
    if (a && (a.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName) || a.closest('[role="dialog"], dialog, .hd, .cart'))) return true;
    if (['menu-open', 'search-open', 'cardwin-open', 'sheet-open', 'ask-open'].some((c) => on.contains(c)) || document.body.classList.contains('cart-open')) return true;
    return !!document.querySelector('.sheet:not([hidden]), .cardwin:not([hidden]), .ask-win:not([hidden]), dialog[open], .dm-lightbox, .cc, .dm-people, .dm-call-ui, [aria-expanded="true"]');
  };
  // Heard before anything else hears the key, so a sheet or menu that Escape is about to close still counts as open.
  addEventListener('keydown', (e) => { if (e.key === 'Escape' && !e.defaultPrevented && !e.repeat && !busy()) leave(); }, true);
})();
