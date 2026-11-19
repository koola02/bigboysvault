// Walks the LOCAL site as a visitor, a seller and an admin and lists every short line of words on the pages that is
// not in proper case (a label with a word starting in a small letter). Sentences are left out. What is listed is
// read by a person: names, card titles and what people wrote show up too and are ignored by eye.
import { createRequire } from 'node:module';
const require = createRequire(new URL('../browser/package.json', import.meta.url));
const puppeteer = require('puppeteer-core');
// Run against a LOCAL copy only: node tests/case/audit.mjs  (BASE, ADMIN_TOKEN, SELLER=id:name and STAFF=id:name can be set).
const BASE = process.env.BASE || 'http://localhost:3100', ADMIN = process.env.ADMIN_TOKEN || 'local-admin-for-tests-only';
const SELLER = (process.env.SELLER || '17:Stib').split(':'), STAFF = (process.env.STAFF || '4:Hayme').split(':');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const admin = (path, body) => fetch(`${BASE}${path}`, { method: 'POST', headers: { 'Content-Type': 'application/json', Accept: 'application/json', Cookie: `marcuru_admin=${ADMIN}`, Origin: BASE }, body: JSON.stringify(body) });
const pass = async (id) => { const r = await admin(`/admin/trainers/${id}`, { action: 'password' }); return r.ok ? (await r.json()).password : ''; };

export const CASE = () => {
  const THROUGH = new Set(['B', 'STRONG', 'EM', 'I', 'U', 'CODE', 'MARK', 'SUP', 'SUB']);
  const KEEP = new Set(['ex', 'px', 'vs', 'x', 'v']);
  const OPEN = '([{"\'“‘«‹+✓✔⛔⏳🔒★☆·•–—-›»';
  const END = /[.!?…]["”’')\]]*\s*$/;
  const out = [];
  const seen = new Set();
  const textOf = (el) => { let t = ''; for (const n of el.childNodes) { if (n.nodeType === 3) t += n.data; else if (n.nodeType === 1 && THROUGH.has(n.tagName)) t += textOf(n); else if (n.nodeType === 1) t += ' \u0001 '; } return t; };
  const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let t = walk.nextNode(); t; t = walk.nextNode()) {
    if (!/[A-Za-z]/.test(t.data)) continue;
    let el = t.parentElement;
    while (el && THROUGH.has(el.tagName)) el = el.parentElement;
    if (!el || seen.has(el)) continue;
    seen.add(el);
    if (el.closest('svg, script, style, noscript, template, textarea, pre, [contenteditable]')) continue;
    const r = el.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) continue;
    const cs = getComputedStyle(el);
    if (cs.visibility !== 'visible' || cs.display === 'none') continue;
    const whole = textOf(el).replace(/\s+/g, ' ').trim();
    const words = whole.replace(/\u0001/g, ' ').replace(/\s+/g, ' ').trim();
    const list = words.split(' ').filter((w) => /[A-Za-z]/.test(w));
    if (!list.length) continue;
    if (END.test(words) || /[.!?] +[A-Z“"]/.test(words) || list.length >= 7 || (words.includes(',') && list.length >= 5)) continue;
    const bad = list.filter((w) => { if (KEEP.has(w.toLowerCase())) return false; let i = 0; while (i < w.length && OPEN.includes(w[i])) i += 1; const ch = w[i]; return !!ch && ch >= 'a' && ch <= 'z' && !(w[i + 1] >= 'A' && w[i + 1] <= 'Z'); });
    if (!bad.length) continue;
    const cls = (n) => (typeof n.className === 'string' && n.className.trim() ? '.' + n.className.trim().split(/\s+/)[0] : '');
    out.push(`${el.parentElement ? el.parentElement.tagName.toLowerCase() + cls(el.parentElement) + ' > ' : ''}${el.tagName.toLowerCase()}${cls(el)}\t${words.slice(0, 90)}`);
  }
  // Words kept in attributes people read.
  for (const el of document.querySelectorAll('[placeholder], [title], [aria-label], [alt]')) {
    for (const a of ['placeholder', 'title', 'aria-label', 'alt']) {
      const v = (el.getAttribute(a) || '').trim();
      if (!v || !/[a-z]/.test(v) || el.closest('svg')) continue;
      const list = v.split(/\s+/).filter((w) => /[A-Za-z]/.test(w));
      if (END.test(v) || list.length >= 7 || (v.includes(',') && list.length >= 5)) continue;
      const bad = list.filter((w) => { let i = 0; while (i < w.length && OPEN.includes(w[i])) i += 1; const ch = w[i]; return !!ch && ch >= 'a' && ch <= 'z' && !KEEP.has(w.toLowerCase()); });
      if (bad.length) out.push(`@${a} ${el.tagName.toLowerCase()}\t${v.slice(0, 90)}`);
    }
  }
  return [...new Set(out)];
};

if (process.argv[1] && process.argv[1].endsWith('audit.mjs')) {
  const hayme = { name: STAFF[1], pw: await pass(Number(STAFF[0])) }, stib = { name: SELLER[1], pw: await pass(Number(SELLER[0])) };
  const browser = await puppeteer.launch({ executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe', headless: 'new', args: ['--no-sandbox'] });
  const found = new Map();
  const note = (list, where) => { for (const l of list) { if (!found.has(l)) found.set(l, new Set()); found.get(l).add(where); } };
  const VIEWS = { phone: { width: 390, height: 844, deviceScaleFactor: 1, isMobile: true, hasTouch: true }, computer: { width: 1440, height: 900, deviceScaleFactor: 1 } };
  const PAGES = {
    visitor: ['/', '/stores', '/store/vault', '/store/hayme', '/store/stib', '/store/hayme/binders/pikachu', '/trainers', '/trainers/4', '/trainers/17', '/vouches', '/ask', '/chat', '/login', '/join', '/all', '/search?q=pikachu', '/collections/pikachu-set', '/posts/22', '/nothing-here'],
    seller: ['/', '/me', '/me/store', '/me/listings', '/me/add', '/me/orders', '/me/notifications', '/me/settings', '/me/bookmarks', '/me/messages', '/store/stib', '/trainers/17', '/apply'],
    admin: ['/admin', '/admin/dashboard', '/admin/quick', '/admin/orders', '/admin/trainers', '/admin/trainers?view=vouches', '/admin/sellers', '/admin/store', '/admin/showcase', '/admin/look', '/admin/messages', '/admin/new', '/me', '/me/store', '/me/listings', '/me/orders', '/store/hayme', '/store/vault', '/'],
  };
  for (const [vname, vp] of Object.entries(VIEWS)) {
    for (const [who, pages] of Object.entries(PAGES)) {
      const ctx = await browser.createBrowserContext(); const page = await ctx.newPage(); await page.setViewport(vp);
      if (who !== 'visitor') {
        const u = who === 'seller' ? stib : hayme;
        await page.goto(BASE + '/login', { waitUntil: 'networkidle2' });
        await page.type('input[name="name"]', u.name); await page.type('input[name="password"]', u.pw);
        await Promise.all([page.waitForNavigation({ timeout: 30000 }).catch(() => null), page.keyboard.press('Enter')]);
        if (who === 'admin') await page.setCookie({ name: 'marcuru_admin', value: ADMIN, url: BASE });
      }
      for (const p of pages) {
        try { await page.goto(BASE + p, { waitUntil: 'networkidle2', timeout: 60000 }); } catch (e) { console.error('!! ' + p + ' ' + e.message); continue; }
        await sleep(500);
        note(await page.evaluate(CASE), `${who} ${p}`);
        // The things a tap opens on this page: menus and windows the scripts draw.
        if (p === '/' && who !== 'visitor') {
          await page.evaluate(() => document.querySelector('.composer-ask[data-compose]')?.click()); await sleep(400);
          note(await page.evaluate(CASE), `${who} ${p} (post box)`);
          await page.evaluate(() => document.querySelector('.sheet [data-x]')?.click()); await sleep(200);
          await page.evaluate(() => document.querySelector('[data-post-menu]')?.click()); await sleep(300);
          note(await page.evaluate(CASE), `${who} ${p} (post menu)`);
          await page.keyboard.press('Escape');
          await page.evaluate(() => document.querySelector('[data-comments-open]')?.click()); await sleep(600);
          note(await page.evaluate(CASE), `${who} ${p} (comments)`);
        }
        if (p === '/' || p === '/me') {
          await page.evaluate(() => (document.querySelector('#hdMe, .hd-me, [data-account], .hd-av') || document.querySelector('header [aria-haspopup]'))?.click()); await sleep(400);
          note(await page.evaluate(CASE), `${who} ${p} (menu)`);
          await page.keyboard.press('Escape');
        }
        if (/^\/store\//.test(p)) {
          await page.evaluate(() => document.querySelector('[data-share-ref]')?.click()); await sleep(400);
          note(await page.evaluate(CASE), `${who} ${p} (share)`);
          await page.keyboard.press('Escape'); await sleep(200);
          await page.evaluate(() => document.querySelector('[data-cart-open], .hd-cart')?.click()); await sleep(500);
          note(await page.evaluate(CASE), `${who} ${p} (cart)`);
          await page.keyboard.press('Escape');
        }
      }
      await ctx.close();
    }
  }
  await browser.close();
  const rows = [...found.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  for (const [l, where] of rows) console.log(`${l}\t← ${[...where].slice(0, 3).join(' | ')}${where.size > 3 ? ` (+${where.size - 3})` : ''}`);
  console.error(`${rows.length} lines not in proper case`);
}
