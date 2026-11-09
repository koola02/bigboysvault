// Holographic cards. The card follows your finger or mouse, and your phone's movement.
//   Best:     WebGPU + HTML-in-Canvas. The real page content (picture, grade, name, price) is painted onto a lit 3D surface.
//   Good:     WebGPU with just the card picture. Same foil and light; the text stays on the flat page. (iPhone and iPad.)
//   Fallback: a CSS tilt with a moving shine, which works in every browser.
// All three are driven by the same motion code. People who ask for less motion get a card that moves only under their own finger.
//
// On touch screens: drag sideways to tilt, or press and hold for a moment to tilt in any direction. A quick up-or-down swipe scrolls the page.

const SHADER = /* wgsl */ `
struct U { face: vec4f, tilt: vec4f, art: vec4f, light: vec4f, tone: vec4f };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex: texture_2d<f32>;
struct V { @builtin(position) pos: vec4f, @location(0) uv: vec2f };

@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
  var c = array(vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(-1.0, 1.0), vec2f(-1.0, 1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0));
  let g = 1.22;                                   // room around the face for the glow
  let p = vec2f(c[i].x * u.face.x * g, c[i].y * u.face.y * g);
  let sx = sin(u.tilt.x); let cx = cos(u.tilt.x); let sy = sin(u.tilt.y); let cy = cos(u.tilt.y);
  // Positive tilt pushes the top edge (x) and the right edge (y) away from you, the way pressing a card would.
  let x = p.x * cy;
  let z0 = -p.x * sy;
  let y = p.y * cx + z0 * sx;
  let z = -p.y * sx + z0 * cx;
  let d = 1500.0;
  var o: V;
  o.pos = vec4f(x / u.face.z, y / u.face.w, 0.0, (d - z) / d);
  o.uv = vec2f(c[i].x * g * 0.5 + 0.5, 0.5 - c[i].y * g * 0.5);
  return o;
}

fn hash(p: vec2f) -> f32 { return fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453); }

@fragment fn fs(v: V) -> @location(0) vec4f {
  let size = u.face.xy * 2.0;
  let px = v.uv * size;
  let lo = u.art.xy * size;
  let hi = u.art.zw * size;
  let centre = (lo + hi) * 0.5;
  let hs = (hi - lo) * 0.5;
  let radius = min(hs.x, hs.y) * 0.055;
  let q = abs(px - centre) - hs + radius;
  let sdf = length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - radius;
  let inFace = all(v.uv >= vec2f(0.0)) && all(v.uv <= vec2f(1.0));
  var col = textureSampleLevel(tex, samp, clamp(v.uv, vec2f(0.0), vec2f(1.0)), 0.0);
  if (!inFace) { col = vec4f(0.0); }
  let inArt = 1.0 - smoothstep(-0.8, 0.8, sdf);
  // Only the picture gets rounded corners. Page content outside it (name and price) stays as it is.
  if (all(px >= lo - 1.0) && all(px <= hi + 1.0)) { col = col * inArt; }
  let auv = (px - lo) / max(hi - lo, vec2f(1.0));
  let t = u.tilt.z;
  let strength = u.tilt.w;
  let ang = u.tilt.y * 1.5 - u.tilt.x;
  let hue = fract(auv.x * 0.9 + auv.y * 0.6 + ang * 0.7);
  let rainbow = 0.5 + 0.5 * cos(6.2832 * (hue + vec3f(0.0, 0.33, 0.67)));
  let band = pow(0.5 + 0.5 * sin((auv.x + auv.y) * 8.0 + ang * 7.0), 3.0);
  let lum = dot(col.rgb, vec3f(0.3, 0.59, 0.11));
  // Tiny round glints that twinkle as the card moves.
  let cell = auv * vec2f(150.0, 210.0);
  let n = hash(floor(cell));
  let dot2 = 1.0 - smoothstep(0.05, 0.45, length(fract(cell) - 0.5));
  let spark = step(0.965, n) * dot2 * pow(0.5 + 0.5 * sin(t * 2.0 + n * 90.0 + ang * 16.0), 4.0);
  let glare = exp(-dot(auv - u.light.xy, auv - u.light.xy) * 5.0) * 0.3;
  // Foil tints what's already there, so bright card art keeps its detail, and adds colour only where the art is dark.
  let tint = mix(vec3f(1.0), rainbow * 1.12, band * 0.42 * strength);
  let lift = rainbow * band * 0.1 * (1.0 - lum) * strength + vec3f(spark * 0.7 * strength * (1.0 - lum * 0.6) + glare * 0.55);
  let foiled = min(col.rgb * tint, vec3f(col.a));
  let shine = vec4f(mix(col.rgb, foiled, inArt) + lift * col.a * inArt * (1.0 - lum * 0.5), col.a);
  // An edge and a soft glow in the card's condition colour (brass when it has none), moving with the card.
  let gold = u.tone.rgb;
  let rim = smoothstep(-3.5, -2.0, sdf) * inArt;
  let glow = exp(-max(sdf, 0.0) / 16.0) * 0.38 * smoothstep(-0.5, 0.5, sdf) * (1.0 - smoothstep(16.0, 34.0, sdf));
  let rgb = mix(shine.rgb, gold * (0.8 + band * 0.5), rim);
  let a = max(shine.a, rim);
  return vec4f(rgb + gold * glow * (1.0 - a), a + glow * (1.0 - a));
}`;

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const parts = (stage) => {
  const art = stage.querySelector(':scope > .sc-img') || stage.querySelector(':scope > img');
  return { art, img: art.matches('img') ? art : art.querySelector('img') };
};

// The picture's true box inside the stage, measured with the card flat. Its element can be wider or taller than the card itself.
function pictureBox(stage, art, img) {
  stage.classList.add('holo-flat');
  const s = stage.getBoundingClientRect();
  const r = art.getBoundingClientRect();
  stage.classList.remove('holo-flat');
  const nw = img.naturalWidth || r.width, nh = img.naturalHeight || r.height;
  const scale = Math.min(r.width / nw, r.height / nh);
  const w = nw * scale, h = nh * scale;
  return { left: r.left - s.left + (r.width - w) / 2, top: r.top - s.top + (r.height - h) / 2, width: w, height: h, stageW: s.width, stageH: s.height };
}

// ---------- What the card is doing: one source of truth for all three renderers ----------
function createMotion(stage, reduced, signal) {
  const on = (t, ev, fn, o = {}) => t.addEventListener(ev, fn, { ...o, signal });
  const m = { rx: 0, ry: 0, mx: 0.5, my: 0.35, power: 0.6, box: null, source: 'idle', onOrient: null };
  let press = null, hover = null, last = 0, dragged = false, grabbed = false;
  let motionRx = 0, motionRy = 0, lastMotion = -1e9;

  const aim = (e) => {
    const s = stage.getBoundingClientRect(), b = m.box;
    if (!b) return null;
    const x = clamp((e.clientX - s.left - b.left) / b.width, 0, 1), y = clamp((e.clientY - s.top - b.top) / b.height, 0, 1);
    return { x, y, ry: (x - 0.5) * 0.62, rx: (0.5 - y) * 0.52 };
  };
  const isHover = (e) => e.pointerType === 'mouse' || (e.pointerType === 'pen' && e.buttons === 0);

  on(stage, 'pointerdown', (e) => {
    if (e.pointerType === 'mouse') return;
    dragged = false; grabbed = false;
    press = { id: e.pointerId, sx: e.clientX, sy: e.clientY, aim: aim(e), hold: 0 };
    press.hold = setTimeout(() => { if (press) grabbed = true; }, 170);
  }, { passive: true });
  on(stage, 'pointermove', (e) => {
    if (press && e.pointerId === press.id) {
      press.aim = aim(e);
      if (Math.hypot(e.clientX - press.sx, e.clientY - press.sy) > 10) { dragged = true; clearTimeout(press.hold); }
    } else if (isHover(e)) hover = aim(e);
  }, { passive: true });
  const release = (e) => { if (press && e.pointerId === press.id) { clearTimeout(press.hold); press = null; grabbed = false; } };
  on(stage, 'pointerup', release, { passive: true });
  on(stage, 'pointercancel', release, { passive: true });
  on(stage, 'pointerleave', (e) => { if (isHover(e)) hover = null; }, { passive: true });
  // Once held, the finger belongs to the card and the page stays still.
  on(stage, 'touchmove', (e) => { if (grabbed && e.cancelable) e.preventDefault(); }, { passive: false });
  // Dragging a card must not open its link, start an image drag, or bring up the long-press menu.
  on(stage, 'click', (e) => { if (dragged || grabbed) { e.preventDefault(); e.stopPropagation(); } dragged = false; }, true);
  on(stage, 'dragstart', (e) => e.preventDefault());
  on(stage, 'contextmenu', (e) => { if (press) e.preventDefault(); });

  // Phone and tablet movement. Works from rotations rather than raw angles, so holding the device upright or sideways can't make it jump.
  let base = null, lastEvent = 0;
  const rad = Math.PI / 360;
  const quat = (a, b, g) => {
    const c1 = Math.cos(a * rad), s1 = Math.sin(a * rad), c2 = Math.cos(b * rad), s2 = Math.sin(b * rad), c3 = Math.cos(g * rad), s3 = Math.sin(g * rad);
    return [c1 * c2 * c3 - s1 * s2 * s3, c1 * s2 * c3 - s1 * c2 * s3, c1 * c2 * s3 + s1 * s2 * c3, s1 * c2 * c3 + c1 * s2 * s3]; // w x y z, order Z-X-Y
  };
  const screenAngle = () => { const a = screen.orientation?.angle; return typeof a === 'number' ? a : ((window.orientation || 0) + 360) % 360; };
  m.onOrient = (e) => {
    if (e.beta == null || e.gamma == null) return;
    const now = performance.now(), q = quat(e.alpha || 0, e.beta, e.gamma);
    if (!base) base = q;
    // Dot and sign: q and -q are the same rotation.
    const d = base[0] * q[0] + base[1] * q[1] + base[2] * q[2] + base[3] * q[3];
    if (d < 0) { q[0] = -q[0]; q[1] = -q[1]; q[2] = -q[2]; q[3] = -q[3]; }
    // The rest position slowly follows however the device is being held, so the card always settles back to centre.
    const k = Math.min(1, Math.max(0, (now - lastEvent) / 1000) / 5);
    lastEvent = now;
    for (let i = 0; i < 4; i++) base[i] += (q[i] - base[i]) * k;
    const len = Math.hypot(...base); for (let i = 0; i < 4; i++) base[i] /= len;
    // Rotation from the rest position, in the device's own axes.
    const [w, x, y, z] = base, [qw, qx, qy, qz] = q;
    const rw = w * qw + x * qx + y * qy + z * qz;
    const rx = w * qx - x * qw - y * qz + z * qy;
    const ry = w * qy + x * qz - y * qw - z * qx;
    const s = rw < 0 ? -2 : 2;
    const tx = rx * s, ty = ry * s;
    if (Math.hypot(tx, ty) > 1.2) { base = q; return; }
    // Turn device axes into screen axes, so tablets and sideways phones tilt the right way.
    let sx, sy;
    switch (screenAngle()) {
      case 90: sx = tx; sy = -ty; break;
      case 180: sx = -ty; sy = -tx; break;
      case 270: sx = -tx; sy = ty; break;
      default: sx = ty; sy = tx;
    }
    const gain = 1.5;
    motionRy = clamp(sx * gain, -0.36, 0.36);
    motionRx = clamp(-sy * gain, -0.32, 0.32);
    lastMotion = now;
  };
  on(window, 'orientationchange', () => { base = null; });

  m.step = (now) => {
    const dt = last ? Math.min(0.1, (now - last) / 1000) : 1 / 60;
    last = now;
    const s = stage.getBoundingClientRect();
    const scroll = reduced ? 0 : clamp((s.top + s.height / 2 - innerHeight / 2) / innerHeight, -1, 1) * 0.16;
    const t = now / 1000;
    let tx, ty, lx, ly, tau, pw;
    if (press?.aim) { ({ rx: tx, ry: ty, x: lx, y: ly } = press.aim); tau = 0.028; pw = 1; m.source = 'touch'; }
    else if (hover) { ({ rx: tx, ry: ty, x: lx, y: ly } = hover); tau = 0.05; pw = 1; m.source = 'mouse'; }
    else if (now - lastMotion < 500 && !reduced) { tx = motionRx + scroll; ty = motionRy; lx = 0.5 + ty * 1.5; ly = 0.4 - tx * 1.5; tau = 0.045; pw = 0.95; m.source = 'motion'; }
    else {
      tx = scroll + (reduced ? 0 : Math.cos(t * 0.5) * 0.035); ty = reduced ? 0 : Math.sin(t * 0.7) * 0.07;
      lx = 0.5 + ty * 1.2; ly = 0.38 - tx * 1.2; tau = 0.16; pw = 0.6; m.source = 'idle';
    }
    // Same response at 30, 60 or 120 frames a second.
    const k = 1 - Math.exp(-dt / tau), kl = 1 - Math.exp(-dt / Math.min(tau, 0.06));
    m.rx += (tx - m.rx) * k; m.ry += (ty - m.ry) * k;
    m.mx += (lx - m.mx) * kl; m.my += (ly - m.my) * kl;
    m.power += (pw - m.power) * (1 - Math.exp(-dt / 0.2));
    m.t = reduced ? 0 : t;
  };
  return m;
}

// iPhones and iPads only give motion to a page after a tap on something, and that tap can't be a link. So: one clear button, once.
function motionAccess(stage, m, reduced, signal) {
  if (reduced || typeof DeviceOrientationEvent === 'undefined') return;
  const listen = () => addEventListener('deviceorientation', m.onOrient, { passive: true, signal });
  const ask = DeviceOrientationEvent.requestPermission;
  if (typeof ask !== 'function') { listen(); return; }
  // Without a tap this only succeeds if they already said yes on this visit.
  ask.call(DeviceOrientationEvent).then((p) => { if (p === 'granted') listen(); }, () => {
    if (document.querySelector('.holo-motion') || !matchMedia('(pointer: coarse)').matches) return;
    const b = document.createElement('button');
    b.type = 'button'; b.className = 'holo-motion';
    b.innerHTML = '<span aria-hidden="true">📱</span> Tilt your device to move the card';
    b.addEventListener('click', () => {
      ask.call(DeviceOrientationEvent).then((p) => { if (p === 'granted') document.dispatchEvent(new Event('holo-motion-on')); }, () => {}).finally(() => b.remove());
    });
    if (signal.aborted) return;
    const info = stage.dataset.holo === 'detail' && stage.closest('.detail')?.querySelector('[data-translate]');
    if (info) info.prepend(b); else stage.after(b);
  });
  document.addEventListener('holo-motion-on', listen, { once: true, signal });
}

// ---------- Fallback: CSS ----------
function createCss(stage, art) {
  const sheen = document.createElement('span');
  sheen.className = 'holo-sheen';
  sheen.setAttribute('aria-hidden', 'true');
  stage.append(sheen);
  stage.classList.add('holo-tilt');
  return {
    place(b) {
      Object.assign(sheen.style, { left: `${b.left - stage.clientLeft}px`, top: `${b.top - stage.clientTop}px`, width: `${b.width}px`, height: `${b.height}px`, borderRadius: getComputedStyle(art).borderRadius });
    },
    draw(m) {
      const st = stage.style;
      st.setProperty('--rx', `${m.rx.toFixed(4)}rad`);
      st.setProperty('--ry', `${m.ry.toFixed(4)}rad`);
      st.setProperty('--mx', `${(m.mx * 100).toFixed(1)}%`);
      st.setProperty('--my', `${(m.my * 100).toFixed(1)}%`);
      st.setProperty('--foil', `${(m.ry * 180 + m.rx * 90 + 50).toFixed(1)}%`);
    },
  };
}

// ---------- Best and good: WebGPU ----------
// A copy of the page content inside the canvas, where the browser can paint it into a texture.
function buildFace(stage, canvas, src) {
  const face = document.createElement('div');
  face.className = 'holo-face';
  face.setAttribute('drawable', '');
  const content = stage.dataset.holo === 'card' ? [...stage.children].filter((n) => n.matches('.sc-img, .sc-t, .sc-m, .sc-p')) : [...stage.querySelectorAll(':scope > img')];
  stage.classList.add('holo-flat');
  const s = stage.getBoundingClientRect();
  face.style.width = `${s.width}px`;
  face.style.height = `${s.height}px`;
  for (const n of content) {
    const box = n.getBoundingClientRect();
    const c = n.cloneNode(true);
    c.querySelector?.('.sc-shine')?.remove();
    const pic = c.matches('img') ? c : c.querySelector('img');
    if (pic) { pic.removeAttribute('srcset'); pic.removeAttribute('sizes'); pic.loading = 'eager'; pic.crossOrigin = 'anonymous'; pic.src = src; }
    Object.assign(c.style, { position: 'absolute', margin: '0', left: `${box.left - s.left}px`, top: `${box.top - s.top}px`, width: `${box.width}px`, height: `${box.height}px` });
    face.append(c);
  }
  stage.classList.remove('holo-flat');
  canvas.append(face);
  return face;
}

// Ask the image service for just enough pixels: a card shrunk too far looks grainy while it moves.
function sizedSrc(src, px) {
  const want = Math.min(1400, Math.ceil(px / 100) * 100);
  return /[?&]width=\d+/.test(src) ? src.replace(/([?&]width=)\d+/, `$1${want}`) : src;
}

async function createGpu(stage, art, img, onLost) {
  const adapter = await navigator.gpu.requestAdapter({ powerPreference: 'low-power' });
  if (!adapter) return null;
  const device = await adapter.requestDevice();
  const canvas = document.createElement('canvas');
  canvas.className = 'holo-c';
  canvas.setAttribute('aria-hidden', 'true');
  const canDrawHtml = typeof GPUQueue !== 'undefined' && typeof GPUQueue.prototype.drawElementImageToTexture === 'function' && typeof canvas.requestPaint === 'function' && 'content' in canvas;
  let html = canDrawHtml;
  if (html) canvas.setAttribute('content', 'drawable');
  const context = canvas.getContext('webgpu');
  if (!context) { device.destroy(); return null; }

  const format = navigator.gpu.getPreferredCanvasFormat();
  context.configure({ device, format, alphaMode: 'premultiplied' });
  const module = device.createShaderModule({ code: SHADER });
  const pipeline = await device.createRenderPipelineAsync({
    layout: 'auto',
    vertex: { module, entryPoint: 'vs' },
    fragment: { module, entryPoint: 'fs', targets: [{ format, blend: { color: { srcFactor: 'one', dstFactor: 'one-minus-src-alpha' }, alpha: { srcFactor: 'one', dstFactor: 'one-minus-src-alpha' } } }] },
    primitive: { topology: 'triangle-list' },
  });
  const uniforms = device.createBuffer({ size: 80, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  const sampler = device.createSampler({ magFilter: 'linear', minFilter: 'linear' });
  const usage = GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST | GPUTextureUsage.RENDER_ATTACHMENT;

  let texture = null, binding = null, faceEl = null, stopped = false, shown = false, ready = false;
  let geom = { w: 1, h: 1, cw: 1, ch: 1, art: [0, 0, 1, 1] }, sized = '';
  const useTexture = (t) => {
    texture?.destroy(); texture = t;
    binding = device.createBindGroup({ layout: pipeline.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer: uniforms } }, { binding: 1, resource: sampler }, { binding: 2, resource: t.createView() }] });
  };

  const api = {
    get ready() { return ready; },
    tier: () => (html ? 'html-in-canvas' : 'webgpu'),
    place, draw,
    destroy() {
      if (stopped) return;
      stopped = true;
      stage.classList.remove('holo-gpu', 'holo-html');
      delete stage.dataset.holoTier;
      canvas.remove();
      device.destroy();
    },
  };
  device.lost.then(() => { if (!stopped) { api.destroy(); onLost(); } });
  device.addEventListener('uncapturederror', (e) => { console.debug('Holo card stopped', e.error?.message); if (!stopped) { api.destroy(); onLost(); } });

  const scale = (w, h) => Math.min(devicePixelRatio || 1, 2.5, 2400 / Math.max(w, h));

  function place(b) {
    // Only change the canvas when its size really changes: resizing clears it, which would flicker on every phone toolbar movement.
    const box = html ? { left: 0, top: 0, width: b.stageW, height: b.stageH } : b;
    const w = box.width, h = box.height, padX = w * 0.16, padY = h * 0.16;
    Object.assign(canvas.style, { left: `${box.left - padX - stage.clientLeft}px`, top: `${box.top - padY - stage.clientTop}px`, width: `${w + padX * 2}px`, height: `${h + padY * 2}px` });
    const f = scale(w + padX * 2, h + padY * 2);
    const pw = Math.round((w + padX * 2) * f), ph = Math.round((h + padY * 2) * f);
    const changed = canvas.width !== pw || canvas.height !== ph;
    if (changed) { canvas.width = pw; canvas.height = ph; }
    geom = { w, h, cw: w + padX * 2, ch: h + padY * 2, art: [(b.left - box.left) / w, (b.top - box.top) / h, (b.left + b.width - box.left) / w, (b.top + b.height - box.top) / h] };
    sized = sizedSrc(stage.dataset.holoSrc, b.width * f);
    if (html && faceEl && changed) { const fresh = buildFace(stage, canvas, sized); faceEl.remove(); faceEl = fresh; canvas.requestPaint(); }
  }

  place(pictureBox(stage, art, img));
  stage.append(canvas);

  if (html) {
    faceEl = buildFace(stage, canvas, sized);
    const loaded = await Promise.all([...faceEl.querySelectorAll('img')].map((p) => p.decode().then(() => true, () => false)));
    if (loaded.includes(false)) {
      faceEl.remove(); faceEl = null; canvas.removeAttribute('content'); html = false;
      place(pictureBox(stage, art, img));
    }
  }
  if (html) {
    canvas.addEventListener('paint', () => {
      if (stopped || !faceEl) return;
      const f = scale(geom.cw, geom.ch), width = Math.max(1, Math.round(geom.w * f)), height = Math.max(1, Math.round(geom.h * f));
      if (!texture || texture.width !== width || texture.height !== height) useTexture(device.createTexture({ size: { width, height }, format: 'rgba8unorm', usage }));
      try {
        device.queue.drawElementImageToTexture({ source: faceEl }, { texture, size: { width, height }, premultipliedAlpha: true });
        ready = true;
      } catch (_) {
        // Nothing painted to copy yet, for instance while the window is still growing into place. Next frame.
        requestAnimationFrame(() => { if (!stopped) canvas.requestPaint(); });
      }
    });
    canvas.requestPaint();
  } else {
    const res = await fetch(sized, { mode: 'cors' });
    if (!res.ok) throw new Error('picture ' + res.status);
    const bmp = await createImageBitmap(await res.blob());
    const t = device.createTexture({ size: { width: bmp.width, height: bmp.height }, format: 'rgba8unorm', usage });
    device.queue.copyExternalImageToTexture({ source: bmp }, { texture: t, premultipliedAlpha: true }, { width: bmp.width, height: bmp.height });
    bmp.close();
    useTexture(t);
    ready = true;
  }

  const data = new Float32Array(20);
  // The condition colour the tiles use (--tone), so the 3D card glows the same colour as its tile.
  const tone = (() => { const v = getComputedStyle(stage).getPropertyValue('--tone').split(',').map((x) => Number(x.trim()) / 255); return v.length === 3 && v.every((x) => x >= 0 && x <= 1) ? v : [0.886, 0.725, 0.294]; })();
  function draw(m) {
    if (stopped || !ready || !binding) return;
    data.set([geom.w / 2, geom.h / 2, geom.cw / 2, geom.ch / 2, m.rx, m.ry, m.t, m.power, ...geom.art, m.mx, m.my, 0, 0, ...tone, 1]);
    device.queue.writeBuffer(uniforms, 0, data);
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({ colorAttachments: [{ view: context.getCurrentTexture().createView(), clearValue: [0, 0, 0, 0], loadOp: 'clear', storeOp: 'store' }] });
    pass.setPipeline(pipeline); pass.setBindGroup(0, binding); pass.draw(6); pass.end();
    device.queue.submit([encoder.finish()]);
    if (!shown) {
      shown = true;
      stage.classList.add('holo-gpu');
      if (html) stage.classList.add('holo-html');
      stage.dataset.holoTier = api.tier();
    }
  }
  return api;
}

// ---------- One card ----------
async function run(stage, signal) {
  const { art, img } = parts(stage);
  if (!img.complete) await img.decode().catch(() => {});
  if (signal.aborted) return;
  const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const m = createMotion(stage, reduced, signal);
  stage.holo = m;
  motionAccess(stage, m, reduced, signal);
  const css = createCss(stage, art);
  let gpu = null, tries = 0, visible = false, running = false;
  const inFloat = !!stage.closest('dialog');

  const place = () => {
    m.box = pictureBox(stage, art, img);
    css.place(m.box);
    gpu?.place(m.box);
  };
  place();
  const resized = new ResizeObserver(place);
  resized.observe(stage);
  if (!img.complete) img.addEventListener('load', place, { once: true, signal });

  function frame(now) {
    if (signal.aborted || !visible || document.hidden) { running = false; return; }
    requestAnimationFrame(frame);
    if (!inFloat && document.documentElement.dataset.float) return; // a floating card is on top: this one rests
    m.step(now);
    try {
      if (gpu?.ready) gpu.draw(m); else css.draw(m);
    } catch (e) {
      console.debug('Holo card using the simple tilt', e);
      gpu?.destroy(); gpu = null; retry();
    }
  }
  const wake = () => { if (visible && !document.hidden && !running && !signal.aborted) { running = true; requestAnimationFrame(frame); } };
  const seen = new IntersectionObserver((es) => { visible = es.some((e) => e.isIntersecting); wake(); }, { rootMargin: '10%' });
  seen.observe(stage);
  document.addEventListener('visibilitychange', wake, { signal });
  addEventListener('pageshow', wake, { signal });

  // Phones drop the graphics connection when an app goes to the background. Reconnect when the page comes back.
  function retry() {
    if (tries >= 4 || signal.aborted) return;
    const go = () => setTimeout(startGpu, 300);
    if (document.hidden) document.addEventListener('visibilitychange', go, { once: true, signal }); else go();
  }
  async function startGpu() {
    if (!navigator.gpu || gpu || signal.aborted) return;
    tries++;
    try {
      const g = await createGpu(stage, art, img, () => { gpu = null; retry(); });
      if (signal.aborted) g?.destroy(); else gpu = g;
    } catch (e) {
      console.debug('Holo card using the simple tilt', e);
      stage.querySelector(':scope > .holo-c')?.remove();
    }
  }
  signal.addEventListener('abort', () => { resized.disconnect(); seen.disconnect(); gpu?.destroy(); gpu = null; });
  startGpu();
}

/** Start every card inside `root`. Returns a function that stops them and releases the graphics connection. */
export function mountHolo(root = document) {
  const ctl = new AbortController();
  if (document.documentElement.dataset.lite) return () => ctl.abort();
  for (const stage of root.querySelectorAll('[data-holo]')) {
    if (stage.dataset.holoOn) continue;
    stage.dataset.holoOn = '1';
    run(stage, ctl.signal);
  }
  return () => ctl.abort();
}

const start = () => { mountHolo(document.querySelector('main') || document); };
if (document.prerendering) document.addEventListener('prerenderingchange', start, { once: true });
else start();
