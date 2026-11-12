/* Big Boys Vault's service worker: shows calls, messages and notifications that arrive by Web Push while the site is
   closed or the phone is locked (the server side is src/push.rs), and answers or declines a call from its alert.
   It has no fetch handler, so pages load straight from the network as if it weren't here.

   Rules it keeps (browsers punish breaking them):
   - every push shows an alert, except in Chrome while a page of the site is on screen (that page rings or shows
     the message itself, and Chrome allows it then). Safari takes alerts away from a site that skips one;
   - an alert that replaces another closes the old one first (Safari doesn't replace by tag). */
const LOGO = '/static/img/icon-192.png';
const UA = self.navigator.userAgent || '';
const SAFARI = /safari/i.test(UA) && !/chrome|chromium|crios|android|edg|fxios|firefox/i.test(UA);

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));

const windows = () => self.clients.matchAll({ type: 'window', includeUncontrolled: true });
// Chrome lets a push show nothing while a page of the site is on screen; Safari never does.
const pageOnScreen = async () => !SAFARI && (await windows()).some((c) => c.visibilityState === 'visible');
const post = (url, body) => fetch(url, { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json', Accept: 'application/json' }, body: JSON.stringify(body) });

// Whether a call still rings, asked before ringing: an alert that arrives late never rings for a call that is over.
async function callState(d) {
  const stop = new AbortController(); const t = setTimeout(() => stop.abort(), 2500);
  try {
    const r = await fetch(`/dm/${d.thread}/call-state?call=${encodeURIComponent(d.call)}`, { credentials: 'same-origin', headers: { Accept: 'application/json' }, signal: stop.signal });
    if (!r.ok) return 'ringing';
    return (await r.json()).state || 'ringing';
  } catch (_) { return 'ringing'; } finally { clearTimeout(t); }
}

async function ring(d) {
  const tag = `call-${d.thread}`;
  const state = await callState(d);
  if (state !== 'ringing') return callEnded({ ...d, reason: state });
  // Over already on this device (its "missed"/"answered" notice is up): nothing rings again.
  const done = (await self.registration.getNotifications({ tag })).find((n) => n.data && n.data.kind === 'call-end' && n.data.call === d.call);
  if (done) {
    if (!SAFARI) return;
    // Safari needs every push to show something: the same notice again, quietly.
    done.close();
    return self.registration.showNotification(done.title, { body: done.body, tag, silent: true, renotify: false, icon: done.icon || LOGO, badge: LOGO, timestamp: Date.now(), data: done.data });
  }
  // The caller's screen moves from "Calling…" to "Ringing…": it really rings here.
  post(`/dm/${d.thread}/call`, { type: 'ringing', call: d.call, page: 'push' }).catch(() => {});
  if (await pageOnScreen()) return;
  for (const n of await self.registration.getNotifications({ tag })) n.close();
  return self.registration.showNotification(`${d.from || 'Someone'} is calling you`, {
    body: 'Voice call · Big Boys Vault', tag, renotify: true, requireInteraction: true, silent: false,
    icon: d.avatar || LOGO, badge: LOGO, timestamp: Date.now(), vibrate: [700, 300, 700, 300, 700],
    actions: [{ action: 'answer', title: 'Answer' }, { action: 'decline', title: 'Decline' }],
    data: { kind: 'call', call: d.call, thread: d.thread, url: d.url },
  });
}

const ENDED = {
  missed: (who) => [`Missed call from ${who}`, 'Tap to see the chat'],
  answered: (who) => [`Call with ${who}`, 'Answered'],
  declined: (who) => [`Call from ${who}`, 'Declined'],
  ended: (who) => [`Call with ${who}`, 'Ended'],
};

async function callEnded(d) {
  const tag = `call-${d.thread}`;
  const open = await self.registration.getNotifications({ tag });
  // A newer call in the same chat keeps ringing; only this call's alert goes.
  const newer = open.find((n) => n.data && n.data.kind === 'call' && n.data.call !== d.call);
  for (const n of open) if (n !== newer) n.close();
  if (newer) {
    if (!SAFARI) return; // the site's alert (the newer call) is on screen, which is all Chrome and Firefox ask
    // Safari needs every push to show something: the newer call's alert again, quietly.
    newer.close();
    return self.registration.showNotification(newer.title, { body: newer.body, tag, icon: newer.icon || LOGO, badge: LOGO, requireInteraction: true, silent: true, renotify: false, timestamp: Date.now(), actions: newer.actions, data: newer.data });
  }
  if (await pageOnScreen()) return;
  if (d.reason === 'superseded') {
    // The caller is calling again: a quiet stand-in, which the new call's ring replaces.
    return self.registration.showNotification(`Call from ${d.from || 'someone'}`, { body: 'Calling again…', tag, silent: true, renotify: false, icon: d.avatar || LOGO, badge: LOGO, timestamp: Date.now(), data: { kind: 'call-end', call: d.call, thread: d.thread, url: d.url } });
  }
  const [title, body] = (ENDED[d.reason] || ENDED.ended)(d.from || 'someone');
  // A missed call makes a sound only if nothing rang before (the phone was out of reach while it rang).
  return self.registration.showNotification(title, {
    body, tag, renotify: false, silent: d.reason !== 'missed' || open.length > 0,
    icon: d.avatar || LOGO, badge: LOGO, timestamp: Date.now(), data: { kind: 'call-end', call: d.call, thread: d.thread, url: d.url },
  });
}

async function message(d) {
  if (await pageOnScreen()) return;
  const tag = `dm-${d.thread}`;
  for (const n of await self.registration.getNotifications({ tag })) n.close();
  return self.registration.showNotification(d.from || 'New message', {
    body: d.text || 'Sent you a message', tag, renotify: true, silent: false, icon: d.avatar || LOGO, badge: LOGO,
    timestamp: Date.now(), data: { kind: 'message', thread: d.thread, url: d.url },
  });
}

function note(d) {
  return self.registration.showNotification('Big Boys Vault', {
    body: d.text || 'Something new for you', tag: `note-${d.kind || 'news'}-${Date.now()}`, silent: false,
    icon: LOGO, badge: LOGO, timestamp: Date.now(), data: { kind: 'note', url: d.url || '/me/notifications' },
  });
}

self.addEventListener('push', (e) => {
  let d = {};
  try { d = e.data ? e.data.json() : {}; } catch (_) { d = { type: 'note', text: e.data ? e.data.text() : '' }; }
  const work = d.type === 'call' ? ring(d) : d.type === 'call-end' ? callEnded(d) : d.type === 'message' ? message(d) : note(d);
  // Whatever happens above, an alert is shown: a push that shows nothing costs the site its alerts.
  e.waitUntil(Promise.resolve(work).catch(() => note({ text: d.text || 'Something new for you', url: d.url })));
});

self.addEventListener('notificationclick', (e) => {
  const n = e.notification, d = n.data || {};
  n.close();
  if (d.kind === 'call' && e.action === 'decline') {
    e.waitUntil(post(`/dm/${d.thread}/call`, { type: 'decline', call: d.call, page: 'push' }).catch(() => {}));
    return;
  }
  const answer = d.kind === 'call' && e.action === 'answer';
  const url = new URL(d.url || '/', self.location.origin);
  if (d.kind === 'call') url.searchParams.set('call', d.call);
  e.waitUntil((async () => {
    // A page of the site that is already open takes the call (or the chat); otherwise a new one opens.
    const open = (await windows()).filter((c) => new URL(c.url).origin === self.location.origin);
    const page = open.find((c) => c.visibilityState === 'visible') || open[0];
    if (page) {
      page.postMessage({ bbv: d.kind === 'call' ? 'call' : 'open', call: d.call, thread: d.thread, answer, url: url.pathname + url.search });
      return page.focus();
    }
    // A new page answers only with a one-time pass left here (a link from anywhere else can't answer a call).
    if (answer) {
      const pass = (self.crypto && crypto.randomUUID) ? crypto.randomUUID() : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}-${Math.random().toString(36).slice(2)}`;
      try {
        const box = await caches.open('bbv-answer');
        await box.put(`/__answer/${pass}`, new Response(JSON.stringify({ call: d.call, exp: Date.now() + 90000 }), { headers: { 'Content-Type': 'application/json' } }));
        url.searchParams.set('answer', pass);
      } catch (_) { url.searchParams.delete('answer'); }
    }
    return self.clients.openWindow(url.href);
  })());
});

// A browser that renews its alert address on its own: the server is told the new one.
self.addEventListener('pushsubscriptionchange', (e) => {
  e.waitUntil((async () => {
    const key = e.oldSubscription && e.oldSubscription.options && e.oldSubscription.options.applicationServerKey;
    const sub = e.newSubscription || (key ? await self.registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key }) : null);
    if (sub) await post('/api/notifications/push', sub.toJSON());
  })().catch(() => {}));
});
