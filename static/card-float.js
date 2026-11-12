// Tapping a card or a trainer opens it in a floating window over the page, so you never lose your place. Tap outside the window to close it.
// The address still changes, so it can be shared, and opening that address directly shows the normal page.
(() => {
  if (document.getElementById('edList') || document.querySelector('.admin')) return; // the owner's tools keep their own links
  const SEL = 'a[href^="/products/"], a[data-float], a[href^="/trainers/"], a[href^="/collections/"]';
  const cache = new Map();
  const part = (href) => href + (href.includes('?') ? '&' : '?') + 'part=1';
  const load = (href) => {
    if (!cache.has(href)) {
      const job = fetch(part(href), { credentials: 'same-origin' }).then((r) => { if (!r.ok) throw new Error('status ' + r.status); return r.text(); });
      job.catch(() => cache.delete(href));
      cache.set(href, job);
    }
    return cache.get(href);
  };
  let tools = null;
  const loadTools = () => (tools ??= Promise.all([import('/static/holo.js?v=6'), import('/static/card-lang.js?v=4')]));

  let dialog, panel, pager, stopHolo = null, baseUrl = null, baseTitle = '', asking = 0;
  let shown = null, depth = 0, trail = [], zoomed = null;
  // A binder stays exactly as it was while a card from it is open: its filters, page and the card on its left page are all still there on the way back.
  const kept = new Map(), mounted = new Set();
  const arrow = (d) => `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="${d}"/></svg>`;

  function build() {
    dialog = document.createElement('dialog');
    dialog.className = 'float';
    dialog.innerHTML = '<div class="float-panel" tabindex="-1"></div>'
      + `<nav class="float-pager" aria-label="Pages of this binder" hidden><button type="button" class="fp-btn" data-turn="-1" aria-label="Previous page">${arrow('M15 5l-7 7 7 7')}<span>Back</span></button>`
      + `<button type="button" class="fp-at" aria-haspopup="true" aria-expanded="false" aria-live="polite"></button><button type="button" class="fp-btn" data-turn="1" aria-label="Next page"><span>Next</span>${arrow('M9 5l7 7-7 7')}</button>`
      + '<div class="fp-pick" role="group" aria-label="Go to a page" hidden><p>Go to page</p><div class="fp-nums"></div></div></nav>';
    panel = dialog.firstChild;
    pager = dialog.lastChild;
    document.body.append(dialog);
    // Closes only when the press both starts and ends outside, so dragging a tilted card past the edge doesn't shut it.
    // The page buttons under a binder are part of the window, not outside it.
    const inside = (t) => panel.contains(t) || pager.contains(t);
    let downOutside = false;
    dialog.addEventListener('pointerdown', (e) => { downOutside = !inside(e.target); });
    dialog.addEventListener('click', (e) => { if (downOutside && !inside(e.target)) leave(); downOutside = false; });
    // The Esc key steps out of a picture first, then closes the window.
    dialog.addEventListener('cancel', (e) => { e.preventDefault(); if (!pick.hidden) pickPage(false); else if (zoomed && shown) history.back(); else leave(); });
    // Page buttons, and the page number itself: tapping it lays out every page to jump straight to.
    const pick = pager.querySelector('.fp-pick');
    const pickPage = (open) => {
      const b = openBook();
      pick.hidden = !open || !b;
      pager.querySelector('.fp-at').setAttribute('aria-expanded', String(!pick.hidden));
      if (pick.hidden) return;
      const { page, pages } = b.info();
      const nums = pick.querySelector('.fp-nums');
      nums.replaceChildren(...Array.from({ length: pages }, (_, i) => {
        const n = document.createElement('button');
        n.type = 'button'; n.dataset.page = String(i); n.textContent = String(i + 1);
        n.setAttribute('aria-label', `Page ${i + 1}`);
        if (i === page) { n.className = 'on'; n.setAttribute('aria-current', 'page'); }
        return n;
      }));
      const on = nums.querySelector('.on');
      if (on) { nums.scrollTop = on.offsetTop - nums.clientHeight / 2; on.focus({ preventScroll: true }); }
    };
    pager.addEventListener('click', (e) => {
      const t = e.target.closest('[data-turn]'); if (t) { pickPage(false); openBook()?.turn(Number(t.dataset.turn)); return; }
      if (e.target.closest('.fp-at')) { pickPage(pick.hidden); return; }
      const n = e.target.closest('[data-page]'); if (n) { openBook()?.go(Number(n.dataset.page)); pickPage(false); }
    });
    panel.addEventListener('pointerdown', () => { if (!pick.hidden) pickPage(false); });
    // The arrow keys turn the binder's pages, unless someone is typing.
    dialog.addEventListener('keydown', (e) => {
      if (zoomed || (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') || e.target.closest?.('input, select, textarea, [contenteditable]')) return;
      const b = openBook(); if (!b) return;
      e.preventDefault(); b.turn(e.key === 'ArrowRight' ? 1 : -1);
    });
  }

  /* ---------- a binder, opened: search on top, nine pockets on the right page, the tapped card on the left page ---------- */
  const PER = 9;
  let seq = 0;
  const openBook = () => panel?.querySelector(':scope > .bf')?.book || null;
  // The page buttons under the window belong to the binder showing in it, if there is one.
  function syncPager() {
    const b = openBook();
    if (!b || zoomed) { if (pager) { pager.hidden = true; pager.querySelector('.fp-pick').hidden = true; } return; }
    b.paint();
  }

  function book(bf) {
    if (bf.book) return bf.book;
    bf.id ||= `bf${++seq}`;
    const pk = bf.querySelector('.bf-pk'), left = bf.querySelector('.bf-left'), slot = bf.querySelector('.bf-finder');
    const box = bf.querySelector('.bf-detail'), intro = bf.querySelector('.bf-intro');
    const style = document.createElement('style'); bf.append(style);
    let grid = bf.querySelector('.bf-grid'), live = false, page = 0, stopLeft = null, queued = false, swiped = false;
    const tiles = () => [...grid.children].filter((e) => e.matches('a.card'));
    // Before the search starts, the server knows how many cards there are; after, it's what the search shows.
    const count = () => (live ? tiles().length : Number(bf.dataset.tiles || tiles().length));
    const pages = () => Math.max(1, Math.ceil(count() / PER));
    const more = () => (live ? slot.querySelector('.fd-more button') : bf.querySelector('.bf-more'));

    function show() {
      queued = false;
      const n = pages();
      if (page > n - 1) page = n - 1;
      const a = page * PER + 1, z = a + PER - 1;
      style.textContent = `#${bf.id} .bf-pk > .grid > :not(:nth-child(n+${a}):nth-child(-n+${z})) { display: none !important; }`;
      // The search shows its cards in batches; a binder wants them all, to count its pages. The server's list only needs the next page.
      if (live || tiles().length < z) more()?.click();
      bf.querySelector('.bf-none').hidden = !(live && count() === 0);
      paint();
    }
    const soon = () => { if (!queued) { queued = true; requestAnimationFrame(show); } };
    let watch = new MutationObserver(soon);
    watch.observe(grid, { childList: true });

    function paint() {
      if (openBook() !== api || !pager) return;
      const n = pages();
      pager.hidden = false;
      pager.querySelector('[data-turn="-1"]').disabled = page <= 0;
      pager.querySelector('[data-turn="1"]').disabled = page >= n - 1;
      const at = pager.querySelector('.fp-at');
      at.innerHTML = count() ? `Page <b>${page + 1}</b> of ${n}${n > 1 ? arrow('M7 10l5 5 5-5') : ''}` : 'No cards';
      at.disabled = n < 2;
      at.setAttribute('aria-label', count() ? `Page ${page + 1} of ${n}. Tap to pick a page.` : 'No cards');
    }
    function turn(by) {
      const to = Math.max(0, Math.min(pages() - 1, page + by));
      if (to === page) return;
      page = to;
      pk.classList.remove('turn-next', 'turn-back');
      void pk.offsetWidth;
      pk.classList.add(by > 0 ? 'turn-next' : 'turn-back');
      show();
    }

    // The tapped card, in full, on the left page. On a phone the left page turns over the pockets until it's closed.
    async function detail(href) {
      tiles().forEach((t) => t.classList.toggle('on', t.getAttribute('href') === href));
      bf.classList.add('reading');
      left.classList.add('busy');
      let html;
      try { html = await load(href); } catch (_) { left.classList.remove('busy'); return; }
      const t = document.createElement('template'); t.innerHTML = html;
      const d = t.content.querySelector('.detail');
      left.classList.remove('busy');
      if (!d || !bf.classList.contains('reading')) return;
      stopLeft?.(); stopLeft = null;
      const x = document.createElement('button');
      x.type = 'button'; x.className = 'bf-close'; x.setAttribute('aria-label', 'Close this card');
      x.innerHTML = arrow('M6 6l12 12M18 6L6 18') + '<span>Back to the page</span>';
      // "More like this" comes along; tapping one of those shows it here too.
      const more = [...t.content.querySelectorAll('section')].find((sec) => sec !== d && sec.querySelector('.rail'));
      if (more) { more.className = 'bf-more-like'; more.querySelectorAll('.rail > a.card:nth-child(n+7)').forEach((c) => c.remove()); }
      box.replaceChildren(x, d, ...(more ? [more] : []));
      box.hidden = false; intro.hidden = true; left.scrollTop = 0;
      if (d.dataset.hit && navigator.sendBeacon) navigator.sendBeacon('/hit/' + d.dataset.hit);
      const [holo, lang] = await loadTools();
      if (!box.contains(d)) return;
      stopLeft = holo.mountHolo(box);
      lang.mountLang(box);
    }
    function closeDetail() {
      stopLeft?.(); stopLeft = null;
      box.replaceChildren(); box.hidden = true; intro.hidden = false;
      bf.classList.remove('reading');
      tiles().forEach((t) => t.classList.remove('on'));
    }

    bf.addEventListener('click', (e) => {
      if (swiped) { e.preventDefault(); e.stopPropagation(); swiped = false; return; }
      const a = e.target.closest('a[href^="/products/"]');
      if (a && !a.classList.contains('bf-full') && (a.closest('.bf-pk') || a.closest('.bf-detail'))) { e.preventDefault(); detail(a.getAttribute('href')); return; }
      if (e.target.closest('.bf-close')) closeDetail();
      if (e.target.closest('.bf-reset')) slot.querySelector('.fd-clear')?.click();
    });
    // A sideways swipe across the pockets turns the page.
    let sx = null;
    pk.addEventListener('pointerdown', (e) => { sx = { x: e.clientX, y: e.clientY }; swiped = false; });
    pk.addEventListener('pointerup', (e) => {
      if (!sx) return;
      const dx = e.clientX - sx.x, dy = e.clientY - sx.y; sx = null;
      if (Math.abs(dx) > 48 && Math.abs(dx) > Math.abs(dy) * 1.4) { swiped = true; turn(dx < 0 ? 1 : -1); setTimeout(() => { swiped = false; }, 350); }
    });

    // Search and filter, always there above the binder. Until it's ready the server's cards are already on the page.
    startFinder(slot, bf.dataset.handle).then((m) => {
      if (!dialog.open) { m.free(); return; }
      slot.mountedFinder = m; mounted.add(m);
      slot.querySelector('.bf-wait')?.remove();
      slot.querySelector('.fd-filters')?.removeAttribute('open');
      const fg = slot.querySelector('.fd-grid');
      if (!fg) return;
      watch.disconnect();
      grid.remove(); bf.querySelector('.bf-more')?.remove();
      pk.append(fg);
      grid = fg; live = true; page = 0;
      watch = new MutationObserver(soon); watch.observe(grid, { childList: true });
      // A new search starts again from the first page.
      const c = slot.querySelector('.fd-count');
      if (c) new MutationObserver(() => { page = 0; soon(); }).observe(c, { childList: true, characterData: true, subtree: true });
      bf.classList.add('live');
      show();
    }).catch((e) => {
      console.error('Binder search could not start', e);
      try { navigator.sendBeacon('/oops', `binder search: ${String(e?.stack ?? e?.message ?? e).slice(0, 220)} | ${navigator.userAgent.slice(0, 90)}`); } catch (_) {}
      const w = slot.querySelector('.bf-wait'); if (w) w.textContent = 'Search isn’t working right now. You can still turn the pages.';
    });

    const api = { paint, turn, go: (n) => turn(n - page), info: () => ({ page, pages: pages() }), free: () => { stopLeft?.(); stopLeft = null; } };
    bf.book = api;
    show();
    return api;
  }

  const clear = () => { stopHolo?.(); stopHolo = null; };
  const titleOf = () => panel.querySelector('[data-card-title]')?.dataset.cardTitle || '';

  function backButton(label) {
    const b = document.createElement('button');
    b.type = 'button'; b.className = 'float-back';
    const t = document.createElement('span'); t.textContent = label; b.append(t);
    b.addEventListener('click', () => history.back());
    return b;
  }
  function paintBack() {
    panel.querySelector(':scope > .float-back')?.remove();
    if (trail.length) panel.prepend(backButton(trail.at(-1) ? `Back to ${trail.at(-1)}` : 'Back'));
  }

  // The search program and the cards it searches. A browser holding an old or damaged copy gets one more go with brand new files.
  async function startFinder(slot, handle) {
    const attempt = async (fresh) => {
      const q = fresh ? `&fresh=${Date.now()}` : '';
      // The same version as the search page (set once in base.html), so both load the same pair of files.
      const v = document.querySelector('meta[name="finder-v"]')?.content || '12';
      const finder = await import(`/static/wasm/finder.js?v=${v}${q}`);
      await finder.default({ module_or_path: `/static/wasm/finder_bg.wasm?v=${v}${q}` });
      const m = await finder.start_in_binder(slot, fresh ? `/cards.tsv?fresh=${Date.now()}` : '/cards.tsv', handle);
      if (!m) throw new Error(`No cards found for ${handle}`);
      return m;
    };
    try { return await attempt(false); } catch (e) {
      console.warn('Binder search: trying again with fresh files', e);
      slot.replaceChildren();
      return attempt(true);
    }
  }

  async function fill(html, thumb, href) {
    clear();
    if (shown && panel.querySelector(':scope > :is(.bf, .ask)')) kept.set(shown, { nodes: [...panel.childNodes], top: panel.scrollTop });
    shown = href;
    panel.innerHTML = html;
    panel.scrollTop = 0;
    paintBack();
    const detail = panel.querySelector('[data-card-title]');
    if (detail) document.title = `${detail.dataset.cardTitle} · ${baseTitle.split(' · ').pop()}`;
    const pic = panel.querySelector('.detail-img > img');
    // The small picture shows straight away while the sharp one loads.
    if (pic && thumb?.currentSrc) { pic.style.backgroundImage = `url("${thumb.currentSrc.replace(/"/g, '%22')}")`; }
    if (detail?.dataset.hit && navigator.sendBeacon) navigator.sendBeacon('/hit/' + detail.dataset.hit);
    document.dispatchEvent(new CustomEvent('float:filled', { detail: panel }));
    const more = panel.querySelector('.bf-more');
    if (more) moreCards(more);
    const bf = panel.querySelector(':scope > .bf');
    if (bf) book(bf);
    syncPager();
    const [holo, lang] = await loadTools();
    if (!dialog.open || shown !== href) return;
    stopHolo = holo.mountHolo(panel);
    lang.mountLang(panel);
  }

  // A big binder brings in more cards as they scroll near the end. The button does the same for anyone who taps it.
  function moreCards(btn) {
    const grid = btn.closest('.bf').querySelector('.bf-grid');
    let busy = false;
    const get = async () => {
      if (busy || !btn.isConnected) return;
      busy = true;
      seen.disconnect();
      try {
        const html = await (await fetch(btn.dataset.more, { credentials: 'same-origin' })).text();
        const t = document.createElement('template'); t.innerHTML = html;
        const next = t.content.querySelector('.bf-more');
        next?.remove();
        grid.append(t.content);
        if (next) { btn.dataset.more = next.dataset.more; busy = false; seen.observe(btn); } else btn.remove();
      } catch (_) { busy = false; }
    };
    const seen = new IntersectionObserver((list) => { if (list.some((e) => e.isIntersecting)) get(); }, { root: panel, rootMargin: '500px' });
    seen.observe(btn);
    btn.addEventListener('click', get);
  }

  function raise() {
    // A filter or sort left an htmx entry here. Closing the window steps back onto it, and htmx would then reload the
    // whole page (it keeps no copy). So while the window is open it is an ordinary entry; closing puts htmx's mark back.
    if (history.state?.htmx) history.replaceState({ floatBase: true }, '', location.href);
    baseUrl = location.href; baseTitle = document.title;
    document.documentElement.style.setProperty('--sbw', `${innerWidth - document.documentElement.clientWidth}px`);
    document.documentElement.dataset.float = '1';
    dialog.showModal();
    panel.focus({ preventScroll: true });
  }

  async function open(href, link) {
    if (closing) await closing;
    const mine = ++asking;
    let html;
    try { html = await load(href); } catch (_) { location.href = href; return; }
    if (mine !== asking) return;
    if (href.startsWith('/trainers/') || href.startsWith('/collections/')) cache.delete(href); // these change often, a card's page doesn't
    if (!dialog?.isConnected) build(); // made again if the page was swapped out under it
    const thumb = link?.querySelector('.bk-win img') || link?.querySelector('img');
    if (dialog.open) {
      if (href === shown) return;
      trail = [...trail, titleOf()];
      depth += 1;
      history.pushState({ float: href, base: baseUrl, depth, trail }, '', href);
      return fill(html, thumb, href);
    }
    trail = []; depth = 1;
    const show = () => { raise(); history.pushState({ float: href, base: baseUrl, depth, trail }, '', href); fill(html, thumb, href); };
    // The tapped card grows into the window, and shrinks back when it closes, where the browser can do it.
    if (!document.startViewTransition || !thumb || matchMedia('(prefers-reduced-motion: reduce)').matches) { show(); return; }
    thumb.style.viewTransitionName = 'vt-card';
    const move = document.startViewTransition(() => {
      thumb.style.viewTransitionName = '';
      show();
      const big = panel.querySelector(':scope > .detail .detail-img > img, :scope > .bf .bf-book');
      if (big) big.style.viewTransitionName = 'vt-card';
    });
    move.ready.catch(() => {});
    move.finished.catch(() => {}).finally(() => panel.querySelector(':scope > .detail .detail-img > img, :scope > .bf .bf-book')?.style.removeProperty('view-transition-name'));
  }

  /* ---------- a picture scaled up, filling the window, with a way back ---------- */
  function zoom(btn) {
    const group = btn.closest('.v-pics');
    const list = group ? [...group.querySelectorAll('[data-zoom]')] : [btn];
    if (!dialog?.isConnected) build(); // made again if the page was swapped out under it
    const inWindow = dialog.open;
    if (!inWindow) { raise(); clear(); panel.replaceChildren(); shown = null; trail = []; depth = 0; }
    const label = btn.dataset.zoomBack || (inWindow && titleOf() ? `Back to ${titleOf()}` : 'Back');
    const box = document.createElement('div'); box.className = 'float-zoom';
    box.append(backButton(label));
    const rail = document.createElement('div'); rail.className = 'z-rail';
    list.forEach((b) => {
      const slide = document.createElement('div'); slide.className = 'z-slide';
      const img = document.createElement('img'); img.src = b.dataset.zoom; img.alt = '';
      if (b.classList.contains('tp-pic')) img.className = 'round';
      slide.append(img); rail.append(slide);
      if (b.hasAttribute('data-loupe')) loupe(slide, img);
    });
    box.append(rail);
    zoomed = { top: panel.scrollTop, height: panel.style.height };
    // The window keeps its size, at least large enough to do the picture justice.
    const tall = Math.max(panel.getBoundingClientRect().height, Math.min(innerHeight * 0.7, innerWidth * 1.1));
    panel.style.height = `${Math.min(tall, innerHeight - 68)}px`;
    panel.classList.add('zoomed');
    panel.append(box);
    syncPager();
    rail.scrollLeft = list.indexOf(btn) * rail.clientWidth;
    box.querySelector('.float-back').focus({ preventScroll: true });
    depth += 1;
    history.pushState({ float: shown || location.href, base: baseUrl, depth, trail, zoom: true }, '', location.href);
  }
  // A magnifier that follows the mouse or finger, for checking corners, centering and scratches.
  function loupe(slide, img) {
    const POWER = 3;
    const lens = document.createElement('div'); lens.className = 'z-loupe'; lens.hidden = true;
    lens.style.backgroundImage = `url("${img.src.replace(/"/g, '%22')}")`;
    const tip = document.createElement('p'); tip.className = 'z-tip'; tip.textContent = matchMedia('(hover: hover)').matches ? 'Move over the card to magnify it' : 'Touch and drag on the card to magnify it';
    slide.append(lens, tip);
    const follow = (e) => {
      const r = img.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top;
      if (x < 0 || y < 0 || x > r.width || y > r.height) { lens.hidden = true; return; }
      lens.hidden = false; tip.hidden = true;
      const size = lens.offsetWidth, box = slide.getBoundingClientRect();
      lens.style.backgroundSize = `${r.width * POWER}px ${r.height * POWER}px`;
      lens.style.backgroundPosition = `${size / 2 - x * POWER}px ${size / 2 - y * POWER}px`;
      // A finger would cover the lens, so it sits above the touch. Near an edge or a corner it slides back toward the middle,
      // so the whole lens always stays in view, still showing the spot under the pointer.
      const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
      lens.style.left = `${clamp(e.clientX - box.left - size / 2, 0, Math.max(0, box.width - size))}px`;
      lens.style.top = `${clamp(e.clientY - box.top - size / 2 - (e.pointerType === 'touch' ? size * 0.7 : 0), 0, Math.max(0, box.height - size))}px`;
    };
    img.style.touchAction = 'none';
    img.draggable = false;
    img.addEventListener('pointerdown', follow);
    img.addEventListener('pointermove', follow);
    img.addEventListener('pointerleave', () => { lens.hidden = true; });
    img.addEventListener('pointerup', (e) => { if (e.pointerType === 'touch') lens.hidden = true; });
  }
  function unzoom() {
    if (!zoomed) return;
    panel.querySelector(':scope > .float-zoom')?.remove();
    panel.classList.remove('zoomed');
    panel.style.height = zoomed.height;
    panel.scrollTop = zoomed.top;
    zoomed = null;
    syncPager();
  }

  function hide() {
    if (!dialog?.open) return;
    asking++;
    clear();
    unzoom();
    dialog.close();
    pager.hidden = true;
    pager.querySelector('.fp-pick').hidden = true;
    panel.querySelectorAll('.bf').forEach((b) => b.book?.free());
    kept.forEach((k) => k.nodes.forEach((n) => n.querySelectorAll?.('.bf').forEach((b) => b.book?.free())));
    const gone = [...mounted];
    mounted.clear();
    kept.clear();
    // The window closes back into the spot it opened from, so it empties a moment later, unless it has opened again.
    setTimeout(() => {
      gone.forEach((m) => m.free());
      if (!dialog.open) panel.replaceChildren();
    }, matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 340);
    shown = null; depth = 0; trail = [];
    delete document.documentElement.dataset.float;
    document.documentElement.style.removeProperty('--sbw');
    document.title = baseTitle;
  }

  // Going back in the browser and the Back button step through the window. Tapping outside leaves it altogether.
  // Closing steps back through the browser's history, which lands a moment later.
  // Anything tapped in that moment waits for the close to finish, instead of being cancelled by it.
  let closing = null, closed = null;
  const done = () => { const ok = closed; closing = null; closed = null; ok?.(); };
  function leave() {
    const steps = history.state?.float ? history.state.depth : 0;
    if (steps > 0) {
      if (!closing) {
        closing = new Promise((ok) => { closed = ok; });
        setTimeout(() => { if (closed) { hide(); done(); } }, 700);
      }
      history.go(-steps);
    } else hide();
  }
  addEventListener('popstate', (e) => {
    const s = e.state;
    if (!s?.float) {
      hide(); done();
      if (s?.floatBase) history.replaceState({ htmx: true }, '', location.href);
      return;
    }
    if (!dialog?.open) return;
    unzoom();
    depth = s.depth; trail = s.trail || [];
    if (s.zoom || s.float === shown) { paintBack(); return; }
    const back = kept.get(s.float);
    if (back) {
      clear();
      kept.delete(s.float);
      shown = s.float;
      panel.replaceChildren(...back.nodes);
      paintBack();
      panel.scrollTop = back.top;
      syncPager();
      const t = panel.querySelector('[data-card-title]')?.dataset.cardTitle;
      if (t) document.title = `${t} · ${baseTitle.split(' · ').pop()}`;
      return;
    }
    load(s.float).then((html) => { if (dialog.open && history.state?.float === s.float) fill(html, null, s.float); }).catch(() => { location.href = s.float; });
  });

  document.addEventListener('click', (e) => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const z = e.target.closest?.('[data-zoom]');
    if (z) { e.preventDefault(); zoom(z); return; }
    const a = e.target.closest?.(SEL);
    // [data-no-float]: links inside it open as ordinary pages (the style guide's samples).
    if (!a || a.target === '_blank' || a.hasAttribute('download') || a.closest('[data-no-float]')) return;
    e.preventDefault();
    open(a.getAttribute('href'), a);
  });

  // Get a head start the moment a finger lands or a mouse rests on a card.
  const prepare = (e) => {
    const a = e.target.closest?.(SEL);
    if (a && !a.closest('[data-no-float]') && !(e.type === 'mouseover' && a.contains(e.relatedTarget))) { load(a.getAttribute('href')).catch(() => {}); loadTools().catch(() => {}); }
  };
  document.addEventListener('pointerdown', prepare, { passive: true });
  if (matchMedia('(hover: hover)').matches) document.addEventListener('mouseover', prepare, { passive: true });
})();
