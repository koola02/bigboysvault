// Shows a push notification from the vault. Sending needs the server's VAPID keys (see docs/SELLER_API.md).
self.addEventListener('push', (e) => {
  let d = {};
  try { d = e.data ? e.data.json() : {}; } catch (_) { d = { body: e.data ? e.data.text() : '' }; }
  e.waitUntil(self.registration.showNotification(d.title || 'Big Boys Vault', { body: d.body || '', icon: '/static/img/logo-sm.png', data: d.url || '/me/notifications' }));
});
self.addEventListener('notificationclick', (e) => {
  e.notification.close();
  e.waitUntil(clients.openWindow(e.notification.data || '/me/notifications'));
});
