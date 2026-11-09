// Every row on the home page that scrolls sideways gets a ‹ and › button, for people without a trackpad or touch screen.
// A button fades out at the end it can't go past, and both hide when the row fits without scrolling.
(() => {
  const SEL = 'main .rail, main .hl-grid, main .sc-many .sc-grid';
  const icon = (d) => `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="${d}"/></svg>`;
  const wire = (row) => {
    if (row.dataset.rx || row.closest('.eb, .float-panel')) return;
    row.dataset.rx = '1';
    const host = row.parentElement;
    host.classList.add('rx-host');
    const mk = (dir) => {
      const b = document.createElement('button');
      b.type = 'button'; b.className = `rx-btn rx-${dir}`; b.hidden = true;
      b.setAttribute('aria-label', dir === 'prev' ? 'Scroll back' : 'Scroll for more');
      b.innerHTML = icon(dir === 'prev' ? 'M15 5l-7 7 7 7' : 'M9 5l7 7-7 7');
      b.addEventListener('click', () => row.scrollBy({ left: (dir === 'prev' ? -1 : 1) * row.clientWidth * 0.8, behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' }));
      host.append(b);
      return b;
    };
    const prev = mk('prev'), next = mk('next');
    const update = () => {
      const max = row.scrollWidth - row.clientWidth;
      const fits = max <= 4;
      prev.hidden = next.hidden = fits;
      if (fits) return;
      const top = `${row.offsetTop + Math.min(row.clientHeight * 0.4, 150)}px`;
      prev.style.top = next.style.top = top;
      prev.dataset.off = row.scrollLeft <= 4 ? '1' : '0';
      next.dataset.off = row.scrollLeft >= max - 4 ? '1' : '0';
    };
    row.addEventListener('scroll', update, { passive: true });
    new ResizeObserver(update).observe(row);
    update();
  };
  const all = () => document.querySelectorAll(SEL).forEach(wire);
  all();
  addEventListener('load', all);
})();
