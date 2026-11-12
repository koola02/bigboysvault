/* The card camera, for a photo of a physical card. The camera shows a card-shaped frame in the middle; the photo is
   cut to exactly that frame, so every card picture comes out centred, upright and the same shape. A photo from the
   gallery opens in the same frame, centred and filling it, and can be dragged or zoomed to sit right. There is no
   card detection: only neat framing.
   data-card-cam="free" is a plain camera instead (the chat's "take a photo"): no frame and no cutting, on phones and
   computers alike; the photo is sent as taken.
   Any <input type="file" data-card-cam> uses it. The finished photo goes back into that input (with a "change"
   event), so the page's own preview and upload work exactly as before. */
(() => {
  const CARD = 63 / 88;
  const OUT_W = 900, OUT_H = Math.round(OUT_W / CARD);
  const coarse = () => matchMedia('(pointer: coarse)').matches;
  const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const svg = (d) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;
  const ICON = {
    x: '<path d="M18 6L6 18M6 6l12 12"/>',
    flip: '<path d="M3 7h13l-3-3M21 17H8l3 3"/>',
    pics: '<rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/>',
    cam: '<path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/>',
  };

  let ui = null;
  document.addEventListener('click', (e) => {
    const inp = e.target.closest && e.target.closest('input[type=file][data-card-cam]');
    if (!inp || inp.dataset.camPass) return;
    e.preventDefault(); e.stopPropagation();
    open(inp, inp.dataset.cardCam === 'free');
  }, true);

  function open(target, free) {
    close();
    const o = el('div', free ? 'cc free' : 'cc'); o.setAttribute('role', 'dialog'); o.setAttribute('aria-modal', 'true'); o.setAttribute('aria-label', free ? 'Take a photo' : 'Card photo');
    o.innerHTML = `
      <header class="cc-top">
        <button type="button" class="cc-ib" data-x aria-label="Close">${svg(ICON.x)}</button>
        <b class="cc-title">${free ? 'Take a photo' : 'Card photo'}</b>
        <button type="button" class="cc-ib" data-flip aria-label="Switch camera" hidden>${svg(ICON.flip)}</button>
      </header>
      <div class="cc-stage">
        <video class="cc-video" playsinline muted autoplay></video>
        <img class="cc-pic" alt="" hidden draggable="false">
        <div class="cc-frame"><i></i><i></i><i></i><i></i></div>
        <p class="cc-hint">Fit the card inside the frame</p>
      </div>
      <input class="cc-zoom" type="range" min="1" max="4" step="0.01" value="1" aria-label="Zoom" hidden>
      <footer class="cc-bot">
        <button type="button" class="cc-side" data-pick>${svg(ICON.pics)}<span>Photos</span></button>
        <button type="button" class="cc-shut" data-shoot aria-label="Take the photo"><span></span></button>
        <button type="button" class="cc-side" data-cam hidden>${svg(ICON.cam)}<span>Camera</span></button>
        <button type="button" class="btn cc-use" data-use hidden>${free ? 'Send photo' : 'Use photo'}</button>
      </footer>`;
    document.body.append(o);
    document.documentElement.classList.add('cc-on');
    const pick = el('input'); pick.type = 'file'; pick.accept = 'image/*'; pick.hidden = true; o.append(pick);
    ui = { o, target, pick, free: !!free, file: null, stream: null, facing: 'environment', img: null, x: 0, y: 0, s: 1, base: 1 };
    o.querySelector('[data-x]').addEventListener('click', close);
    o.querySelector('[data-pick]').addEventListener('click', () => pick.click());
    o.querySelector('[data-cam]').addEventListener('click', () => camera());
    o.querySelector('[data-flip]').addEventListener('click', () => { ui.facing = ui.facing === 'environment' ? 'user' : 'environment'; camera(); });
    o.querySelector('[data-shoot]').addEventListener('click', shoot);
    o.querySelector('[data-use]').addEventListener('click', use);
    pick.addEventListener('change', () => { const f = pick.files[0]; pick.value = ''; if (f) edit(f); });
    o.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
    addEventListener('resize', layout);
    gestures(o.querySelector('.cc-stage'));
    if (window.ResizeObserver) { ui.ro = new ResizeObserver(() => layout()); ui.ro.observe(o.querySelector('.cc-stage')); }
    o.querySelector('.cc-zoom').addEventListener('input', (e) => { zoomTo(Number(e.target.value)); });
    layout();
    // A phone opens the camera straight away; a computer opens the photo picker (a webcam is rarely what's wanted).
    // "Take a photo" opens the camera everywhere: that is what was asked for.
    if ((free || coarse()) && navigator.mediaDevices?.getUserMedia) camera(); else { mode('pick'); pick.click(); }
    o.querySelector('[data-x]').focus({ preventScroll: true });
  }

  function close() {
    if (!ui) return;
    stop();
    ui.ro?.disconnect();
    ui.o.remove();
    removeEventListener('resize', layout);
    document.documentElement.classList.remove('cc-on');
    ui = null;
  }
  function stop() { ui?.stream?.getTracks().forEach((t) => t.stop()); if (ui) ui.stream = null; }

  // The frame: as big as fits, card-shaped, in the middle of the stage.
  function frameRect() {
    const st = ui.o.querySelector('.cc-stage').getBoundingClientRect();
    const h = Math.min(st.height * 0.82, (st.width * 0.86) / CARD);
    const w = h * CARD;
    return { x: (st.width - w) / 2, y: (st.height - h) / 2, w, h, sw: st.width, sh: st.height };
  }
  function layout() {
    if (!ui) return;
    const f = frameRect(), fr = ui.o.querySelector('.cc-frame');
    Object.assign(fr.style, { left: `${f.x}px`, top: `${f.y}px`, width: `${f.w}px`, height: `${f.h}px` });
    if (ui.img && !ui.free) { fit(false); paint(); }
  }

  function mode(m) {
    const q = (s) => ui.o.querySelector(s);
    const live = m === 'live', editing = m === 'edit';
    q('.cc-video').hidden = !live; q('.cc-pic').hidden = !editing;
    q('[data-shoot]').hidden = !live; q('[data-use]').hidden = !editing;
    q('[data-pick]').hidden = false; q('[data-cam]').hidden = live || !navigator.mediaDevices?.getUserMedia;
    q('.cc-zoom').hidden = !editing || ui.free;
    q('.cc-hint').textContent = ui.free
      ? (live ? 'Tap the button to take the photo' : editing ? 'Send this photo, or take another' : 'Choose a photo')
      : (live ? 'Fit the card inside the frame' : editing ? 'Drag or zoom so the card fills the frame' : 'Choose a photo of the card');
  }

  async function camera() {
    stop();
    try {
      ui.stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: { ideal: ui.facing }, width: { ideal: 1920 }, height: { ideal: 1440 } }, audio: false });
    } catch (_) {
      mode('pick');
      ui.o.querySelector('.cc-hint').textContent = 'The camera is blocked here. Choose a photo instead, or allow the camera.';
      return;
    }
    const v = ui.o.querySelector('.cc-video');
    v.srcObject = ui.stream; await v.play().catch(() => {});
    ui.img = null; mode('live'); layout();
    navigator.mediaDevices.enumerateDevices?.().then((d) => { ui.o.querySelector('[data-flip]').hidden = d.filter((x) => x.kind === 'videoinput').length < 2; }).catch(() => {});
  }

  // The frame's part of the camera picture, at full camera resolution (the video is shown "cover", so work back).
  function shoot() {
    const v = ui.o.querySelector('.cc-video');
    if (!v.videoWidth) return;
    if (ui.free) {
      // The whole picture, as shown (the video is shown whole in this mode), at most 1600 on its long side.
      const k = Math.min(1, 1600 / Math.max(v.videoWidth, v.videoHeight));
      const fc = document.createElement('canvas'); fc.width = Math.round(v.videoWidth * k); fc.height = Math.round(v.videoHeight * k);
      fc.getContext('2d').drawImage(v, 0, 0, fc.width, fc.height);
      ui.o.querySelector('.cc-stage').classList.add('flash'); setTimeout(() => ui?.o.querySelector('.cc-stage').classList.remove('flash'), 180);
      fc.toBlob((b) => { if (!b) return; stop(); edit(new File([b], `photo-${Date.now()}.jpg`, { type: 'image/jpeg' }), true); }, 'image/jpeg', 0.9);
      return;
    }
    const f = frameRect();
    const scale = Math.max(f.sw / v.videoWidth, f.sh / v.videoHeight);
    const ox = (f.sw - v.videoWidth * scale) / 2, oy = (f.sh - v.videoHeight * scale) / 2;
    const c = document.createElement('canvas'); c.width = OUT_W; c.height = OUT_H;
    const g = c.getContext('2d');
    g.imageSmoothingQuality = 'high';
    g.drawImage(v, (f.x - ox) / scale, (f.y - oy) / scale, f.w / scale, f.h / scale, 0, 0, OUT_W, OUT_H);
    ui.o.querySelector('.cc-stage').classList.add('flash'); setTimeout(() => ui?.o.querySelector('.cc-stage').classList.remove('flash'), 180);
    c.toBlob((b) => { if (!b) return; stop(); edit(new File([b], 'card.jpg', { type: 'image/jpeg' }), true); }, 'image/jpeg', 0.92);
  }

  // A photo in the frame: starts centred and filling the frame; drag to move, pinch / wheel / slider to zoom.
  function edit(file, fromCamera) {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      ui.img = img; ui.fromCamera = !!fromCamera; ui.file = file;
      const p = ui.o.querySelector('.cc-pic'); p.src = url;
      if (ui.free) { p.removeAttribute('style'); mode('edit'); return; }
      mode('edit'); layout(); fit(true); paint();
      ui.o.querySelector('.cc-zoom').value = '1';
    };
    img.onerror = () => { ui.o.querySelector('.cc-hint').textContent = 'That photo couldn’t be opened. Try another.'; };
    img.src = url;
  }
  function fit(reset) {
    const f = frameRect(), img = ui.img;
    ui.base = Math.max(f.w / img.naturalWidth, f.h / img.naturalHeight);
    if (reset) { ui.s = 1; ui.x = 0; ui.y = 0; }
    clamp();
  }
  // The photo always covers the frame: no empty corners.
  function clamp() {
    const f = frameRect(), sc = ui.base * ui.s;
    const w = ui.img.naturalWidth * sc, h = ui.img.naturalHeight * sc;
    const mx = Math.max(0, (w - f.w) / 2), my = Math.max(0, (h - f.h) / 2);
    ui.x = Math.min(mx, Math.max(-mx, ui.x)); ui.y = Math.min(my, Math.max(-my, ui.y));
  }
  function paint() {
    const f = frameRect(), sc = ui.base * ui.s, p = ui.o.querySelector('.cc-pic');
    const w = ui.img.naturalWidth * sc, h = ui.img.naturalHeight * sc;
    Object.assign(p.style, { width: `${w}px`, height: `${h}px`, left: `${f.x + f.w / 2 - w / 2 + ui.x}px`, top: `${f.y + f.h / 2 - h / 2 + ui.y}px` });
  }
  function zoomTo(s) { if (!ui?.img || ui.free) return; ui.s = Math.min(4, Math.max(1, s)); clamp(); paint(); ui.o.querySelector('.cc-zoom').value = String(ui.s); }
  function gestures(stage) {
    const pts = new Map(); let last = null;
    stage.addEventListener('pointerdown', (e) => { if (!ui?.img || ui.free) return; stage.setPointerCapture(e.pointerId); pts.set(e.pointerId, { x: e.clientX, y: e.clientY }); last = null; });
    stage.addEventListener('pointermove', (e) => {
      if (!ui?.img || ui.free || !pts.has(e.pointerId)) return;
      const prev = pts.get(e.pointerId); pts.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pts.size === 1) { ui.x += e.clientX - prev.x; ui.y += e.clientY - prev.y; clamp(); paint(); return; }
      const [a, b] = [...pts.values()]; const d = Math.hypot(a.x - b.x, a.y - b.y);
      if (last) zoomTo(ui.s * (d / last));
      last = d;
    });
    const up = (e) => { pts.delete(e.pointerId); last = null; };
    stage.addEventListener('pointerup', up); stage.addEventListener('pointercancel', up);
    stage.addEventListener('wheel', (e) => { if (!ui?.img) return; e.preventDefault(); zoomTo(ui.s * (e.deltaY < 0 ? 1.08 : 1 / 1.08)); }, { passive: false });
  }

  // Cut what the frame shows into a card-shaped JPEG and hand it to the page.
  function give(target, file) {
    try {
      const dt = new DataTransfer(); dt.items.add(file); target.files = dt.files;
      target.dispatchEvent(new Event('input', { bubbles: true }));
      target.dispatchEvent(new Event('change', { bubbles: true }));
    } catch (_) {
      // A very old browser: let it use its own picker instead.
      target.dataset.camPass = '1'; target.click(); delete target.dataset.camPass;
    }
    close();
  }
  function use() {
    if (ui.free) { if (ui.file) give(ui.target, ui.file); return; }
    const f = frameRect(), sc = ui.base * ui.s;
    const w = ui.img.naturalWidth * sc, h = ui.img.naturalHeight * sc;
    const left = f.x + f.w / 2 - w / 2 + ui.x, top = f.y + f.h / 2 - h / 2 + ui.y;
    const c = document.createElement('canvas'); c.width = OUT_W; c.height = OUT_H;
    const g = c.getContext('2d'); g.imageSmoothingQuality = 'high';
    g.drawImage(ui.img, (f.x - left) / sc, (f.y - top) / sc, f.w / sc, f.h / sc, 0, 0, OUT_W, OUT_H);
    const target = ui.target;
    c.toBlob((b) => {
      if (!b) return;
      give(target, new File([b], `card-${Date.now()}.jpg`, { type: 'image/jpeg' }));
    }, 'image/jpeg', 0.92);
  }
})();
