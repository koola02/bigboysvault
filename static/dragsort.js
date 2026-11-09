/* Drag to reorder, for admin lists and grids (page sections, binders). Pointer events: mouse, finger or pen.
   dragSort(listOrLists, { item, key, handle, onDrop })
   - handle: a selector inside each item to grab (page sections). Without it the whole item is grabbed (binders):
     with a mouse the drag starts after a small move, so a plain click still works; on touch, press and hold for a moment
     (so swiping still scrolls the page).
   - Several lists can share one drag (binders: "on the home page" and "hidden"); drop into any of them, even an empty one.
   - onDrop(keysPerList): an array with each list's keys in their new order. Only called when something changed. */
(() => {
  window.dragSort = function dragSort(lists, { item, key, handle, onDrop }) {
    lists = (Array.isArray(lists) ? lists : [lists]).filter(Boolean);
    if (!lists.length || lists[0].dataset.sortable) return;
    lists.forEach((l) => { l.dataset.sortable = '1'; });
    const snapshot = () => lists.map((l) => [...l.querySelectorAll(item)].map(key).join('|')).join('#');
    let pend = null, drag = null, suppressClick = false;

    function start(li, x, y, pointerId) {
      const r = li.getBoundingClientRect();
      const ph = document.createElement(li.tagName); ph.className = 'drag-ph'; ph.style.height = r.height + 'px';
      // In a sideways row the gap keeps the card's width too.
      const across = getComputedStyle(li.parentElement).display.includes('flex') && getComputedStyle(li.parentElement).flexDirection.startsWith('row');
      if (across) { ph.style.width = r.width + 'px'; ph.style.flex = '0 0 auto'; }
      li.after(ph);
      const style = li.getAttribute('style');
      // The nearest box that scrolls sideways (a row of cards, the binder slider) follows the finger near its edges.
      let side = li.parentElement;
      while (side && side !== document.body && !(side.scrollWidth > side.clientWidth + 2 && /auto|scroll/.test(getComputedStyle(side).overflowX))) side = side.parentElement;
      if (side === document.body) side = null;
      if (side) side.style.scrollSnapType = 'none';
      Object.assign(li.style, { position: 'fixed', left: r.left + 'px', top: r.top + 'px', width: r.width + 'px', height: r.height + 'px', zIndex: 80, pointerEvents: 'none' });
      li.classList.add('dragging'); document.body.classList.add('is-dragging');
      drag = { li, ph, style, side, dx: x - r.left, dy: y - r.top, before: snapshot(), id: pointerId, scroll: 0, sideScroll: 0, lastX: x, lastY: y };
      if (navigator.vibrate) navigator.vibrate(10);
      requestAnimationFrame(tick);
    }
    function place(x, y) {
      const { li, ph } = drag;
      li.style.left = (x - drag.dx) + 'px'; li.style.top = (y - drag.dy) + 'px';
      const under = document.elementFromPoint(x, y);
      const row = under && under.closest(`${item}, .drag-ph`);
      const list = under && lists.find((l) => l.contains(under));
      if (row && row !== ph && list) {
        const b = row.getBoundingClientRect();
        const grid = b.width < list.getBoundingClientRect().width * 0.75;
        const after = grid ? x > b.left + b.width / 2 : y > b.top + b.height / 2;
        const ref = after ? row.nextElementSibling : row;
        if (ref !== ph && (ph.parentNode !== list || ph.nextElementSibling !== ref)) list.insertBefore(ph, ref);
      } else if (list && !row && ph.parentNode !== list) {
        list.append(ph);               // empty list, or the gap after the last item
      } else if (list && !row && list.lastElementChild !== ph) {
        const last = [...list.querySelectorAll(item)].pop();
        if (!last || y > last.getBoundingClientRect().top) list.append(ph);
      }
      const edge = 80; drag.scroll = y < edge ? -14 : y > innerHeight - edge ? 14 : 0;   // scroll near the screen edges
      if (drag.side) { const s = drag.side.getBoundingClientRect(), e2 = Math.min(70, s.width / 6); drag.sideScroll = x < s.left + e2 ? -12 : x > s.right - e2 ? 12 : 0; }
    }
    function tick() {
      if (!drag) return;
      if (drag.scroll) scrollBy(0, drag.scroll);
      if (drag.sideScroll) drag.side.scrollLeft += drag.sideScroll;
      if (drag.scroll || drag.sideScroll) place(drag.lastX, drag.lastY);
      requestAnimationFrame(tick);
    }

    lists.forEach((list) => list.addEventListener('pointerdown', (e) => {
      if (e.button > 0 || drag) return;
      const grab = handle ? e.target.closest(handle) : e.target.closest(item);
      const li = grab && grab.closest(item);
      if (!li || !lists.some((l) => l.contains(li)) || (!handle && e.target.closest('[data-nodrag]'))) return;
      if (handle) { e.preventDefault(); start(li, e.clientX, e.clientY, e.pointerId); grab.setPointerCapture?.(e.pointerId); return; }
      pend = { li, x: e.clientX, y: e.clientY, id: e.pointerId, touch: e.pointerType !== 'mouse' };
      if (pend.touch) pend.timer = setTimeout(() => { if (pend) { const p = pend; pend = null; start(p.li, p.x, p.y, p.id); } }, 280);
    }));
    window.addEventListener('pointermove', (e) => {
      if (pend && e.pointerId === pend.id) {
        const moved = Math.hypot(e.clientX - pend.x, e.clientY - pend.y);
        if (pend.touch) { if (moved > 10) { clearTimeout(pend.timer); pend = null; } return; }   // a swipe: let the page scroll
        if (moved > 6) { const p = pend; pend = null; start(p.li, p.x, p.y, p.id); }
      }
      if (!drag || e.pointerId !== drag.id) return;
      drag.lastX = e.clientX; drag.lastY = e.clientY; place(e.clientX, e.clientY);
    });
    // While dragging on touch, stop the page from scrolling under the finger.
    window.addEventListener('touchmove', (e) => { if (drag) e.preventDefault(); }, { passive: false });
    async function end(e) {
      if (pend && (!e || e.pointerId === pend.id)) { clearTimeout(pend.timer); pend = null; }
      if (!drag || (e && e.pointerId !== drag.id)) return;
      const { li, ph, before, style, side } = drag; drag = null;
      ph.replaceWith(li); if (style == null) li.removeAttribute('style'); else li.setAttribute('style', style);
      if (side) side.style.scrollSnapType = ''; li.classList.remove('dragging'); document.body.classList.remove('is-dragging');
      suppressClick = true; setTimeout(() => { suppressClick = false; }, 0);
      if (snapshot() !== before) {
        li.classList.add('dropped'); setTimeout(() => li.classList.remove('dropped'), 700);
        await onDrop(lists.map((l) => [...l.querySelectorAll(item)].map(key)), li);
      }
    }
    window.addEventListener('pointerup', end); window.addEventListener('pointercancel', end);
    // A drag must not also count as a tap.
    lists.forEach((l) => l.addEventListener('click', (e) => { if (suppressClick) { e.stopPropagation(); e.preventDefault(); } }, true));
    lists.forEach((l) => l.addEventListener('contextmenu', (e) => { if (drag || pend) e.preventDefault(); }));
  };
})();
