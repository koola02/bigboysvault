// The Manager's tables (the vault's cards, a seller's listings).
// Sort: tap a column's name to sort by it; tap again to turn it round, once more to let it go. The column you tapped
// last leads, and the one before it settles the ties, so two columns can sort together. The server does the sorting
// (the address carries it, as "price:desc,card:asc"), so it holds for every card, not only the ones on screen.
// Widths: drag the line at the right of a column's name. Double-click the line to set that column back.
// Widths are remembered on this device.
(() => {
  const el = (tag, cls, txt) => { const n = document.createElement(tag); if (cls) n.className = cls; if (txt != null) n.textContent = txt; return n; };

  // "36px minmax(230px, 1.5fr) 132px" → ["36px", "minmax(230px, 1.5fr)", "132px"]
  const tracks = (text) => {
    const out = []; let depth = 0, cur = '';
    for (const ch of text.trim()) {
      if (ch === '(') depth++; else if (ch === ')') depth--;
      if (/\s/.test(ch) && !depth) { if (cur) out.push(cur); cur = ''; } else cur += ch;
    }
    if (cur) out.push(cur);
    return out;
  };
  // The narrowest a column can be drawn: "132px" → 132, "minmax(230px, 1.5fr)" → 230.
  const least = (t) => { const m = /^minmax\(\s*([\d.]+)px/.exec(t) || /^([\d.]+)px$/.exec(t); return m ? parseFloat(m[1]) : 0; };
  const DEFAULTS = new WeakMap(); // the table → its columns as the stylesheet has them, read before any are changed

  const key = (root) => 'bbv:cols:' + root.id;
  const stored = (root) => { try { const w = JSON.parse(localStorage.getItem(key(root)) || '{}'); return w && typeof w === 'object' ? w : {}; } catch (_) { return {}; } };
  const keep = (root, w) => { try { Object.keys(w).length ? localStorage.setItem(key(root), JSON.stringify(w)) : localStorage.removeItem(key(root)); } catch (_) {} };

  function apply(root, head, w) {
    const def = DEFAULTS.get(root); if (!def) return;
    const cells = [...head.children];
    const mine = cells.some((c) => c.dataset.col && w[c.dataset.col]);
    if (!mine) { root.style.removeProperty('--cols'); root.style.removeProperty('--minw'); return; }
    const t = def.map((d, i) => { const k = cells[i] && cells[i].dataset.col; return k && w[k] ? `${Math.round(w[k])}px` : d; });
    root.style.setProperty('--cols', t.join(' '));
    root.style.setProperty('--minw', `${Math.ceil(t.reduce((a, x) => a + least(x), 0))}px`);
  }

  const read = (spec) => String(spec || '').split(',').map((s) => s.split(':')).filter((p) => p.length === 2 && p[0].trim()).slice(0, 2).map(([c, d]) => [c.trim(), d.trim() === 'desc' ? 'desc' : 'asc']);
  const write = (state) => state.map(([c, d]) => `${c}:${d}`).join(',');
  // The column tapped last leads. Tapping the leader turns it round, then lets it go; tapping the second makes it lead.
  function next(state, col, first) {
    const [lead, tie] = state;
    if (lead && lead[0] === col) return lead[1] === first ? [[col, first === 'asc' ? 'desc' : 'asc'], ...(tie ? [tie] : [])] : (tie ? [tie] : []);
    if (tie && tie[0] === col) return [tie, lead];
    return [[col, first], ...(lead ? [lead] : [])];
  }

  function setup(root) {
    const head = root.querySelector('.mgr-thead');
    if (!head || head.dataset.ready) return;
    head.dataset.ready = '1';
    if (!DEFAULTS.has(root)) {
      root.style.removeProperty('--cols');
      DEFAULTS.set(root, tracks(getComputedStyle(root).getPropertyValue('--cols')));
    }
    let w = stored(root);
    apply(root, head, w);

    [...head.children].forEach((c) => { if (c.dataset.col && !c.dataset.name) c.dataset.name = c.textContent.trim(); });
    const select = root.querySelector('select[name="sort"]');
    const state = read(head.dataset.sort).filter(([c]) => head.querySelector(`[data-col="${c}"]`));
    const cellOf = (c) => head.querySelector(`[data-col="${c}"]`);
    const nameOf = (c) => cellOf(c).dataset.name;
    const wayOf = ([c, d]) => cellOf(c).dataset[d === 'desc' ? 'down' : 'up'] || (d === 'desc' ? 'downwards' : 'upwards');
    const said = (st) => st.map((s, i) => `${i ? 'then ' : ''}${nameOf(s[0])} (${wayOf(s)})`).join(', ');

    function go(st) {
      if (!select) return;
      const spec = write(st);
      select.querySelectorAll('option[data-columns]').forEach((o) => { if (o.value !== spec) o.remove(); });
      if (spec && ![...select.options].some((o) => o.value === spec)) { const o = el('option', null, 'By column: ' + said(st)); o.value = spec; o.dataset.columns = ''; select.append(o); }
      select.value = spec;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    }

    // The line above the table: what the order is, in words, and the way back.
    const line = el('p', 'mgr-sortline');
    const hasWidths = () => [...head.children].some((c) => c.dataset.col && w[c.dataset.col]);
    function drawLine() {
      line.replaceChildren();
      if (state.length) {
        line.append('Sorted by ');
        state.forEach((s, i) => { if (i) line.append(', then '); line.append(el('b', null, nameOf(s[0])), ` (${wayOf(s)})`); });
        line.append('. ');
        const back = el('button', 'linkbtn', select && select.options[0] ? `Back to ${select.options[0].textContent.trim().replace(/\s*\(.*\)$/, '').toLowerCase()}` : 'Clear'); back.type = 'button';
        back.onclick = () => go([]);
        line.append(back);
      } else line.append('Tap a column’s name to sort by it. Tap a second one and the two sort together. Drag the line beside a name to change the column’s width.');
      if (hasWidths()) {
        const reset = el('button', 'linkbtn', 'Reset column widths'); reset.type = 'button';
        reset.onclick = () => { w = {}; keep(root, w); apply(root, head, w); drawLine(); };
        line.append(' · ', reset);
      }
    }
    (head.closest('.mgr-scroll') || head).before(line);

    // The option the server drew for a column sort gets its words.
    if (select && state.length) { const o = [...select.options].find((x) => x.value === write(state)); if (o) { o.textContent = 'By column: ' + said(state); o.dataset.columns = ''; } }

    head.setAttribute('role', 'row');
    [...head.children].forEach((cell) => {
      const col = cell.dataset.col; if (!col) return;
      const at = state.findIndex((s) => s[0] === col);
      cell.setAttribute('role', 'columnheader'); cell.tabIndex = 0;
      if (at >= 0) {
        cell.dataset.dir = state[at][1];
        if (state.length > 1) cell.append(el('i', 'mgr-ord', String(at + 1)));
        if (at === 0) cell.setAttribute('aria-sort', state[0][1] === 'desc' ? 'descending' : 'ascending');
      }
      cell.title = at === 0 ? `Sorted by ${nameOf(col)} (${wayOf(state[0])}). Tap to change.` : `Sort by ${nameOf(col)}`;
      const sort = () => go(next(state, col, cell.dataset.first === 'desc' ? 'desc' : 'asc'));
      cell.addEventListener('click', (e) => { if (!e.target.closest('.mgr-rz')) sort(); });
      cell.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); sort(); } });

      // The line you drag to make the column wider or narrower.
      const rz = el('i', 'mgr-rz'); rz.title = 'Drag to change the width. Double-click to set it back.'; rz.setAttribute('aria-hidden', 'true');
      cell.append(rz);
      const min = Number(cell.dataset.min) || 80;
      rz.addEventListener('pointerdown', (e) => {
        if (e.button) return;
        e.preventDefault(); e.stopPropagation();
        const x0 = e.clientX, w0 = cell.getBoundingClientRect().width;
        // Holding the pointer keeps the drag going when it strays off the thin line; the page listens too, in case it can't be held.
        try { rz.setPointerCapture(e.pointerId); } catch (_) {}
        rz.classList.add('on'); root.classList.add('mgr-resizing');
        const move = (ev) => { w = { ...w, [col]: Math.max(min, Math.min(900, w0 + ev.clientX - x0)) }; apply(root, head, w); };
        const end = () => {
          removeEventListener('pointermove', move); removeEventListener('pointerup', end); removeEventListener('pointercancel', end);
          rz.classList.remove('on'); root.classList.remove('mgr-resizing');
          keep(root, w); drawLine();
        };
        addEventListener('pointermove', move); addEventListener('pointerup', end); addEventListener('pointercancel', end);
      });
      rz.addEventListener('dblclick', (e) => { e.preventDefault(); e.stopPropagation(); const { [col]: _gone, ...rest } = w; w = rest; keep(root, w); apply(root, head, w); drawLine(); });
      rz.addEventListener('click', (e) => e.stopPropagation());
    });
    drawLine();
  }

  const all = () => document.querySelectorAll('#cardsRoot, #listings').forEach(setup);
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', all); else all();
  // The vault's table is drawn again whenever it is searched, filtered or sorted.
  document.addEventListener('htmx:afterSwap', all);
  document.addEventListener('htmx:afterSettle', all);
})();
