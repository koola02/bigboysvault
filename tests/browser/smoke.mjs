// The check before every release: does the site work on a phone, a tablet and a computer?
//
// It opens the site in a real browser at three sizes and walks the path a buyer takes: home, The Vault, a search,
// a card (in its window and on its own page), the cart, and "Send order". Then it looks at the other main pages.
// On every page it checks: the page opened, nothing sticks out sideways, no script broke, nothing failed to load.
//
//   node tests/browser/smoke.mjs                         the local site, http://localhost:3000
//   node tests/browser/smoke.mjs https://www.bigboysvault.com
//   node tests/browser/smoke.mjs http://localhost:3000 --order
//
// Without --order nothing is written: the cart stays in the test browser, and "Send order" only has to send a
// visitor who isn't logged in to the log-in page. With --order (a local copy only, never the live site) it also
// joins as a throwaway trainer, sends the order, looks for it under My orders, and cancels it again.
//
// Needs Node 18+, `npm install` in this folder once (puppeteer-core), and Chrome: found by itself in the usual
// places, or named with CHROME=/path/to/chrome. Ends with code 1 when any check fails.
import puppeteer from 'puppeteer-core';
import fs from 'node:fs';

const args = process.argv.slice(2);
const BASE = (args.find((a) => /^https?:\/\//.test(a)) || 'http://localhost:3000').replace(/\/+$/, '');
const ORDER = args.includes('--order');
const LOCAL = /^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/.test(BASE);
if (ORDER && !LOCAL) { console.error('--order writes a test trainer and a test order: only for a local copy.'); process.exit(2); }

const chrome = () => {
  const guesses = [
    process.env.CHROME,
    'C:/Program Files/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser',
  ];
  const found = guesses.find((p) => p && fs.existsSync(p));
  if (!found) { console.error('Chrome not found. Set CHROME to its path.'); process.exit(2); }
  return found;
};

const SIZES = {
  phone: { width: 390, height: 844, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
  tablet: { width: 820, height: 1180, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
  computer: { width: 1440, height: 900 },
};
/** How many cards from The Vault to try before saying none can be bought. */
const TRIES = 6;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = 0, passed = 0;
const say = (ok, size, what, more = '') => { ok ? passed++ : failed++; console.log(`${ok ? '  ok ' : 'FAIL '} ${size.padEnd(8)} ${what}${more ? '  ·  ' + more : ''}`); };
const note = (size, what) => console.log(`  --  ${size.padEnd(8)} ${what}`);

const browser = await puppeteer.launch({ executablePath: chrome(), headless: 'new', args: ['--no-sandbox'] });
const hardStop = setTimeout(() => { console.log('FAIL  the whole check took too long (15 minutes)'); process.exit(1); }, 900000);

for (const [size, viewport] of Object.entries(SIZES)) {
  const ctx = await browser.createBrowserContext();
  const page = await ctx.newPage();
  await page.setViewport(viewport);
  page.on('dialog', (d) => d.accept().catch(() => {}));
  let broke = [];
  page.on('pageerror', (e) => broke.push('script: ' + String(e.message).slice(0, 140)));
  page.on('response', (r) => {
    const u = r.url();
    if (!u.startsWith(BASE) || r.status() < 400) return;
    const path = u.slice(BASE.length);
    // A local copy has the live cards but not their photos; sending an order while logged out is meant to be refused.
    if ((LOCAL && path.startsWith('/uploads/')) || (r.status() === 401 && path.startsWith('/cart/checkout'))) return;
    broke.push(`${r.status()} ${path.slice(0, 90)}`);
  });

  // Opens a page and checks the things every page must get right.
  const open = async (path, name, extra) => {
    broke = [];
    let status = 0;
    try { status = (await page.goto(BASE + path, { waitUntil: 'networkidle2', timeout: 60000 }))?.status() ?? 0; } catch (e) { say(false, size, name, 'did not open: ' + String(e.message).slice(0, 80)); return false; }
    await sleep(400);
    const seen = await page.evaluate(() => {
      const de = document.documentElement;
      const out = [...document.querySelectorAll('body *')].filter((e) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.right > innerWidth + 2 && getComputedStyle(e).position !== 'fixed'; }).slice(0, 3).map((e) => e.tagName.toLowerCase() + '.' + String(e.className).split(' ')[0]);
      return { frame: !!document.querySelector('header.hd') && !!document.querySelector('main'), sideways: de.scrollWidth > innerWidth + 1, out, bar: innerWidth >= 700 || !!document.querySelector('nav.bt') };
    });
    const problems = [];
    if (status !== 200) problems.push('answered ' + status);
    if (!seen.frame) problems.push('no header or no page body');
    if (seen.sideways) problems.push('sticks out sideways: ' + seen.out.join(', '));
    if (!seen.bar) problems.push('no bar of tabs at the bottom');
    if (extra) { const why = await extra(); if (why) problems.push(why); }
    if (broke.length) problems.push(broke.slice(0, 4).join(' | '));
    say(!problems.length, size, name, problems.join('; '));
    return !problems.length;
  };
  const count = (sel) => page.evaluate((s) => document.querySelectorAll(s).length, sel);
  const text = (sel) => page.evaluate((s) => document.querySelector(s)?.textContent.trim() || '', sel);
  const shown = (sel) => page.evaluate((s) => { const e = document.querySelector(s); return !!e && !e.hidden && e.getClientRects().length > 0; }, sel);
  // Taps the way a person does: with the thing in the middle of the screen, never under the bar at the bottom.
  const tap = async (sel) => { await page.$eval(sel, (e) => e.scrollIntoView({ block: 'center' })); await sleep(250); await page.click(sel); };
  const until = async (fn, ms = 20000) => { const end = Date.now() + ms; while (Date.now() < end) { if (await fn()) return true; await sleep(250); } return false; };

  // 1. Home: the feed.
  await open('/', 'Home', async () => ((await count('main .post, main .stories, main .box')) ? '' : 'the feed is empty'));

  // 2. The Vault: the card finder loads and shows cards.
  let cards = [], word = '';
  await open('/all', 'The Vault', async () => {
    if (!(await until(() => count('.fd-grid a.card')))) return 'no cards showed (the card finder did not load)';
    cards = await page.evaluate((n) => [...document.querySelectorAll('.fd-grid a.card')].slice(0, n).map((a) => a.getAttribute('href')).filter(Boolean), TRIES);
    word = (await text('.fd-grid a.card .card-t')).split(/[^A-Za-z]+/).find((w) => w.length > 3) || '';
    return cards.length ? '' : 'the first card has no link';
  });

  // 3. A card opens in its window from The Vault, and closes again.
  if (cards.length) {
    broke = [];
    await tap('.fd-grid a.card');
    const opened = await until(async () => (await shown('.cardwin')) && /₱/.test(await text('.cardwin .price')), 15000);
    let closed = false;
    if (opened) { await page.click('.cardwin [data-cw-x]'); closed = await until(async () => !(await shown('.cardwin')), 8000); }
    say(opened && closed && !broke.length, size, 'A card opens in its window', !opened ? 'the window or its price did not show' : !closed ? 'the window did not close' : broke.slice(0, 3).join(' | '));
  }

  // 4. Searching finds cards.
  if (word) await open('/search?q=' + encodeURIComponent(word), `Search for “${word}”`, async () => ((await until(() => count('.fd-grid a.card'))) ? '' : 'the search found nothing'));

  // 5. A card's own page: its name and its price.
  if (cards.length) await open(cards[0], 'A card’s page', async () => (!/₱/.test(await text('main .price')) ? 'no price' : (await count('main h1')) ? '' : 'no name'));

  // 6. The cart. The first card that can really be bought is used: a copy held by someone's open order can't be.
  let bought = '', why = 'none of them has an Add to cart button';
  for (const href of cards) {
    await page.goto(BASE + href, { waitUntil: 'networkidle2', timeout: 60000 }).catch(() => {});
    await page.evaluate(() => { try { localStorage.removeItem('bbv:cart'); } catch (_) {} });
    await page.reload({ waitUntil: 'networkidle2' }).catch(() => {});
    if (!(await count('main button.cart-add'))) continue;
    broke = [];
    await tap('main button.cart-add');
    if (!(await until(async () => (await text('#cartN')) === '1' && (await shown('#cartFab')), 8000))) { why = 'the cart did not count the card'; break; }
    await page.click('#cartFab');
    if (!(await until(() => shown('#cartDrawer'), 8000))) { why = 'the cart did not open'; break; }
    // The cart asks the site whether each line can still be bought: wait for that answer to settle.
    await sleep(1800);
    const line = await page.evaluate(() => ({ off: !!document.querySelector('#cartList .cart-line.off'), ready: /^Send order/.test(document.getElementById('cartGo').textContent.trim()) && !document.getElementById('cartGo').disabled }));
    if (line.off) { why = 'every card tried is held by an open order'; continue; }
    if (!line.ready) { why = '“Send order” is not ready'; break; }
    if (broke.length) { why = broke.slice(0, 3).join(' | '); break; }
    bought = href;
    break;
  }
  if (cards.length) say(!!bought, size, 'Add to cart, open the cart', bought ? '' : `${why} (tried up to ${TRIES} cards)`);

  if (bought) {
    await Promise.all([page.waitForNavigation({ timeout: 20000 }).catch(() => null), page.click('#cartGo')]);
    const atLogin = new URL(page.url()).pathname === '/login';
    say(atLogin, size, 'Send order asks a visitor to log in', atLogin ? '' : 'went to ' + page.url().slice(BASE.length));
    if (atLogin && ORDER) {
      const name = 'Smoke' + Math.random().toString(36).slice(2, 8);
      await page.goto(BASE + '/join', { waitUntil: 'networkidle2' });
      await page.type('input[name="name"]', name);
      await page.type('input[name="password"]', 'smoke-test-' + name);
      await Promise.all([page.waitForNavigation({ timeout: 30000 }).catch(() => null), page.click('main form button[type="submit"], main form .btn')]);
      await page.goto(BASE + bought, { waitUntil: 'networkidle2' });
      if (!(await shown('#cartDrawer'))) await page.click('#cartFab').catch(() => {});
      const again = await until(() => shown('#cartDrawer'), 10000);
      let sent = false;
      if (again && (await until(async () => !(await page.evaluate(() => document.getElementById('cartGo').disabled)), 10000))) {
        await page.click('#cartGo');
        sent = await until(async () => /Order sent/.test(await text('.cart-toast')) || (await text('#cartN')) === '0', 15000);
      }
      let listed = false;
      if (sent) { await page.goto(BASE + '/me/orders?tab=buying', { waitUntil: 'networkidle2' }); listed = (await count('.st-order')) > 0; }
      say(sent && listed, size, `A new trainer (${name}) sends the order`, !again ? 'the cart did not come back after joining' : !sent ? 'the order was not sent' : !listed ? 'the order is not under My orders' : '');
      // Hand the card back, so the next run (and the next buyer on this copy) can order it.
      if (listed && (await count('[data-vcancel]'))) {
        await Promise.all([page.waitForNavigation({ timeout: 15000 }).catch(() => null), tap('[data-vcancel]')]);
        if (await count('[data-vcancel]')) note(size, 'The test order could not be cancelled; cancel it under My orders');
      }
    }
  }

  // 7. The other places a visitor goes.
  for (const [path, name] of [['/stores', 'Stores'], ['/store/vault', 'The vault’s store'], ['/trainers', 'Trainers'], ['/vouches', 'Vouches'], ['/ask', 'Ask the Vault'], ['/chat', 'Vault chat'], ['/login', 'Log in'], ['/join', 'Join']]) {
    await open(path, name);
  }
  await ctx.close();
}

// 8. The site says it is well.
try {
  const r = await fetch(BASE + '/healthz');
  const j = await r.json();
  say(r.ok && j.ok === true, 'server', '/healthz', r.ok && j.ok === true ? '' : JSON.stringify(j).slice(0, 120));
} catch (e) { say(false, 'server', '/healthz', String(e.message).slice(0, 100)); }

clearTimeout(hardStop);
await browser.close();
console.log(`\n${failed ? 'NOT READY' : 'READY'}: ${passed} passed, ${failed} failed  (${BASE})`);
process.exit(failed ? 1 : 0);
