// Search suggestions: inline completion ("ghost" text) + a ranked list with probabilities. Works on every input[data-suggest].
(() => {
  const el = (tag, cls, txt) => { const n = document.createElement(tag); if (cls) n.className = cls; if (txt != null) n.textContent = txt; return n; };
  const cache = new Map();

  function setup(input) {
    if (input.dataset.sgReady) return; input.dataset.sgReady = '1';
    const wrap = el('div', 'sg'); input.parentNode.insertBefore(wrap, input); wrap.appendChild(input);
    const ghost = el('div', 'sg-ghost'); ghost.setAttribute('aria-hidden', 'true'); const gTyped = el('span'); const gRest = el('span', 'rest'); ghost.append(gTyped, gRest);
    const panel = el('div', 'sg-panel'); panel.hidden = true; panel.setAttribute('role', 'listbox'); wrap.append(ghost, panel);
    input.setAttribute('role', 'combobox'); input.setAttribute('aria-autocomplete', 'both'); input.setAttribute('aria-expanded', 'false');
    let data = null, active = -1, seq = 0, timer, ctl;

    // Newer browsers show the list in the top layer, attached to the box, so nothing can clip it. Others keep the plain panel.
    const topLayer = 'popover' in HTMLElement.prototype && CSS.supports('anchor-name: --x');
    if (topLayer) { panel.popover = 'manual'; wrap.classList.add('sg-top'); }
    const show = () => { panel.hidden = false; if (topLayer && !panel.matches(':popover-open')) panel.showPopover(); };
    const close = () => { if (topLayer && panel.matches(':popover-open')) panel.hidePopover(); panel.hidden = true; input.setAttribute('aria-expanded', 'false'); active = -1; gRest.textContent = ''; };
    function syncGhostStyle() {
      const cs = getComputedStyle(input);
      Object.assign(ghost.style, { font: cs.font, letterSpacing: cs.letterSpacing, paddingLeft: cs.paddingLeft, paddingRight: cs.paddingRight, top: cs.borderTopWidth, left: cs.borderLeftWidth });
    }
    function setGhost() {
      gTyped.textContent = ''; gRest.textContent = '';
      const v = input.value; if (!data || !data.ghost || !v) return;
      if (!data.ghost.toLowerCase().startsWith(v.toLowerCase()) || input.selectionStart !== v.length) return;
      syncGhostStyle(); gTyped.textContent = v; gRest.textContent = data.ghost.slice(v.length);
    }
    function choose(item) {
      if (item.href) { location.href = item.href; return; }
      input.value = item.text; close(); setGhost();
      if (input.form && input.form.hasAttribute('hx-get')) input.dispatchEvent(new Event('input', { bubbles: true }));
      else input.form && input.form.requestSubmit();
    }
    function render() {
      panel.replaceChildren();
      if (!data || (!data.items.length && !data.cards.length)) { close(); return; }
      const v = input.value.trim().toLowerCase();
      if (data.items.length) {
        panel.append(el('div', 'sg-sec', data.items[0].typo ? 'Did you mean' : 'Suggestions'));
        data.items.forEach((it, i) => {
          const b = el('button', 'sg-item'); b.type = 'button'; b.setAttribute('role', 'option'); b.dataset.i = i;
          b.style.setProperty('--pf', Math.max(0.02, Math.min(1, it.p)).toFixed(3));
          const t = el('span', 'sg-t'); const lower = it.text.toLowerCase();
          if (!it.typo && lower.startsWith(v)) { t.append(document.createTextNode(it.text.slice(0, v.length)), el('b', null, it.text.slice(v.length))); } else t.textContent = it.text;
          const meta = el('span', 'sg-meta'); if (it.kind !== 'card') meta.append(el('span', 'sg-k', it.kind));
          meta.append(el('span', 'sg-p', Math.max(1, Math.round(it.p * 100)) + '%')); b.append(t, meta);
          b.onmousedown = (e) => e.preventDefault(); b.onclick = () => choose(it); panel.append(b);
        });
      }
      if (data.cards.length) {
        panel.append(el('div', 'sg-sec', 'Cards'));
        for (const c of data.cards) {
          const a = el('a', 'sg-card'); a.href = '/products/' + c.handle; a.setAttribute('role', 'option');
          const im = el('img'); im.src = c.img; im.alt = ''; im.width = 36; im.height = 50; const s = el('span', 'sg-ct'); s.append(el('b', null, c.title), el('small', null, c.price)); a.append(im, s); panel.append(a);
        }
      }
      panel.append(el('p', 'sg-note', 'The percentage is the estimated chance that this is what you mean, learned from your collection and past searches.'));
      show(); input.setAttribute('aria-expanded', 'true');
    }
    async function fetchFor(q) {
      if (cache.has(q)) return cache.get(q);
      ctl?.abort(); ctl = new AbortController();
      const r = await fetch('/suggest?q=' + encodeURIComponent(q), { signal: ctl.signal, headers: { Accept: 'application/json' } });
      const j = await r.json(); cache.set(q, j); return j;
    }
    input.addEventListener('input', () => {
      clearTimeout(timer); const q = input.value;
      // The card finder is showing live results for this box, so a suggestion list would only cover them.
      if (document.documentElement.dataset.finder) { data = null; close(); setGhost(); return; }
      if (!q.trim()) { data = null; close(); setGhost(); return; }
      const my = ++seq; timer = setTimeout(async () => {
        try { const j = await fetchFor(q); if (my !== seq) return; data = j; active = -1; render(); setGhost(); } catch (_) { /* aborted or offline: stay quiet */ }
      }, 70);
    });
    input.addEventListener('keydown', (e) => {
      const opts = [...panel.querySelectorAll('.sg-item')];
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        if (panel.hidden || !opts.length) return; e.preventDefault();
        active = (active + (e.key === 'ArrowDown' ? 1 : -1) + opts.length) % opts.length;
        opts.forEach((o, i) => o.setAttribute('aria-selected', String(i === active))); opts[active].scrollIntoView({ block: 'nearest' });
      } else if ((e.key === 'Tab' || e.key === 'ArrowRight') && data?.ghost && gRest.textContent && input.selectionStart === input.value.length) {
        e.preventDefault(); input.value = data.ghost; input.dispatchEvent(new Event('input', { bubbles: true }));
      } else if (e.key === 'Enter' && active >= 0 && data) { e.preventDefault(); choose(data.items[active]); }
      else if (e.key === 'Escape') close();
    });
    input.addEventListener('blur', () => setTimeout(close, 120));
    input.addEventListener('focus', () => { if (data && input.value.trim()) render(); });
  }

  const scan = (root = document) => root.querySelectorAll('input[data-suggest]').forEach(setup);
  scan(); document.addEventListener('htmx:load', (e) => scan(e.target));
})();
