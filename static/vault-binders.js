// The binders on the home page stand on one 3D ring, each one a real binder: leather cover, strap, spine, pages.
// Next (›, a swipe to the left, the → key) turns the ring clockwise by one binder, seen from above: the front binder
// swings away to the left and the next one comes round from the right. Back (‹, a swipe to the right, ←) turns it the
// other way. The ring follows a dragging finger and settles on the nearest binder.
// Tapping the binder in front slips its strap and opens its cover on a page of its cards; tapping it again goes in.
// Tapping a binder at the side turns the ring to it. Turning the ring shuts the open binder.
(() => {
  document.querySelectorAll('[data-bx]').forEach((root) => {
    if (root.dataset.on) return;
    root.dataset.on = '1';
    const stage = root.querySelector('[data-stage]');
    const real = [...stage.querySelectorAll('.bx-i')];
    const dots = [...root.querySelectorAll('[data-dot]')];
    const cap = root.querySelector('.bx-cap');
    const x = root.querySelector('[data-shut]');
    const capT = root.querySelector('[data-cap-t]'), capN = root.querySelector('[data-cap-n]'), capGo = root.querySelector('[data-cap-go]');
    const n = real.length;
    if (!n) return;

    // A ring needs a few binders to look like one, so a short list goes round twice (the copies are only for show).
    const items = [...real];
    while (n > 1 && items.length < 6) real.forEach((el) => { const c = el.cloneNode(true); c.setAttribute('aria-hidden', 'true'); c.tabIndex = -1; c.dataset.copy = '1'; items.push(c); });
    const ring = document.createElement('div'); ring.className = 'bx-ring';
    items.forEach((el) => ring.append(el));
    stage.append(ring);
    const N = items.length, step = 360 / N;

    let pos = 0;               // which binder is in front; it keeps counting up or down, so every turn goes the short way round
    let radius = 0, shown = -1;
    const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
    const front = () => ((Math.round(pos) % N) + N) % N;

    function size() {
      const W = stage.clientWidth;
      if (!W) return;
      const phone = W < 760;
      // Never taller than about half the screen, so a phone on its side still sees the whole binder.
      const tw = Math.min(phone ? Math.min(W * 0.6, 270) : Math.min(360, W * 0.25), innerHeight * 0.5);
      const th = tw * 1.24, d = tw * 0.15;
      const lift = phone ? 22 : 34;
      radius = N > 1 ? (tw / 2 + d * 0.6 + (phone ? 8 : 20)) / Math.tan(Math.PI / N) : 0;
      root.style.setProperty('--tw', `${tw}px`);
      root.style.setProperty('--th', `${th}px`);
      root.style.setProperty('--d', `${d}px`);
      root.style.setProperty('--r', `${radius}px`);
      stage.style.height = `${th + lift * 2 + (phone ? 16 : 36)}px`;
      items.forEach((el, i) => el.style.setProperty('--a', `${i * step}deg`));
      paint();
    }

    // The pocket photos load only for the binders that can be looked into: the one in front and its neighbours.
    const fill = (el) => el.querySelectorAll('.bk-pk img[data-src]').forEach((im) => {
      im.addEventListener('load', () => im.classList.add('in'), { once: true });
      im.src = im.dataset.src; im.removeAttribute('data-src');
      if (im.complete) im.classList.add('in');
    });

    // The ✕ above an open binder shuts it again.
    const shut = () => { items.forEach((el) => el.classList.remove('open')); if (x) x.hidden = true; };
    const openIt = (el) => { fill(el); el.classList.add('open'); if (x) x.hidden = false; };
    x?.addEventListener('click', (e) => { e.stopPropagation(); shut(); items[front()]?.focus({ preventScroll: true }); });

    function caption(i) {
      const el = items[i]; if (!el || !cap) return;
      const set = () => { capT.textContent = el.dataset.t; capN.textContent = el.dataset.n; capGo.href = el.getAttribute('href'); capGo.setAttribute('aria-label', `See all the cards in ${el.dataset.t}`); };
      if (reduce || shown === -1) { set(); return; }
      cap.classList.add('swap');
      setTimeout(() => { set(); cap.classList.remove('swap'); }, 220);
    }

    function paint() {
      ring.style.transform = `translateZ(${-radius}px) rotateY(${-pos * step}deg)`;
      const f0 = front();
      items.forEach((el, i) => {
        const f = Math.cos(((i - pos) * step * Math.PI) / 180);
        el.style.setProperty('--f', Math.max(0, f).toFixed(3));
        el.dataset.seat = i === f0 ? 'mid' : f > 0.15 ? 'side' : 'away';
        if (!el.dataset.copy) { el.tabIndex = i === f0 ? 0 : -1; el.setAttribute('aria-hidden', i === f0 ? 'false' : 'true'); }
      });
      if (f0 !== shown) {
        caption(f0);
        shown = f0;
        [f0, (f0 + 1) % N, (f0 + N - 1) % N].forEach((i) => fill(items[i]));
      }
      const k = f0 % n;
      dots.forEach((d, j) => { d.setAttribute('aria-current', String(j === k)); d.dataset.on = j === k ? '1' : '0'; });
    }

    const go = (to) => {
      const was = front();
      pos = to; ring.classList.remove('held');
      if (front() !== was) shut();
      paint();
    };
    const turn = (by) => go(Math.round(pos) + by);
    // Turn to binder i (any copy of it) the short way round.
    const turnTo = (i) => {
      const here = Math.round(pos), at = front();
      let d = (((i - at) % N) + N) % N;
      if (d > N / 2) d -= N;
      go(here + d);
    };

    root.querySelector('[data-next]')?.addEventListener('click', () => turn(1));
    root.querySelector('[data-prev]')?.addEventListener('click', () => turn(-1));
    dots.forEach((d) => d.addEventListener('click', () => {
      const want = Number(d.dataset.dot), at = front();
      // The nearest copy of that binder.
      let best = want, gap = N;
      for (let i = want; i < N; i += n) { const g = Math.min((i - at + N) % N, (at - i + N) % N); if (g < gap) { gap = g; best = i; } }
      turnTo(best);
    }));
    root.addEventListener('keydown', (e) => {
      if (e.key === 'ArrowRight') { e.preventDefault(); turn(1); }
      if (e.key === 'ArrowLeft') { e.preventDefault(); turn(-1); }
      if (e.key === 'Escape' && x && !x.hidden) { e.preventDefault(); shut(); }
    });

    // Dragging turns the ring with the finger. A quick flick counts too, so a short swipe still turns one binder.
    let drag = null, moved = false;
    stage.addEventListener('pointerdown', (e) => {
      if (e.button || N < 2) return;
      drag = { x: e.clientX, y: e.clientY, id: e.pointerId, from: pos, t: performance.now(), lastX: e.clientX, v: 0, side: null };
      moved = false;
    });
    addEventListener('pointermove', (e) => {
      if (!drag || e.pointerId !== drag.id) return;
      const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
      if (drag.side === null && Math.hypot(dx, dy) > 8) drag.side = Math.abs(dx) > Math.abs(dy);   // sideways: turn; up or down: let the page scroll
      if (!drag.side) return;
      if (!moved) shut();
      moved = true;
      const now = performance.now();
      drag.v = (e.clientX - drag.lastX) / Math.max(1, now - drag.t); drag.t = now; drag.lastX = e.clientX;
      const per = parseFloat(getComputedStyle(root).getPropertyValue('--tw')) || 240;
      pos = drag.from - dx / (per * 0.9);
      ring.classList.add('held');
      paint();
    }, { passive: true });
    const end = (e) => {
      if (!drag || (e && e.pointerId !== drag.id)) return;
      const d = drag; drag = null;
      if (!d.side) return;
      let to = Math.round(pos);
      if (to === Math.round(d.from) && Math.abs(d.v) > 0.35) to += d.v < 0 ? 1 : -1;   // a flick
      go(to);
      if (navigator.vibrate) navigator.vibrate(8);
      setTimeout(() => { moved = false; }, 80);
    };
    addEventListener('pointerup', end, { passive: true });
    addEventListener('pointercancel', end, { passive: true });

    // A drag is not a tap. A tap on a binder at the side turns it to the front. A tap on the shut binder in front
    // opens its cover; a tap on it once it's open goes into the binder.
    stage.addEventListener('click', (e) => {
      if (moved) { e.preventDefault(); e.stopPropagation(); moved = false; return; }
      const el = e.target.closest('.bx-i'); if (!el) return;
      if (el.dataset.seat !== 'mid') { e.preventDefault(); e.stopPropagation(); turnTo(items.indexOf(el)); return; }
      if (!el.classList.contains('open')) { e.preventDefault(); e.stopPropagation(); openIt(el); if (navigator.vibrate) navigator.vibrate(8); }
    }, true);
    stage.addEventListener('dragstart', (e) => e.preventDefault());

    // An open binder shuts again once it's scrolled out of sight.
    new IntersectionObserver((es) => es.forEach((en) => { if (!en.isIntersecting) shut(); })).observe(stage);

    if (reduce) ring.classList.add('calm');
    new ResizeObserver(size).observe(stage);
    size();
    requestAnimationFrame(() => root.classList.add('ready'));
    root.hx = { get at() { return front() % n; } };
  });
})();
