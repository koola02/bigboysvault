// The home page's opening scene, drawn live on the graphics card with plain WebGL2 and no libraries:
// the logo is a coin floating in space, a disc of gold light turns around it, and the vault's real cards orbit it in 3D.
// Drag to spin the cards, tap a card to open it, tap the logo to flip it. Without WebGL2 the page shows the logo and the cards as links.
const hero = document.getElementById('hxHero');
const canvas = hero?.querySelector('.hx-canvas');
const links = hero ? [...hero.querySelectorAll('.hx-links a')] : [];
const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
const lite = !!document.documentElement.dataset.lite;
const opts = { alpha: false, antialias: !lite, powerPreference: 'high-performance', desynchronized: false };
const gl = !canvas || navigator.connection?.saveData ? null : canvas.getContext('webgl2', opts);

if (hero && !gl) {
  hero.classList.add('no-gl');
  links.forEach((a) => { const i = a.querySelector('img'); if (i) i.src = a.dataset.img; });
} else if (gl) {
  run();
}

function run() {
  const COMMON = `
  precision highp float;
  #define TAU 6.28318530718
  float hash(vec2 p){p=fract(p*vec2(123.34,456.21));p+=dot(p,p+45.32);return fract(p.x*p.y);}
  float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.-2.*f);return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x),f.y);}
  float fbm(vec2 p){float v=0.,a=.5;for(int i=0;i<4;i++){v+=a*noise(p);p=p*2.03+vec2(17.1,9.2);a*=.5;}return v;}
  vec3 prism(float t){return .5+.5*cos(TAU*(t+vec3(.0,.33,.67)));}
  const vec3 GOLD=vec3(1.,.74,.32);
  `;
  const FULL = `#version 300 es
  void main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);gl_Position=vec4(p*2.-1.,0.,1.);}`;

  // Deep space, bent around the logo, with the logo's gold light pouring out and splitting into the colours of the card types.
  const SPACE = `#version 300 es
  ${COMMON}
  uniform vec2 uC;uniform float uR,uT,uRv,uShock,uAmp;
  out vec4 o;
  float stars(vec2 q,float s,float seed){
    vec2 g=q*s,i=floor(g),f=fract(g)-.5;
    float h=hash(i+seed);
    if(h<.86)return 0.;
    vec2 d=f-(vec2(hash(i+1.3+seed),hash(i+2.7+seed))-.5)*.7;
    float l=length(d);
    float tw=.55+.45*sin(uT*(.6+h*2.2)+h*40.);
    float cross=max(0.,1.-abs(d.x*d.y)*260.)*max(0.,1.-l*5.);
    return (smoothstep(.055,.0,l)+cross*.35)*tw*(h-.86)*7.;
  }
  void main(){
    vec2 p=(gl_FragCoord.xy-uC)/uR;
    float r=length(p),a=atan(p.y,p.x);
    vec2 q=p*(1.+.62/(r*r+.32));
    float w=fbm(q*.34-uT*.009);
    float n=fbm(q*.62+vec2(uT*.011,-uT*.007)+w*1.5);
    vec3 c=vec3(.004,.005,.013);
    float fall=exp(-r*.2);
    c+=mix(vec3(.012,.022,.075),vec3(.075,.02,.095),smoothstep(.3,.7,w))*n*n*2.2*fall;
    c+=prism(n*1.6+a*.16+uT*.018)*pow(n,3.4)*.55*fall;
    float out1=max(r-1.,0.);
    float halo=.5*exp(-out1*3.)+.06*exp(-out1*.62);
    c+=GOLD*halo*(1.+uAmp*.8);
    float k1=fbm(vec2(a*2.6,uT*.05))*5.;
    float rays=.42*pow(.5+.5*sin(a*7.+k1),18.)+.75*pow(.5+.5*sin(a*19.-k1*1.7+uT*.06),34.)+.9*pow(.5+.5*sin(a*47.+k1*2.3-uT*.09),64.);
    rays*=smoothstep(.8,1.12,r)*exp(-out1*.62);
    vec3 rayC=mix(GOLD,prism(a/TAU*2.+.05+uT*.012)*1.2,smoothstep(.5,2.6,out1));
    c+=rayC*rays*(.42+uAmp*.6);
    c+=mix(vec3(1.,.93,.75),prism(hash(floor(q*30.))),.2)*stars(q+uT*.0015,15.,0.);
    c+=vec3(.82,.88,1.)*stars(q*1.1-uT*.0025,34.,5.)*.7;
    // Sparks drifting outward from the logo.
    vec2 u=vec2(a/TAU*26.,log(r+.001)*7.-uT*.24);
    vec2 ci=floor(u),cf=fract(u)-.5;
    ci.x=mod(ci.x,26.);
    float ch=hash(ci+11.);
    if(ch>.82){
      vec2 d=cf-(vec2(hash(ci+3.),hash(ci+7.))-.5)*.6;
      float sp=smoothstep(.13,.0,length(d))*smoothstep(1.,1.25,r)*exp(-out1*.75);
      c+=mix(GOLD,prism(ch*7.),.45)*sp*1.6;
    }
    // Lightning from the logo's own bolts: a few arcs that strike and fade.
    for(int i=0;i<3;i++){
      float fi=float(i),tk=uT*(.34+fi*.11)+fi*3.7,id=floor(tk),ph=fract(tk);
      float flash=smoothstep(0.,.03,ph)*smoothstep(.2,.05,ph);
      if(flash<=.001||r<.98)continue;
      float a0=hash(vec2(id,fi))*TAU-3.14159,len=1.55+hash(vec2(fi,id))*1.3;
      float da=mod(a-a0+3.14159,TAU)-3.14159;
      float wob=(fbm(vec2(r*5.2+id*3.,id))-.5)*.62*out1;
      float dist=abs(da-wob)*r;
      float arc=(.0028/(dist+.0028)+.018/(dist+.05)*.45)*smoothstep(len,1.,r);
      c+=mix(vec3(1.,.9,.6),vec3(.55,.8,1.),out1*.5)*arc*flash*1.2;
    }
    if(uShock>=0.&&uShock<1.5){
      float R=1.+uShock*4.2,d=abs(r-R);
      c+=prism(a/TAU+r*.32)*(.018/(d+.012))*pow(1.-uShock/1.5,2.)*smoothstep(.98,1.1,r);
    }
    c*=smoothstep(0.,.55,uRv);
    c=c/(1.+c*.62);
    c=pow(c,vec3(.92));
    c+=(hash(gl_FragCoord.xy+fract(uT))-.5)/200.;
    o=vec4(c,1.);
  }`;

  // The disc of gold light and the thin orbit rings, one half behind the logo and one half in front.
  const RINGS = `#version 300 es
  ${COMMON}
  uniform vec2 uC;uniform float uR,uT,uD,uSide,uRv,uAmp;uniform vec3 uN0,uN1,uN2;
  out vec4 o;
  // The eleven card types set as lights around the ring.
  const vec3 TYPES[11]=vec3[11](vec3(1.,.27,.16),vec3(.2,.58,1.),vec3(.26,.84,.32),vec3(1.,.86,.15),vec3(.74,.33,.98),vec3(.86,.45,.22),vec3(.28,.22,.42),vec3(.72,.77,.86),vec3(.95,.74,.2),vec3(1.,.42,.74),vec3(.95,.93,.88));
  vec3 hit(vec3 ro,vec3 rd,vec3 n){return ro+rd*(-dot(ro,n)/dot(rd,n));}
  void main(){
    vec2 p=(gl_FragCoord.xy-uC)/uR;
    vec3 ro=vec3(0.,0.,uD),rd=normalize(vec3(p,0.)-ro);
    vec3 c=vec3(0.);
    vec3 h=hit(ro,rd,uN0);
    if(h.z*uSide>=0.){
      vec3 ex=normalize(cross(vec3(0.,1.,0.),uN0)),ey=cross(uN0,ex);
      vec2 lp=vec2(dot(h,ex),dot(h,ey));
      float rho=length(lp),phi=atan(lp.y,lp.x);
      float spin=uT*(1.9/pow(max(rho,.4),1.5));
      float st=fbm(vec2(phi*5.+spin,rho*15.));
      float st2=fbm(vec2(phi*14.-spin*1.6,rho*38.));
      float d=smoothstep(1.16,1.34,rho)*smoothstep(2.5,1.42,rho);
      float dens=d*(.2+1.25*st*st+.5*st2*st2);
      float dop=1.+.72*sin(phi+.6);
      vec3 film=mix(GOLD,prism(rho*1.3+st*.8+phi/TAU+uT*.02)*1.2,.34);
      c+=film*dens*dop*(.62+uAmp*.6);
      c+=vec3(1.,.9,.68)*smoothstep(.022,.0,abs(rho-1.17))*.7;
    }
    for(int i=0;i<2;i++){
      vec3 n=i==0?uN1:uN2;
      vec3 g=hit(ro,rd,n);
      if(g.z*uSide<0.)continue;
      vec3 ex=normalize(cross(vec3(.2,1.,.1),n)),ey=cross(n,ex);
      vec2 lp=vec2(dot(g,ex),dot(g,ey));
      float rho=length(lp),phi=atan(lp.y,lp.x),R=i==0?2.82:3.52;
      float line=smoothstep(.016,.0,abs(rho-R));
      float pa=fract(phi/TAU+uT*(i==0?.018:-.012));
      float tick=smoothstep(.07,.0,abs(rho-R))*step(.5,fract(pa*132.))*.32;
      c+=mix(GOLD,vec3(.72,.82,1.),float(i)*.7)*(line*.34+tick*.5);
      float seg=pa*11.,si=floor(seg),sd=(fract(seg)-.5)/11.*TAU*rho;
      float dq=length(vec2(sd,rho-R));
      vec3 tc=TYPES[int(mod(si,11.))];
      c+=tc*(smoothstep(.052,.015,dq)*1.5+.006/(dq*dq+.006)*.2);
    }
    c*=smoothstep(.15,.85,uRv);
    o=vec4(c/(1.+c*.5),0.);
  }`;

  // A flat thing in the scene: the corners are worked out in JavaScript, which also tells the pointer what it's touching.
  const QUAD = `#version 300 es
  uniform vec3 uQ[4];uniform vec2 uC,uRes;uniform float uR,uD;
  out vec2 v;
  void main(){
    int i=gl_VertexID;
    vec3 w=uQ[i];
    v=vec2(float(i&1),float((i>>1)&1));
    float k=(uD-w.z)/uD;
    vec2 px=uC+w.xy/k*uR;
    vec2 ndc=px/uRes*2.-1.;
    gl_Position=vec4(ndc*k,0.,k);
  }`;
  const CARD = `#version 300 es
  ${COMMON}
  in vec2 v;uniform sampler2D uTex;uniform float uPh,uLit,uHot;uniform vec3 uTone;uniform vec2 uCover;
  out vec4 o;
  void main(){
    vec2 e=abs(v-.5)-vec2(.5-.052,.5-.037);
    float sd=length(max(e,0.))+min(max(e.x,e.y),0.)-.037;
    float al=smoothstep(.004,-.002,sd);
    vec3 t=texture(uTex,(vec2(v.x,1.-v.y)-.5)*uCover+.5).rgb;
    float band=.5+.5*sin((v.x*1.2+v.y)*8.+uPh);
    vec3 foil=prism(v.x*1.3+v.y*.9+uPh*.16)*pow(band,3.);
    float sweep=smoothstep(.09,.0,abs(fract((v.x+v.y*.5)*.5+uPh*.05)-.5));
    vec3 c=t+foil*(.1+.2*uHot)*(.35+dot(t,vec3(.33)))+vec3(1.,.96,.84)*sweep*(.12+.2*uHot);
    float rim=smoothstep(-.022,-.002,sd);
    c=mix(c,uTone*1.15+.15,rim*.92);
    c*=uLit;
    o=vec4(c*al,al);
  }`;
  const COIN = `#version 300 es
  ${COMMON}
  in vec2 v;uniform sampler2D uTex;uniform float uT,uRv,uEdge,uFoil,uFlipX;
  out vec4 o;
  void main(){
    vec2 uv=vec2(mix(v.x,1.-v.x,uFlipX),1.-v.y);
    float ab=.0016+.002*uFoil;
    vec4 t=texture(uTex,uv);
    float ra=texture(uTex,uv+vec2(ab,0.)).r,ba=texture(uTex,uv-vec2(ab,0.)).b;
    vec3 c=vec3(ra,t.g,ba);
    float warm=smoothstep(.1,.5,c.r-c.b*.8)*smoothstep(.25,.75,c.r);
    float sweepP=fract(uT*.075)*2.6-.8;
    float sw=smoothstep(.17,.0,abs(v.x*.75+v.y*.45-sweepP));
    vec3 foil=prism(v.x*1.4-v.y+uT*.05+uFoil);
    c+=warm*foil*(.14+.34*uFoil+sw*.35);
    c+=warm*vec3(1.,.93,.75)*sw*.72;
    float gr=noise(v*30.)*.5+noise(v*73.)*.5;
    float keep=smoothstep(gr-.07,gr+.07,uRv*1.2-.1);
    float ember=smoothstep(.0,.08,keep)*(1.-smoothstep(.1,.5,keep));
    c=c*keep+GOLD*ember*2.5;
    c=mix(c,c*vec3(.34,.26,.15),uEdge);
    float al=t.a*max(keep,ember);
    o=vec4(c*al,al);
  }`;

  const prog = (fs, vs = FULL) => {
    const p = gl.createProgram();
    for (const [type, src] of [[gl.VERTEX_SHADER, vs], [gl.FRAGMENT_SHADER, fs]]) {
      const s = gl.createShader(type);
      gl.shaderSource(s, src.trim());
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
      gl.attachShader(p, s);
    }
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
    const cache = {};
    return { p, u: (n) => (cache[n] ??= gl.getUniformLocation(p, n)) };
  };
  let space, rings, card, coin;
  try { space = prog(SPACE); rings = prog(RINGS); card = prog(CARD, QUAD); coin = prog(COIN, QUAD); } catch (e) {
    console.warn('Vault scene unavailable', e);
    hero.classList.add('no-gl');
    links.forEach((a) => { const i = a.querySelector('img'); if (i) i.src = a.dataset.img; });
    return;
  }
  gl.bindVertexArray(gl.createVertexArray());
  gl.disable(gl.DEPTH_TEST);
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
  const aniso = gl.getExtension('EXT_texture_filter_anisotropic');

  const texture = (img) => {
    const t = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, t);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);
    gl.generateMipmap(gl.TEXTURE_2D);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    if (aniso) gl.texParameterf(gl.TEXTURE_2D, aniso.TEXTURE_MAX_ANISOTROPY_EXT, 8);
    return t;
  };
  const load = (url, cors) => new Promise((ok, no) => { const i = new Image(); if (cors) i.crossOrigin = 'anonymous'; i.onload = () => ok(i); i.onerror = no; i.src = url; });

  const TONES = { nm: [.06, .73, .51], lp: [.96, .62, .04], mp: [1, .5, .31], hp: [.94, .27, .27], dm: [.78, .16, .22], gr: [.89, .73, .29] };
  let logoTex = null;
  const orbit = [];
  const logoEl = hero.querySelector('.hx-logo');
  load(logoEl.currentSrc || logoEl.src).then((i) => { logoTex = texture(i); wake(); }).catch(() => {});
  links.forEach((a, n) => {
    load(a.dataset.img, !a.dataset.img.startsWith('/')).then((i) => {
      if (orbit.length >= 12) return;
      const asp = i.naturalWidth / i.naturalHeight, shape = 5 / 7;
      orbit.push({ a, n, tex: texture(i), cover: asp > shape ? [shape / asp, 1] : [1, asp / shape], tone: TONES[a.dataset.c] || [.6, .62, .68], hot: 0, q: null, born: performance.now() });
      orbit.sort((x, y) => x.n - y.n);
      wake();
    }).catch(() => {});
  });

  /* ---------- the state of the scene ---------- */
  const D = 9;
  let W = 1, H = 1, C = [0, 0], R = 100, rho = 2.5, tilt = 0.32, cardS = 1, res = lite ? 0.62 : 1, pxr = 1, drawn = 0;
  const baseT = still ? 40 : 0;
  let rot = 0.4, vel = still ? 0 : 0.17, flip = 0, flipV = 0, flipTo = 0, shock = -1, amp = 0;
  let mx = 0, my = 0, smx = 0, smy = 0, hot = null;

  function measure() {
    const box = hero.getBoundingClientRect(), logo = logoEl.getBoundingClientRect();
    const dpr = Math.min(devicePixelRatio || 1, matchMedia('(pointer: coarse)').matches ? 1.35 : 1.75) * res;
    pxr = dpr;
    W = Math.max(2, Math.round(box.width * dpr)); H = Math.max(2, Math.round(box.height * dpr));
    if (canvas.width !== W || canvas.height !== H) { canvas.width = W; canvas.height = H; }
    R = (logo.width / 2) * dpr;
    C = [(logo.left + logo.width / 2 - box.left) * dpr, H - (logo.top + logo.height / 2 - box.top) * dpr];
    const halfW = W / 2 / R;
    rho = Math.min(2.7, Math.max(1.5, halfW * 0.86));
    const narrow = halfW < 2.6;
    tilt = narrow ? 0.68 : 0.52;
    cardS = narrow ? Math.min(0.9, Math.max(0.68, halfW * 0.38)) : 1;
    wake();
  }
  new ResizeObserver(measure).observe(hero);

  const rotX = (p, a) => { const c = Math.cos(a), s = Math.sin(a); return [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c]; };
  const rotY = (p, a) => { const c = Math.cos(a), s = Math.sin(a); return [p[0] * c + p[2] * s, p[1], -p[0] * s + p[2] * c]; };
  const rotZ = (p, a) => { const c = Math.cos(a), s = Math.sin(a); return [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]]; };
  const wrap = (a) => { a = (a + Math.PI) % (Math.PI * 2); if (a < 0) a += Math.PI * 2; return a - Math.PI; };
  const ease = (k) => 1 - Math.pow(1 - Math.min(1, Math.max(0, k)), 3);

  /* ---------- drawing ---------- */
  const setCommon = (P, t) => {
    gl.uniform2f(P.u('uC'), C[0], C[1]);
    gl.uniform1f(P.u('uR'), R);
    if (t !== undefined) gl.uniform1f(P.u('uT'), t);
  };
  const drawQuad = (P, q) => {
    gl.uniform3fv(P.u('uQ'), new Float32Array(q.flat()));
    gl.uniform2f(P.u('uRes'), W, H);
    gl.uniform1f(P.u('uD'), D);
    gl.uniform2f(P.u('uC'), C[0], C[1]);
    gl.uniform1f(P.u('uR'), R);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  };
  const toScreen = (w) => { const k = (D - w[2]) / D; return [(C[0] + w[0] / k * R) / pxr, (H - (C[1] + w[1] / k * R)) / pxr]; };

  let t0 = performance.now(), last = t0, frames = 0, spent = 0, raf = 0, seen = true, startAt = t0;
  if (document.documentElement.dataset.gate) startAt = t0 + 700;

  function frame(now) {
    raf = 0;
    // A lighter device draws 30 times a second, not 60: half the work, still smooth.
    if (lite && !still && now - drawn < 31) { raf = requestAnimationFrame(frame); return; }
    drawn = now;
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    const t = still ? baseT : (now - t0) / 1000;
    const rv = still ? 1 : Math.min(1, Math.max(0, (now - startAt) / 2100));

    smx += (mx - smx) * Math.min(1, dt * 5);
    smy += (my - smy) * Math.min(1, dt * 5);
    if (!dragging) { vel += ((hot ? 0.025 : still ? 0 : 0.17) - vel) * Math.min(1, dt * 1.5); }
    rot += vel * dt;
    if (focusTo !== null) { const d = wrap(focusTo - rot); rot += d * Math.min(1, dt * 5); if (Math.abs(d) < 0.003) focusTo = null; }
    // The coin returns to rest after a flip with a little overshoot.
    const flipRest = flipTo;
    flipV += ((flipRest - flip) * 30 - flipV * 6.6) * dt;
    flip += flipV * dt;
    amp = Math.max(0, amp - dt * 1.1);
    if (shock >= 0) shock += dt;
    if (shock > 1.5) shock = -1;

    const sc = Math.min(1, Math.max(0, window.scrollY / Math.max(1, hero.offsetHeight)));
    const groupX = tilt + smy * 0.1 + sc * 0.55;
    const groupZ = -0.1 + smx * 0.05;

    const norm = (ax, az) => rotZ(rotX([0, 1, 0], ax), az);
    const n0 = norm(groupX, groupZ);
    const n1 = norm(groupX + 0.6 + Math.sin(t * 0.07) * 0.06, 0.55 + t * 0.012);
    const n2 = norm(groupX - 0.34, -0.75 - t * 0.008);

    gl.viewport(0, 0, W, H);
    gl.disable(gl.BLEND);
    gl.useProgram(space.p);
    setCommon(space, t);
    gl.uniform1f(space.u('uRv'), rv);
    gl.uniform1f(space.u('uShock'), shock);
    gl.uniform1f(space.u('uAmp'), amp);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.enable(gl.BLEND);

    const drawRings = (side) => {
      gl.blendFunc(gl.ONE, gl.ONE);
      gl.useProgram(rings.p);
      setCommon(rings, t);
      gl.uniform1f(rings.u('uD'), D);
      gl.uniform1f(rings.u('uSide'), side);
      gl.uniform1f(rings.u('uRv'), rv);
      gl.uniform1f(rings.u('uAmp'), amp);
      gl.uniform3fv(rings.u('uN0'), n0);
      gl.uniform3fv(rings.u('uN1'), n1);
      gl.uniform3fv(rings.u('uN2'), n2);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    };
    drawRings(-1);

    // Cards: burst out from behind the logo at the start, then orbit.
    const list = [];
    const open = ease((rv - 0.28) / 0.72);
    const n = orbit.length;
    orbit.forEach((o, i) => {
      const th = rot + (i / n) * Math.PI * 2;
      const rad = rho * open;
      let pos = [Math.sin(th) * rad, Math.sin(t * 0.7 + i * 1.7) * 0.035, Math.cos(th) * rad];
      pos = rotZ(rotX(pos, groupX), groupZ);
      const yaw = wrap(th) * 0.4;
      o.hot += ((o === hot ? 1 : 0) - o.hot) * Math.min(1, dt * 9);
      const s = cardS * (0.62 + 0.38 * open) * (1 + o.hot * 0.12);
      const hw = 0.345 * s, hh = 0.483 * s;
      const local = [[-hw, -hh, 0], [hw, -hh, 0], [-hw, hh, 0], [hw, hh, 0]];
      o.q = local.map((c) => {
        const r = rotZ(rotX(rotY(c, yaw), -0.06 + smy * 0.07), Math.sin(t * 0.4 + i) * 0.03);
        return [r[0] + pos[0], r[1] + pos[1], r[2] + pos[2]];
      });
      o.z = pos[2];
      o.ph = t * 0.55 + i * 1.3 + wrap(th) * 2.2;
      list.push(o);
    });
    list.sort((a, b) => a.z - b.z);

    const drawCard = (o) => {
      gl.useProgram(card.p);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, o.tex);
      gl.uniform1i(card.u('uTex'), 0);
      gl.uniform1f(card.u('uPh'), o.ph);
      gl.uniform1f(card.u('uHot'), o.hot);
      const lit = 0.34 + 0.66 * Math.min(1, Math.max(0, (o.z + rho) / (rho * 1.25)));
      gl.uniform1f(card.u('uLit'), lit * Math.min(1, rv * 2));
      gl.uniform3fv(card.u('uTone'), o.tone);
      gl.uniform2fv(card.u('uCover'), o.cover);
      drawQuad(card, o.q);
    };
    list.filter((o) => o.z < 0).forEach(drawCard);

    // The coin, with thickness: the same disc a few times, slightly behind itself and darker.
    if (logoTex) {
      gl.useProgram(coin.p);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, logoTex);
      gl.uniform1i(coin.u('uTex'), 0);
      gl.uniform1f(coin.u('uT'), t);
      gl.uniform1f(coin.u('uRv'), rv);
      const yaw = smx * 0.34 + Math.sin(t * 0.32) * 0.09 + flip;
      const pitch = -smy * 0.24 + Math.sin(t * 0.23) * 0.045;
      const back = Math.cos(yaw) < 0;
      gl.uniform1f(coin.u('uFlipX'), back ? 1 : 0);
      gl.uniform1f(coin.u('uFoil'), Math.min(1, Math.abs(smx) + Math.abs(smy) + amp * 1.2));
      const bob = Math.sin(t * 0.5) * 0.018;
      const coinAt = (z) => [[-1, -1, z], [1, -1, z], [-1, 1, z], [1, 1, z]].map((c) => { const r = rotX(rotY(c, yaw), pitch); return [r[0], r[1] + bob, r[2]]; });
      for (let i = 5; i >= 0; i--) {
        gl.uniform1f(coin.u('uEdge'), i === 0 ? 0 : 1);
        const z = (i - 2.5) * -0.012 * (back ? -1 : 1);
        drawQuad(coin, coinAt(z));
      }
    }
    drawRings(1);
    list.filter((o) => o.z >= 0).forEach(drawCard);

    canvas.style.opacity = String(1 - Math.max(0, sc - 0.35) * 1.4);

    // Keep it smooth: a slow device draws at a lower resolution rather than stuttering.
    spent += dt * 1000;
    if (++frames >= 45) {
      if (spent / frames > 24 && res > 0.5) { res = Math.max(0.5, res * 0.82); measure(); }
      frames = 0; spent = 0;
    }
    if (!still && seen && !document.hidden && !document.documentElement.dataset.float) raf = requestAnimationFrame(frame);
    else if (still && (Math.abs(flip - flipRest) > 0.002 || dragging || Math.abs(vel) > 0.002)) raf = requestAnimationFrame(frame);
  }
  function wake() { if (!raf) raf = requestAnimationFrame(frame); }
  new IntersectionObserver((e) => { seen = e[0].isIntersecting; if (seen) { last = performance.now(); wake(); } }, { rootMargin: '80px' }).observe(hero);
  document.addEventListener('visibilitychange', () => { last = performance.now(); wake(); });
  new MutationObserver(() => { if (!document.documentElement.dataset.float) { last = performance.now(); wake(); } }).observe(document.documentElement, { attributes: true, attributeFilter: ['data-float'] });
  canvas.addEventListener('webglcontextlost', (e) => { e.preventDefault(); hero.classList.add('no-gl'); links.forEach((a) => { const i = a.querySelector('img'); if (i) i.src = a.dataset.img; }); });

  /* ---------- touching the scene ---------- */
  const stage = hero.querySelector('.hx-stage');
  const tip = hero.querySelector('.hx-tip');
  const inside = (q, x, y) => {
    const s = [q[0], q[1], q[3], q[2]].map(toScreen);
    let on = false;
    for (let i = 0, j = 3; i < 4; j = i++) {
      if ((s[i][1] > y) !== (s[j][1] > y) && x < ((s[j][0] - s[i][0]) * (y - s[i][1])) / (s[j][1] - s[i][1]) + s[i][0]) on = !on;
    }
    return on;
  };
  const pick = (x, y) => {
    const front = [...orbit].filter((o) => o.q).sort((a, b) => b.z - a.z);
    const coinHit = () => { const dx = x * pxr - C[0], dy = (H - y * pxr) - C[1]; return dx * dx + dy * dy < R * R; };
    for (const o of front) {
      if (o.z < 0 && coinHit()) return 'coin';
      if (inside(o.q, x, y)) return o;
    }
    return coinHit() ? 'coin' : null;
  };
  const showTip = (o, x, y) => {
    if (!tip) return;
    if (!o) { tip.hidden = true; return; }
    tip.hidden = false;
    tip.textContent = o.a.dataset.t + (o.a.dataset.p ? `  ·  ${o.a.dataset.p}` : '');
    tip.style.setProperty('--tx', `${x}px`);
    tip.style.setProperty('--ty', `${y}px`);
  };
  const local = (e) => { const b = hero.getBoundingClientRect(); return [e.clientX - b.left, e.clientY - b.top]; };
  let dragging = false, downAt = null, lastX = 0, lastTime = 0, moved = 0, focusTo = null;

  const flipCoin = (e) => {
    flipTo += Math.PI * 2;
    shock = 0;
    amp = 1;
    vel += 1.5;
    if (navigator.vibrate) navigator.vibrate(14);
    wake();
  };

  hero.addEventListener('pointermove', (e) => {
    const b = hero.getBoundingClientRect();
    if (e.pointerType !== 'touch') {
      mx = ((e.clientX - b.left) / b.width - 0.5) * 2;
      my = ((e.clientY - b.top) / b.height - 0.5) * 2;
    }
    if (downAt) {
      const dx = e.clientX - lastX;
      moved = Math.max(moved, Math.abs(e.clientX - downAt[0]));
      if (moved > 6) {
        dragging = true;
        const nowT = performance.now(), dtm = Math.max(1, nowT - lastTime);
        const dr = (dx / Math.max(120, b.width * 0.34)) * 1.2;
        rot += dr;
        vel = (dr / dtm) * 1000 * 0.7;
        lastTime = nowT;
        wake();
      }
      lastX = e.clientX;
      return;
    }
    if (e.pointerType === 'mouse' && e.target.closest('.hx-stage')) {
      const [x, y] = local(e);
      const got = pick(x, y);
      hot = got && got !== 'coin' ? got : null;
      stage.style.cursor = got ? 'pointer' : 'grab';
      showTip(hot, x, y);
    }
  }, { passive: true });
  hero.addEventListener('pointerleave', () => { hot = null; mx = 0; my = 0; showTip(null); });
  stage.addEventListener('pointerdown', (e) => {
    if (e.button) return;
    downAt = [e.clientX, e.clientY];
    lastX = e.clientX; lastTime = performance.now(); moved = 0; dragging = false;
    try { stage.setPointerCapture(e.pointerId); } catch (_) {}
  });
  const release = (e) => {
    if (!downAt) return;
    const wasDrag = dragging;
    downAt = null; dragging = false;
    if (wasDrag || e.type === 'pointercancel') return;
    const [x, y] = local(e);
    const got = pick(x, y);
    if (got === 'coin') flipCoin(e);
    else if (got) got.a.click();
  };
  stage.addEventListener('pointerup', release);
  stage.addEventListener('pointercancel', release);

  // Keyboard: the cards are real links. Moving to one brings it round to the front.
  links.forEach((a) => {
    a.addEventListener('focus', () => {
      const o = orbit.find((c) => c.a === a);
      if (!o) return;
      focusTo = -(orbit.indexOf(o) / orbit.length) * Math.PI * 2;
      hot = o;
      wake();
    });
    a.addEventListener('blur', () => { hot = null; });
  });

  hero.hx = { get rot() { return rot; }, get flip() { return flip; }, get shock() { return shock; }, get cards() { return orbit.length; }, get res() { return res; }, screenOf: (i) => { const o = orbit[i]; return o?.q && { z: o.z, at: toScreen([(o.q[0][0] + o.q[3][0]) / 2, (o.q[0][1] + o.q[3][1]) / 2, (o.q[0][2] + o.q[3][2]) / 2]) }; } };
  measure();
  wake();
}
