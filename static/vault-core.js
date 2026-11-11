// How the whole site responds to you: light ripples out from every tap, windows and pages open from where you touched,
// things near the pointer lean toward it and light up along their edges, and numbers resolve when you reach them.
(() => {
  const root = document.documentElement;
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const tools = () => !!document.querySelector('.admin, #edList');

  // Where you last touched: the floating window opens from here, and so does the next page.
  addEventListener('pointerdown', (e) => {
    root.style.setProperty('--ox', `${e.clientX}px`);
    root.style.setProperty('--oy', `${e.clientY}px`);
    try { sessionStorage.setItem('bbvTap', `${Math.round(e.clientX)},${Math.round(e.clientY)}`); } catch (_) {}
    if (still || tools() || e.target.closest?.('input, textarea, select, .hx-stage')) return;
    const ring = document.createElement('i');
    ring.className = 'hx-ripple';
    ring.style.left = `${e.clientX}px`;
    ring.style.top = `${e.clientY}px`;
    ring.addEventListener('animationend', () => ring.remove());
    (document.querySelector('dialog.float[open]') || document.body).append(ring);
  }, { capture: true, passive: true });

  // A small tick in the hand on phones that can do it.
  document.addEventListener('click', (e) => {
    if (navigator.vibrate && e.target.closest?.('a, button')) navigator.vibrate(6);
  }, { passive: true });

  // The dock shows where you are.
  const here = location.pathname;
  document.querySelectorAll('.tabs a').forEach((a) => {
    const h = a.getAttribute('href');
    // A seller's own store (/store/name) lights the Stores tab too.
    if (h === here || (h !== '/' && here.startsWith(h)) || (h === '/stores' && here.startsWith('/store/'))) a.setAttribute('aria-current', 'page');
  });

  if (tools()) return;

  /* ---------- text that fits ----------
     Anybody is a variable font, 50% to 150% wide. A label too long for its space gets exactly the width that fits, solved in a few steps,
     so it stays one clean line at full height and nothing spills out of a card. */
  const FIT = '.card-cond, .card-set, .worth, .sc-p, .card-p, .stars, .role, .bf-plate h1, .hl-who b, .stats dd, .iv-spot b, .iv-spot small, .tr-tile b, .fd-chip, .fd-b-t, .iv-legend span, .msg-name, .bf-id h1, .bf-id p, .fp-at, .bk-n, .bx-cap-n, .binder-t, .qa-set-en, .brand-t, .tabs a:not(.tab-ask)';
  // Narrower letters first; if that isn't enough, a slightly smaller size (never under 70%), so nothing ends in "…".
  const fit = (n) => {
    n.style.removeProperty('font-stretch');
    n.style.removeProperty('font-size');
    if (!n.offsetWidth || !n.offsetHeight) return; // hidden (say, inside a closed panel): measured when it shows
    let s = parseFloat(getComputedStyle(n).fontStretch) || 100;
    for (let i = 0; i < 4 && n.scrollWidth > n.clientWidth + 0.5; i++) {
      s = Math.max(50, s * (n.clientWidth / n.scrollWidth) * 0.995);
      n.style.setProperty('font-stretch', `${s.toFixed(1)}%`, 'important');
      if (s <= 50) break;
    }
    let fs = parseFloat(getComputedStyle(n).fontSize);
    const least = fs * 0.7;
    for (let i = 0; i < 4 && n.scrollWidth > n.clientWidth + 0.5 && fs > least; i++) {
      fs = Math.max(least, fs * (n.clientWidth / n.scrollWidth) * 0.99);
      n.style.setProperty('font-size', `${fs.toFixed(2)}px`, 'important');
    }
  };
  let fitQueue = new Set(), fitWait = 0;
  const fitSoon = (scope = document) => {
    (scope.querySelectorAll ? scope.querySelectorAll(FIT) : []).forEach((n) => fitQueue.add(n));
    if (scope.matches?.(FIT)) fitQueue.add(scope);
    if (!fitWait) fitWait = requestAnimationFrame(() => { fitWait = 0; const list = fitQueue; fitQueue = new Set(); list.forEach(fit); });
  };
  fitSoon();
  document.fonts?.ready.then(() => fitSoon());
  // A panel that opens (the finder's filters) shows labels that couldn't be measured while it was closed.
  document.addEventListener('toggle', (e) => { if (e.target.matches?.('details')) fitSoon(e.target); }, true);
  let widthWas = innerWidth;
  addEventListener('resize', () => { if (innerWidth !== widthWas) { widthWas = innerWidth; fitSoon(); } }, { passive: true });
  new MutationObserver((changes) => {
    for (const c of changes) c.addedNodes.forEach((n) => { if (n.nodeType === 1 && !n.classList?.contains('hx-ripple')) fitSoon(n); });
  }).observe(document.body, { childList: true, subtree: true });
  // On a phone the bell takes room in the top bar when it shows up: fit the shop's name again.
  const bell = document.getElementById('notifDropdown'), brand = document.querySelector('.bar .brand');
  if (bell && brand) new MutationObserver(() => fitSoon(brand)).observe(bell, { attributes: true, attributeFilter: ['hidden'] });

  /* ---------- the panel of what's inside the vault answers as you point ---------- */
  document.querySelectorAll('.iv').forEach((iv) => {
    const n = iv.querySelector('[data-iv-n]'), l = iv.querySelector('[data-iv-l]'), read = iv.querySelector('[data-iv-read]');
    if (!n) return;
    const allN = n.textContent, allL = l.textContent, line = read?.innerHTML;
    iv.addEventListener('pointerover', (e) => {
      const a = e.target.closest?.('.iv-legend a');
      if (a) { n.textContent = a.dataset.n; l.textContent = a.dataset.l; iv.dataset.on = a.dataset.c; } else if (!e.target.closest?.('.iv-arc')) { n.textContent = allN; l.textContent = allL; delete iv.dataset.on; }
      const bar = e.target.closest?.('[data-iv-bar]');
      if (read) { if (bar) read.textContent = `${bar.dataset.ivBar}. Tap to see them.`; else if (!e.target.closest?.('.iv-plot')) read.innerHTML = line; }
    }, { passive: true });
    iv.addEventListener('pointerleave', () => { n.textContent = allN; l.textContent = allL; delete iv.dataset.on; if (read) read.innerHTML = line; });
  });

  /* ---------- numbers resolve out of noise when they come into view ---------- */
  if (!still && 'IntersectionObserver' in window) {
    const seen = new IntersectionObserver((list) => {
      for (const { isIntersecting, target } of list) {
        if (!isIntersecting) continue;
        seen.unobserve(target);
        const end = target.textContent, t0 = performance.now(), dur = 1100;
        const step = (now) => {
          const k = Math.min(1, (now - t0) / dur), fixed = Math.floor(k * k * end.length * 1.2);
          target.textContent = [...end].map((c, i) => (/\d/.test(c) && i >= fixed ? String(Math.floor(Math.random() * 10)) : c)).join('');
          if (k < 1) requestAnimationFrame(step); else { target.textContent = end; fitSoon(target); }
        };
        requestAnimationFrame(step);
      }
    }, { threshold: 0.6 });
    document.querySelectorAll('.stats dd').forEach((d) => seen.observe(d));
  }

  /* ---------- the field around the pointer ---------- */
  if (still || !matchMedia('(hover: hover) and (pointer: fine)').matches) return;
  const SEL = 'a.card, .sc-card, .hl, .vouch, .btn, .hl-count, .bx-btn, .float-back, .fd-go, .bf-search, .tr-tile, .iv-spot, .ask-chip, .tabs a, .binder';
  const REACH = 300;
  let nodes = [], stamp = 0, x = -1e4, y = -1e4, queued = false, lit = new Set();
  const refresh = (now) => {
    if (now - stamp < 350) return;
    stamp = now;
    const open = document.querySelector('dialog.float[open]');
    nodes = [...(open || document).querySelectorAll(SEL)].filter((n) => {
      const r = n.getBoundingClientRect();
      return r.bottom > -REACH && r.top < innerHeight + REACH && r.width > 0;
    });
  };
  const paint = (now) => {
    queued = false;
    refresh(now);
    const rects = nodes.map((n) => n.getBoundingClientRect());
    const next = new Set();
    rects.forEach((r, i) => {
      const n = nodes[i];
      const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
      const nx = Math.max(r.left, Math.min(x, r.right)), ny = Math.max(r.top, Math.min(y, r.bottom));
      const d = Math.hypot(x - nx, y - ny);
      if (d > REACH) return;
      const k = 1 - d / REACH, pull = k * k;
      const big = r.width > 170 ? 5 : 8;
      const dist = Math.hypot(x - cx, y - cy) || 1;
      n.style.setProperty('--fx', `${(x - r.left).toFixed(0)}px`);
      n.style.setProperty('--fy', `${(y - r.top).toFixed(0)}px`);
      n.style.setProperty('--fn', k.toFixed(3));
      n.style.setProperty('--fdx', `${(((x - cx) / dist) * pull * big).toFixed(2)}px`);
      n.style.setProperty('--fdy', `${(((y - cy) / dist) * pull * big).toFixed(2)}px`);
      next.add(n);
    });
    lit.forEach((n) => {
      if (next.has(n)) return;
      for (const v of ['--fx', '--fy', '--fn', '--fdx', '--fdy']) n.style.removeProperty(v);
    });
    lit = next;
  };
  addEventListener('pointermove', (e) => {
    x = e.clientX; y = e.clientY;
    if (!queued) { queued = true; requestAnimationFrame(paint); }
  }, { passive: true });
  addEventListener('scroll', () => { stamp = 0; if (!queued) { queued = true; requestAnimationFrame(paint); } }, { passive: true, capture: true });
  document.addEventListener('pointerleave', () => { x = -1e4; y = -1e4; if (!queued) { queued = true; requestAnimationFrame(paint); } });
})();

// The top menu: a plain mouse wheel scrolls it sideways, for anyone without a trackpad.
(() => {
  const m = document.querySelector('.menu');
  m?.addEventListener('wheel', (e) => {
    if (Math.abs(e.deltaY) <= Math.abs(e.deltaX) || m.scrollWidth <= m.clientWidth) return;
    m.scrollLeft += e.deltaY;
    e.preventDefault();
  }, { passive: false });
})();

// ---------- Who's here: asked once per page, shared by every script that needs it ----------
// vault-staff.js and vault-talk.js call the same window.bbvWho(), so the page asks the server only once.
window.bbvWho = window.bbvWho || (() => (window.bbvWhoAnswer ||= fetch('/me/who', { credentials: 'same-origin', headers: { Accept: 'application/json' } })
  .then((r) => (r.ok ? r.json() : {})).catch(() => ({}))));

// ---------- The binders menu in the top bar: closes when you tap elsewhere, press Esc, or pick a binder ----------
(() => {
  const dd = document.querySelector('.menu-dd');
  if (!dd) return;
  document.addEventListener('click', (e) => { if (dd.open && !dd.contains(e.target)) dd.open = false; });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && dd.open) { dd.open = false; dd.querySelector('summary')?.focus(); } });
  dd.querySelector('.menu')?.addEventListener('click', (e) => { if (e.target.closest('a')) dd.open = false; });
})();

// ---------- Notifications Bell ----------
// Only signed-in trainers have notifications, so the bell asks only once /me/who says someone is signed in.
// Signed-out visitors never ask (no failed request), and signing in shows the bell on the very next page.
(() => {
  const notifBtn = document.getElementById('notifBtn');
  if (!notifBtn) return;
  const notifMenu = document.getElementById('notifMenu');
  const notifBadge = document.getElementById('notifBadge');
  const notifList = document.getElementById('notifList');
  const notifDropdown = document.getElementById('notifDropdown');
  let loaded = false;
  // Old copies of this script kept a "not signed in" flag for the whole visit: it isn't used any more.
  try { sessionStorage.removeItem('bbv:no-notif'); } catch (_) {}
  const ago = (t) => {
    const s = Math.max(0, Math.floor(Date.now() / 1000 - (Number(t) || 0)));
    if (s < 60) return 'Just now';
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    if (s < 7 * 86400) return `${Math.floor(s / 86400)} d ago`;
    return new Date(Number(t) * 1000).toLocaleDateString('en-PH', { day: 'numeric', month: 'short' });
  };

  async function loadNotifs() {
    try {
      const res = await fetch('/api/notifications', { credentials: 'same-origin', headers: { Accept: 'application/json' } });
      if (!res.ok) { notifDropdown.hidden = true; return; }
      const j = await res.json();
      notifDropdown.hidden = false;
      if (j.unread_count > 0) {
        notifBadge.textContent = j.unread_count > 99 ? '99+' : j.unread_count;
        notifBadge.hidden = false;
      } else {
        notifBadge.hidden = true;
      }
      notifList.replaceChildren();
      if (!j.notifications || !j.notifications.length) {
        const p = document.createElement('p'); p.className = 'notif-empty'; p.textContent = 'No new notifications.';
        notifList.append(p);
      } else {
        j.notifications.forEach(n => {
          // Messages can hold what other people typed: text only, never HTML.
          const a = document.createElement('a'); a.className = 'notif-item' + (n.is_read ? '' : ' unread'); a.href = n.link || '#';
          const k = document.createElement('div'); k.className = 'notif-k'; k.textContent = n.kind || '';
          const m = document.createElement('div'); m.className = 'notif-m'; m.textContent = n.message || '';
          const t = document.createElement('div'); t.className = 'notif-t'; t.textContent = ago(n.created_at);
          a.append(k, m, t);
          notifList.append(a);
        });
      }
      loaded = true;
    } catch (_) {}
  }

  notifBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    const showing = !notifMenu.hidden;
    notifMenu.hidden = showing;
    if (!showing) {
      if (!loaded) loadNotifs();
    }
  });
  document.addEventListener('click', (e) => {
    if (!notifMenu.hidden && !notifDropdown.contains(e.target)) notifMenu.hidden = true;
  });
  document.getElementById('notifRead')?.addEventListener('click', async (e) => {
    e.stopPropagation();
    try {
      await fetch('/api/notifications/read', { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' }, body: '{}' });
      notifBadge.hidden = true;
      notifList.querySelectorAll('.unread').forEach(el => el.classList.remove('unread'));
    } catch (_) {}
  });

  // Ask for the count once we know a trainer is signed in.
  window.bbvWho().then((j) => { if (j && j.member) loadNotifs(); else notifDropdown.hidden = true; });
})();

// ---------- Who's here: the profile chip in the top bar, and no "Log in" while signed in ----------
(() => {
  const root = document.documentElement;
  // Looked up each time, so it still works if part of the page was swapped.
  const $ = (id) => document.getElementById(id);
  const paint = (me, admin) => {
    const chip = $('barMe'), av = $('barMeAv'), label = $('barMeT');
    if (me && me.name) {
      root.dataset.me = me.role === 'admin' || admin ? 'admin' : '1';
      if (chip && av && label) {
        chip.hidden = false; chip.href = '/me';
        av.replaceChildren();
        if (me.avatar) { const i = document.createElement('img'); i.src = me.avatar; i.alt = ''; i.width = 36; i.height = 36; av.append(i); av.classList.add('has-pic'); }
        else { av.textContent = me.initial || me.name[0].toUpperCase(); av.classList.remove('has-pic'); }
        label.textContent = me.name;
        chip.title = `${me.name} · My profile`;
      }
    } else if (admin) {
      // The owner, signed in with the admin password alone.
      root.dataset.me = 'admin';
      if (chip && av && label) { chip.hidden = false; chip.href = '/admin'; av.replaceChildren(); av.textContent = '★'; av.classList.remove('has-pic'); label.textContent = 'Admin'; chip.title = 'Admin'; }
    } else {
      delete root.dataset.me;
      if (chip) chip.hidden = true;
    }
    const login = $('barLogin'); if (login) login.hidden = !!root.dataset.me;
  };
  // Last visit's answer first (no flash), then today's.
  let last = null;
  try { last = JSON.parse(localStorage.getItem('bbv:me') || 'null'); } catch (_) {}
  if (last) paint(last.me, last.admin);
  window.bbvWho().then((j) => {
    if (!j || !('member' in j)) return; // no answer (offline): keep what's shown
    const me = j.member || null, admin = !!j.full;
    paint(me, admin);
    try { if (me || admin) localStorage.setItem('bbv:me', JSON.stringify({ me, admin })); else localStorage.removeItem('bbv:me'); } catch (_) {}
  });
})();
