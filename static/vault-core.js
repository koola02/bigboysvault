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
    if (h === here || (h !== '/' && here.startsWith(h))) a.setAttribute('aria-current', 'page');
  });

  if (tools()) return;

  /* ---------- text that fits ----------
     Anybody is a variable font, 50% to 150% wide. A label too long for its space gets exactly the width that fits, solved in a few steps,
     so it stays one clean line at full height and nothing spills out of a card. */
  const FIT = '.card-cond, .card-set, .worth, .sc-p, .card-p, .stars, .role, .bf-plate h1, .hl-who b, .stats dd, .iv-spot b, .iv-spot small, .tr-tile b, .fd-chip, .fd-b-t, .iv-legend span, .msg-name, .bf-id h1, .bf-id p, .fp-at, .bk-n, .bx-cap-n, .binder-t, .qa-set-en';
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

// ---------- Notifications Bell ----------
(() => {
  const notifBtn = document.getElementById('notifBtn');
  if (!notifBtn) return;
  const notifMenu = document.getElementById('notifMenu');
  const notifBadge = document.getElementById('notifBadge');
  const notifList = document.getElementById('notifList');
  const notifDropdown = document.getElementById('notifDropdown');
  let loaded = false;
  // Only trainers have notifications. Once a page gets "not signed in", the rest of the visit doesn't ask again.
  const SKIP = 'bbv:no-notif';
  try { if (sessionStorage.getItem(SKIP)) return; } catch (_) {}
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
      const res = await fetch('/api/notifications');
      if (!res.ok) { notifDropdown.hidden = true; if (res.status === 401 || res.status === 403) { try { sessionStorage.setItem(SKIP, '1'); } catch (_) {} } return; }
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
      await fetch('/api/notifications/read', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
      notifBadge.hidden = true;
      notifList.querySelectorAll('.unread').forEach(el => el.classList.remove('unread'));
    } catch (_) {}
  });

  // Fetch count on load
  setTimeout(loadNotifs, 500);
})();
