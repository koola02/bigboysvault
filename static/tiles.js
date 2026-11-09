// Small touches for every card tile: a picture that fails never leaves a blank box, tilt and foil for a mouse, and Ctrl+K to search.
(() => {
  // A set symbol that doesn't exist just goes away. A card picture that fails shows the vault placeholder.
  document.addEventListener('error', (e) => {
    const n = e.target;
    if (n.tagName !== 'IMG') return;
    if (n.classList.contains('card-sym')) n.remove();
    else if (n.parentElement?.classList.contains('card-img')) n.parentElement.classList.add('no-pic');
  }, true);

  // Once a picture is in: the placeholder goes, and a photo that isn't card-shaped gets a soft copy of itself filling the gaps. It reuses the file already downloaded.
  const got = (img) => {
    const box = img.parentElement;
    if (!box?.classList.contains('card-img')) return;
    box.classList.add('got');
    if (img.naturalWidth && Math.abs(img.naturalWidth / img.naturalHeight - 5 / 7) > 0.06) {
      box.style.setProperty('--pic', `url("${(img.currentSrc || img.src).replace(/"/g, '%22')}")`);
      box.classList.add('fill');
    }
  };
  document.addEventListener('load', (e) => { if (e.target.tagName === 'IMG') got(e.target); }, true);
  document.querySelectorAll('.card-img > img').forEach((i) => { if (i.complete && i.naturalWidth) got(i); });

  // Ctrl+K or ⌘K jumps to the search box, wherever you are.
  document.addEventListener('keydown', (e) => {
    if (e.key.toLowerCase() !== 'k' || !(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey || document.querySelector('dialog[open]')) return;
    const box = document.querySelector('input[data-suggest]');
    if (box && box.offsetParent !== null) { e.preventDefault(); box.focus(); box.select(); }
  });

  // Fingers scroll past tiles, so tilting is only for a mouse. Cards in the floating window and the big 3D stage have their own.
  if (!matchMedia('(hover: hover) and (pointer: fine)').matches || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  let on = null, frame = 0, x = 0, y = 0;
  const drop = () => { if (!on) return; on.classList.remove('tilting'); on.style.removeProperty('--rx'); on.style.removeProperty('--ry'); on = null; };
  const paint = () => {
    frame = 0;
    if (!on) return;
    const r = on.parentElement.getBoundingClientRect(); // the card, which doesn't move, so the tilt can't feed back on itself
    const box = on.getBoundingClientRect();
    const px = Math.min(1, Math.max(0, (x - r.left) / r.width)), py = Math.min(1, Math.max(0, (y - box.top) / box.height));
    on.style.setProperty('--ry', `${((px - 0.5) * 18).toFixed(2)}deg`);
    on.style.setProperty('--rx', `${((0.5 - py) * 18).toFixed(2)}deg`);
    on.style.setProperty('--gx', `${(px * 100).toFixed(1)}%`);
    on.style.setProperty('--gy', `${(py * 100).toFixed(1)}%`);
  };
  document.addEventListener('pointermove', (e) => {
    const img = e.target.closest?.('a.card .card-img');
    if (img !== on) { drop(); if (img) { on = img; on.classList.add('tilting'); } }
    if (!on) return;
    x = e.clientX; y = e.clientY;
    if (!frame) frame = requestAnimationFrame(paint);
  }, { passive: true });
  document.addEventListener('pointerleave', drop);
})();
