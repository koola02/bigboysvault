/* Call and message alerts: this phone or computer rings for a call and shows new messages even with the site
   closed (Web Push: the server side is src/push.rs, the alerts themselves static/sw.js).
   - window.bbvAlerts: state(), enable() (only from a tap: browsers ask permission only then), disable().
   - Places marked [data-alerts] ("panel": the inbox drop-down, "page": the Messages page, "settings": notification
     settings) show a short card while this device doesn't have alerts on, or how to get them.
   - Keeps the server's copy of this device's alert address current, and drops it on signing out, or when someone
     else is signed in on this browser (alerts belong to the person who turned them on). */
(() => {
  const ok = 'serviceWorker' in navigator && 'PushManager' in window && 'Notification' in window;
  const ua = navigator.userAgent || '';
  const ios = /iphone|ipad|ipod/i.test(ua) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
  const android = /android/i.test(ua);
  const installed = matchMedia('(display-mode: standalone)').matches || matchMedia('(display-mode: fullscreen)').matches || navigator.standalone === true;
  const ls = {
    get(k) { try { return localStorage.getItem(k); } catch (_) { return null; } },
    set(k, v) { try { if (v == null) localStorage.removeItem(k); else localStorage.setItem(k, v); } catch (_) {} },
  };
  const who = () => (window.bbvWho ? window.bbvWho() : fetch('/me/who', { credentials: 'same-origin', headers: { Accept: 'application/json' } }).then((r) => r.json())).catch(() => ({}));
  const post = async (url, body) => {
    const r = await fetch(url, { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json', Accept: 'application/json' }, body: JSON.stringify(body) });
    const j = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
    return j;
  };
  const bytes = (k) => { const s = atob(k.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(k.length / 4) * 4, '=')); return Uint8Array.from(s, (c) => c.charCodeAt(0)); };
  // A subscription made with another key (the site's key changed) can't be used.
  const sameKey = (sub, key) => {
    try { const a = new Uint8Array(sub.options.applicationServerKey); const b = bytes(key); return a.length === b.length && a.every((x, i) => x === b[i]); } catch (_) { return true; }
  };
  const subscription = async () => {
    if (!ok) return null;
    const r = await navigator.serviceWorker.getRegistration('/').catch(() => null);
    return r ? r.pushManager.getSubscription().catch(() => null) : null;
  };
  const say = (msg, bad) => { if (window.bbvToast) window.bbvToast(msg, bad); };

  /** on | off | blocked | homescreen (iPhone/iPad: add to Home Screen first) | unsupported */
  async function state() {
    if (ios && !installed) return 'homescreen';
    if (!ok) return 'unsupported';
    if (Notification.permission === 'denied') return 'blocked';
    return Notification.permission === 'granted' && (await subscription()) ? 'on' : 'off';
  }

  async function enable() {
    if (ios && !installed) return 'homescreen';
    if (!ok) return 'unsupported';
    const w = await who();
    if (!w.member) { location.href = '/login'; return 'off'; }
    if (!w.push_key) throw new Error('Alerts aren’t ready on the site yet. Please try again in a minute.');
    const p = await Notification.requestPermission();
    if (p !== 'granted') return p === 'denied' ? 'blocked' : 'off';
    const r = await navigator.serviceWorker.register('/sw.js', { scope: '/' });
    await navigator.serviceWorker.ready;
    let sub = await r.pushManager.getSubscription();
    if (sub && !sameKey(sub, w.push_key)) { await sub.unsubscribe().catch(() => {}); sub = null; }
    if (!sub) sub = await r.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: bytes(w.push_key) });
    await post('/api/notifications/push', sub.toJSON());
    ls.set('bbv:alerts-owner', String(w.member.id)); ls.set('bbv:alerts-sync', String(Date.now()));
    return 'on';
  }

  async function disable() {
    const sub = await subscription();
    if (sub) { await post('/api/notifications/push/forget', { endpoint: sub.endpoint }).catch(() => {}); await sub.unsubscribe().catch(() => {}); }
    ls.set('bbv:alerts-owner', null);
    return 'off';
  }

  // On every page: the old worker from before alerts moved to /sw.js goes; the address is kept current; another
  // person signed in (or nobody) means this browser stops getting the last person's alerts.
  async function sync() {
    if (!ok) return;
    navigator.serviceWorker.getRegistrations().then((rs) => rs.forEach((r) => { if (new URL(r.scope).pathname === '/static/') r.unregister(); })).catch(() => {});
    const w = await who();
    if (!('member' in w)) return; // couldn't ask: change nothing
    const sub = await subscription();
    if (!sub) return;
    const me = w.member ? String(w.member.id) : null;
    const owner = ls.get('bbv:alerts-owner');
    if (!me || owner !== me) {
      await post('/api/notifications/push/forget', { endpoint: sub.endpoint }).catch(() => {});
      await sub.unsubscribe().catch(() => {});
      ls.set('bbv:alerts-owner', null);
      return;
    }
    if (Notification.permission !== 'granted') return;
    // Made with a key the site no longer uses: no push could reach it. Renew it (permission is already given); if the
    // browser wants a tap for that, the device honestly shows "off" and the card offers to turn alerts on again.
    if (w.push_key && !sameKey(sub, w.push_key)) {
      await post('/api/notifications/push/forget', { endpoint: sub.endpoint }).catch(() => {});
      await sub.unsubscribe().catch(() => {});
      try {
        const r = await navigator.serviceWorker.getRegistration('/');
        const fresh = await r.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: bytes(w.push_key) });
        await post('/api/notifications/push', fresh.toJSON());
        ls.set('bbv:alerts-sync', String(Date.now()));
      } catch (_) { ls.set('bbv:alerts-owner', null); }
      return;
    }
    // Once a day the server hears this device's address again: browsers change it now and then, Safari silently.
    if (Date.now() - Number(ls.get('bbv:alerts-sync') || 0) > 864e5) {
      await post('/api/notifications/push', sub.toJSON()).then(() => ls.set('bbv:alerts-sync', String(Date.now()))).catch(() => {});
    }
  }

  // Signing out: this device stops getting this person's alerts first.
  document.addEventListener('submit', (e) => {
    const f = e.target;
    if (!(f instanceof HTMLFormElement) || f.dataset.alertsDone) return;
    let path = ''; try { path = new URL(f.action, location.href).pathname; } catch (_) {}
    if (path !== '/signout') return;
    e.preventDefault();
    let done = false;
    const go = () => { if (done) return; done = true; f.dataset.alertsDone = '1'; f.submit(); };
    setTimeout(go, 1500);
    subscription().then((s) => (s ? disable() : null)).finally(go);
  }, true);

  // ---------- the cards ----------
  const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const icon = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9M13.7 21a2 2 0 0 1-3.4 0"/></svg>';
  const ANDROID_TIP = 'Tip for Android: so calls ring while the phone sleeps, set Chrome’s battery use to Unrestricted (Settings › Apps › Chrome › Battery).';

  function card(place, st) {
    const kind = place.dataset.alerts;
    const later = Number(ls.get('bbv:alerts-later') || 0) > Date.now();
    place.replaceChildren();
    const show = (on) => { place.hidden = !on; };
    if (st === 'guest' || st === 'unsupported' || (st === 'on' && kind === 'panel') || (later && kind === 'panel' && st !== 'on')) { show(false); return; }
    const box = el('div', 'alerts-in'); box.innerHTML = icon;
    const words = el('div', 'alerts-t');
    const btns = el('div', 'alerts-b');
    if (st === 'on') {
      words.append(el('b', null, 'Calls and messages reach this device ✓'), el('span', null, android ? ANDROID_TIP : 'Even with the site closed.'));
      const off = el('button', 'btn ghost sm', 'Turn off'); off.type = 'button';
      off.addEventListener('click', async () => { off.disabled = true; await disable(); say('Alerts are off on this device.'); paint(); });
      btns.append(off);
    } else if (st === 'blocked') {
      words.append(el('b', null, 'Alerts are blocked for this site'), el('span', null, 'Allow notifications for this site in your browser’s settings, then come back here to turn them on.'));
    } else if (st === 'homescreen') {
      words.append(el('b', null, 'Get calls and messages on your iPhone'), el('span', null, 'Tap Share, then “Add to Home Screen”. Open Big Boys Vault from your Home Screen and turn alerts on there.'));
    } else {
      words.append(el('b', null, 'Get calls and messages even when the site is closed'), el('span', null, 'Your phone rings for a call and shows new messages, like an app.'));
      const on = el('button', 'btn sm', 'Turn on'); on.type = 'button';
      on.addEventListener('click', async () => {
        on.disabled = true;
        try {
          const r = await enable();
          if (r === 'on') say(android ? `Alerts on ✓ ${ANDROID_TIP}` : 'Alerts on ✓ Calls and messages now reach this device.');
          else if (r === 'blocked') say('Alerts are blocked for this site. Allow them in your browser’s settings.', true);
          else if (r === 'homescreen') say('On iPhone, add Big Boys Vault to your Home Screen first.', true);
        } catch (err) { say(err.message || 'Couldn’t turn alerts on.', true); }
        paint();
      });
      btns.append(on);
      if (kind !== 'settings') {
        const no = el('button', 'btn ghost sm', 'Not now'); no.type = 'button';
        no.addEventListener('click', () => { ls.set('bbv:alerts-later', String(Date.now() + 7 * 864e5)); paint(); });
        btns.append(no);
      }
    }
    if (st === 'homescreen' && kind !== 'settings') {
      const no = el('button', 'btn ghost sm', 'Got it'); no.type = 'button';
      no.addEventListener('click', () => { ls.set('bbv:alerts-later', String(Date.now() + 14 * 864e5)); paint(); });
      btns.append(no);
    }
    box.append(words, btns); place.append(box); show(true);
  }

  async function paint() {
    const places = document.querySelectorAll('[data-alerts]');
    if (!places.length) return;
    const w = await who();
    const st = w.member ? await state() : 'guest';
    places.forEach((p) => card(p, st));
  }

  window.bbvAlerts = { state, enable, disable, paint };
  const start = () => { sync().catch(() => {}).finally(paint); };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start); else start();
  // Back from the browser's settings (a permission changed): the cards say what is true now.
  document.addEventListener('visibilitychange', () => { if (!document.hidden) paint(); });
})();
