// Two small pieces of live graphics, for every page:
// the sky, slow ribbons of light from domain-warped noise, drawn small and scaled up so it costs almost nothing;
// and foil, the colours of real thin-film interference, on whichever card you're pointing at, or the one in the middle of the screen on a phone.
const root = document.documentElement;
const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
const tools = !!document.querySelector('.admin, #edList') || location.pathname.startsWith('/admin');
const saving = !!navigator.connection?.saveData;

const FULL = `#version 300 es
void main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);gl_Position=vec4(p*2.-1.,0.,1.);}`;
const NOISE = `
precision highp float;
float hash(vec2 p){p=fract(p*vec2(123.34,456.21));p+=dot(p,p+45.32);return fract(p.x*p.y);}
float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.-2.*f);return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x),f.y);}
float fbm(vec2 p){float v=0.,a=.5;for(int i=0;i<4;i++){v+=a*noise(p);p=p*2.03+vec2(17.1,9.2);a*=.5;}return v;}
`;
function program(gl, fs) {
  const p = gl.createProgram();
  for (const [type, src] of [[gl.VERTEX_SHADER, FULL], [gl.FRAGMENT_SHADER, fs]]) {
    const s = gl.createShader(type);
    gl.shaderSource(s, src.trim());
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
    gl.attachShader(p, s);
  }
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
  gl.useProgram(p);
  gl.bindVertexArray(gl.createVertexArray());
  return (n) => gl.getUniformLocation(p, n);
}

/* ---------- the sky ---------- */
function sky() {
  const c = document.createElement('canvas');
  c.className = 'hx-sky';
  c.setAttribute('aria-hidden', 'true');
  const gl = c.getContext('webgl2', { alpha: false, antialias: false, powerPreference: 'low-power' });
  if (!gl) return;
  let u;
  try {
    u = program(gl, `#version 300 es
    ${NOISE}
    uniform vec2 uRes;uniform float uT,uS;
    out vec4 o;
    void main(){
      vec2 p=gl_FragCoord.xy/uRes.y;
      p.y+=uS*.55;
      float t=uT;
      vec2 q=vec2(fbm(p*1.25+vec2(0.,t*.018)),fbm(p*1.25+vec2(5.2,1.3)-t*.014));
      vec2 r=vec2(fbm(p*1.1+q*2.4+vec2(1.7,9.2)+t*.011),fbm(p*1.1+q*2.4+vec2(8.3,2.8)-t*.009));
      float n=fbm(p*1.05+r*2.6);
      vec3 c=mix(vec3(.012,.018,.07),vec3(.09,.024,.13),smoothstep(.2,.8,q.x));
      c=mix(c,vec3(.01,.1,.12),smoothstep(.35,.8,r.y)*.75);
      c*=n*n*2.4;
      c+=vec3(.5,.34,.13)*pow(smoothstep(.42,.86,n),3.)*.34;
      float band=pow(max(0.,1.-abs(r.x-.52)*7.),4.);
      c+=(.5+.5*cos(6.283*(n*1.6+vec3(0.,.33,.67))))*band*.1;
      c+=(hash(gl_FragCoord.xy)-.5)/255.;
      o=vec4(c,1.);
    }`);
  } catch (e) { console.warn('Sky unavailable', e); return; }
  document.body.prepend(c);
  const size = () => {
    const w = Math.max(2, Math.round(innerWidth / 3.2)), h = Math.max(2, Math.round(innerHeight / 3.2));
    if (c.width !== w || c.height !== h) { c.width = w; c.height = h; }
    gl.viewport(0, 0, w, h);
  };
  size();
  addEventListener('resize', () => { size(); draw(performance.now()); }, { passive: true });
  const t0 = performance.now();
  let last = 0, slow = 0, frames = 0, prev = t0;
  function draw(now) {
    gl.uniform2f(u('uRes'), c.width, c.height);
    gl.uniform1f(u('uT'), still ? 30 : (now - t0) / 1000 + 40);
    gl.uniform1f(u('uS'), scrollY / Math.max(1, document.documentElement.scrollHeight));
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }
  draw(t0);
  c.classList.add('on');
  if (still) return;
  const hero = document.getElementById('hxHero');
  let stopped = false;
  function tick(now) {
    if (stopped) return;
    requestAnimationFrame(tick);
    if (document.hidden || root.dataset.float || now - last < 46) return;
    if (hero && scrollY < hero.offsetHeight * 0.62) return; // the opening scene is covering the sky
    last = now;
    draw(now);
    // A device that can't spare it keeps the still picture rather than stutter.
    const gap = now - prev; prev = now;
    if (gap < 400) { slow += gap > 90 ? 1 : 0; if (++frames >= 40) { if (slow > 24) stopped = true; frames = 0; slow = 0; } }
  }
  requestAnimationFrame(tick);
}

/* ---------- foil ---------- */
function foil() {
  const c = document.createElement('canvas');
  c.className = 'hx-foil';
  c.setAttribute('aria-hidden', 'true');
  c.width = 240; c.height = 336;
  const gl = c.getContext('webgl2', { alpha: true, premultipliedAlpha: true, antialias: false });
  if (!gl) return;
  let u;
  try {
    u = program(gl, `#version 300 es
    ${NOISE}
    uniform vec2 uRes,uTilt;uniform float uT,uKind;
    out vec4 o;
    // The colour a thin film reflects at one wavelength, for thickness d in nanometres.
    float film(float d,float cosT,float lambda){return .5+.5*cos(25.1327*1.42*d*cosT/lambda);}
    void main(){
      vec2 uv=gl_FragCoord.xy/uRes;
      vec2 v=uv-.5+uTilt*.6;
      float s2=min(.9,dot(v,v)*1.6);
      float cosT=sqrt(1.-s2/(1.42*1.42));
      float d=260.+560.*fbm(uv*vec2(2.6,3.6)+uTilt*.9+uT*.015);
      vec3 c=vec3(film(d,cosT,640.),film(d,cosT,532.),film(d,cosT,450.));
      float along=dot(uv-.5,normalize(vec2(.8,.6)));
      float band=pow(max(0.,1.-abs(along-(uTilt.x+uTilt.y)*.9-sin(uT*.35)*.08)*2.6),3.);
      vec2 g=uv*vec2(38.,53.),id=floor(g);
      float h=hash(id);
      float tw=pow(max(0.,sin(h*90.+(uTilt.x*9.+uTilt.y*7.)*(1.+h*3.)+uT*.9)),24.);
      float sp=step(.9,h)*tw*smoothstep(.5,.1,length(fract(g)-.5));
      float a=(.22+.78*band)*uKind;
      vec3 col=c*a+vec3(1.,.97,.88)*sp*(.35+uKind*.5);
      o=vec4(col,clamp(a+sp,0.,1.));
    }`);
  } catch (e) { console.warn('Foil unavailable', e); return; }
  gl.viewport(0, 0, c.width, c.height);
  gl.uniform2f(u('uRes'), c.width, c.height);

  const KIND = { plain: 0.34, holo: 0.78, reverse: 0.68, ex: 1, first: 0.72 };
  const SEL = '.card-img, .sc-img, .hl-card, .tp-hl-card, .iv-spot';
  const fine = matchMedia('(hover: hover) and (pointer: fine)').matches;
  let on = null, raf = 0, tx = 0, ty = 0, sx = 0, sy = 0;

  const seat = (box) => {
    if (on === box) return;
    on = box;
    if (!box) { c.remove(); return; }
    const pic = box.querySelector('img');
    if (!pic) { on = null; c.remove(); return; }
    box.append(c);
    const place = () => { c.style.left = `${pic.offsetLeft}px`; c.style.top = `${pic.offsetTop}px`; c.style.width = `${pic.offsetWidth}px`; c.style.height = `${pic.offsetHeight}px`; };
    place();
    const holder = box.closest('[data-foil]');
    gl.uniform1f(u('uKind'), KIND[holder?.dataset.foil] ?? 0.62);
    if (!raf) raf = requestAnimationFrame(tick);
  };
  const t0 = performance.now();
  function tick(now) {
    raf = 0;
    if (!on || !on.isConnected) { on = null; c.remove(); return; }
    sx += (tx - sx) * 0.14; sy += (ty - sy) * 0.14;
    gl.uniform2f(u('uTilt'), sx, sy);
    gl.uniform1f(u('uT'), still ? 8 : (now - t0) / 1000);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    if (!document.hidden) raf = requestAnimationFrame(tick);
  }
  if (fine) {
    addEventListener('pointermove', (e) => {
      const box = e.target.closest?.(SEL);
      if (box) {
        const r = box.getBoundingClientRect();
        tx = (e.clientX - r.left) / r.width - 0.5;
        ty = (e.clientY - r.top) / r.height - 0.5;
      }
      seat(box || null);
    }, { passive: true });
    return;
  }
  // On a phone the card nearest the middle of the screen catches the light as you scroll past it, and as you tilt the phone.
  let wait = 0;
  const pick = () => {
    wait = 0;
    const scope = document.querySelector('dialog.float[open]') || document;
    const mid = innerHeight / 2;
    let best = null, near = 1e9;
    for (const n of scope.querySelectorAll(SEL)) {
      const r = n.getBoundingClientRect();
      if (r.bottom < 60 || r.top > innerHeight - 60 || !r.width) continue;
      const d = Math.abs(r.top + r.height / 2 - mid);
      if (d < near) { near = d; best = n; }
      if (r.top > innerHeight) break;
    }
    if (best) {
      const r = best.getBoundingClientRect();
      ty = (r.top + r.height / 2 - mid) / innerHeight;
      if (best === on) { /* keeps moving with the scroll */ } else tx = 0;
    }
    seat(best);
  };
  const soon = () => { if (!wait) wait = setTimeout(pick, 60); };
  addEventListener('scroll', soon, { passive: true, capture: true });
  addEventListener('deviceorientation', (e) => { if (e.gamma != null) { tx = Math.max(-0.5, Math.min(0.5, e.gamma / 50)); soon(); } }, { passive: true });
  document.addEventListener('float:filled', soon);
  setTimeout(pick, 1500);
}

if (!tools && !saving && !root.dataset.lite && 'WebGL2RenderingContext' in window) {
  const begin = () => { sky(); foil(); };
  if ('requestIdleCallback' in window) requestIdleCallback(begin, { timeout: 1800 }); else setTimeout(begin, 500);
}
