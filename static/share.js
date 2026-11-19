/* Share (the owner, 19 Nov 2026: "share a binder within messages and even outside the site like facebook messenger").
   One small window for a binder, a store, a card or a post:
   - Send in a message: pick someone here, and the chat opens with it attached, ready to send (static/dm.js).
   - Messenger: a phone's own share menu (Messenger, Viber and the rest) where it has one; on a computer the link is
     copied and Messenger opens, ready to paste.
   - Facebook, and Copy link.
   Any button with data-share-ref opens it: data-share-ref="binder:12" (what Messages attaches), data-share-url (its
   address on the site), data-share-title (its name). Other scripts call window.bbvShare.open({ ref, url, title }). */
(() => {
  if (window.bbvShare) return;
  const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const ICONS = {
    chat: '<path d="M21 11.5a8.4 8.4 0 0 1-9 8.3 9 9 0 0 1-3.8-.8L3 20l1.2-4.3A8 8 0 0 1 3 11.5 8.4 8.4 0 0 1 12 3.2a8.4 8.4 0 0 1 9 8.3z"/>',
    bolt: '<path d="M12 3C6.9 3 3 6.7 3 11.4c0 2.6 1.200 4.900 3.200 6.400V21l3-1.600c.900.200 1.800.400 2.800.400 5.100 0 9-3.700 9-8.400S17.100 3 12 3z"/><path d="M7.500 13.500l3-3.200 2.400 2 3.600-2.800-3 3.400-2.400-2z"/>',
    fb: '<path d="M14 8.500V7c0-.800.300-1.300 1.400-1.300H17V3h-2.300C12 3 11 4.700 11 6.800v1.700H9V11h2v10h3V11h2.300l.400-2.500z"/>',
    link: '<path d="M10 13a5 5 0 0 0 7.100 0l3-3a5 5 0 0 0-7.100-7.100l-1.500 1.500"/><path d="M14 11a5 5 0 0 0-7.100 0l-3 3a5 5 0 0 0 7.100 7.100l1.500-1.500"/>',
    more: '<path d="M4 12v7a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-7M16 6l-4-4-4 4M12 2v13"/>',
    x: '<path d="M6 6l12 12M18 6L6 18"/>',
  };
  const svg = (name) => `<svg viewBox="0 0 24 24" aria-hidden="true">${ICONS[name]}</svg>`;
  const toast = (msg, bad) => {
    let t = document.querySelector('.cart-toast');
    if (!t) { t = el('div', 'cart-toast'); t.setAttribute('role', 'status'); document.body.append(t); }
    t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on');
    clearTimeout(toast.t); toast.t = setTimeout(() => t.classList.remove('on'), bad ? 5000 : 3200);
  };
  async function copy(link, said) {
    try { await navigator.clipboard.writeText(link); toast(said || 'Link copied.'); }
    catch (_) { window.prompt('Copy This Link', link); }
  }
  const touch = () => matchMedia('(pointer: coarse)').matches;

  function open({ ref, url, title }) {
    const link = new URL(url || location.pathname, location.origin).href;
    const name = (title || document.title || '').trim();
    document.querySelector('dialog.sh')?.remove();
    const d = el('dialog', 'bt-dlg bt-fit sh');
    const head = el('header', 'bt-dlg-h'); const h = el('h2', '', 'Share'); h.id = 'shTitle'; d.setAttribute('aria-labelledby', h.id);
    const x = el('button', 'bt-x'); x.type = 'button'; x.setAttribute('aria-label', 'Close'); x.innerHTML = svg('x');
    head.append(h, x);
    const body = el('div', 'bt-dlg-b');
    if (name) body.append(el('p', 'sh-what', name));
    const list = el('div', 'sh-list');
    const row = (icon, words, fn, cls) => {
      const b = el('button', `sh-row${cls ? ` ${cls}` : ''}`); b.type = 'button';
      const i = el('span', 'sh-ic'); i.innerHTML = svg(icon);
      b.append(i, el('span', 'sh-t', words));
      b.addEventListener('click', fn); list.append(b); return b;
    };
    const back = document.activeElement;
    const close = () => { if (d.open) d.close(); };
    d.addEventListener('close', () => { d.remove(); document.documentElement.classList.remove('bt-modal'); if (back && back.isConnected) back.focus({ preventScroll: true }); });
    x.addEventListener('click', close);
    d.addEventListener('mousedown', (e) => { if (e.target === d) close(); });

    if (ref) row('chat', 'Send In A Message', () => {
      close();
      if (window.bbvDM && window.bbvDM.share) window.bbvDM.share(ref); else location.href = '/login';
    }, 'sh-main');
    if (navigator.share && touch()) {
      // A phone: its own share menu lists Messenger, Viber and whatever else is on it.
      row('bolt', 'Messenger And Other Apps', async () => {
        try { await navigator.share({ title: name, url: link }); close(); }
        catch (err) { if (!err || err.name !== 'AbortError') copy(link, 'The link is copied: paste it in a chat.'); }
      });
    } else if (touch()) {
      // A phone without a share menu (inside another app's browser): Messenger itself, or the link if it doesn't open.
      row('bolt', 'Messenger', () => {
        const wait = setTimeout(() => { if (!document.hidden) copy(link, 'Messenger didn’t open. The link is copied: paste it in a chat.'); }, 1600);
        document.addEventListener('visibilitychange', () => clearTimeout(wait), { once: true });
        location.href = `fb-messenger://share/?link=${encodeURIComponent(link)}`;
        close();
      });
    } else {
      // A computer: Messenger has no "share this link" address of its own, so the link is copied and Messenger opens.
      row('bolt', 'Messenger', async () => { await copy(link, 'Link copied. Paste it in a Messenger chat.'); window.open('https://www.messenger.com/', '_blank', 'noopener'); close(); });
    }
    row('fb', 'Facebook', () => { window.open(`https://www.facebook.com/sharer/sharer.php?u=${encodeURIComponent(link)}`, '_blank', 'noopener'); close(); });
    row('link', 'Copy Link', async () => { await copy(link); close(); });
    body.append(list);
    d.append(head, body);
    document.body.append(d);
    d.showModal(); document.documentElement.classList.add('bt-modal');
  }

  document.addEventListener('click', (e) => {
    const b = e.target.closest && e.target.closest('[data-share-ref]'); if (!b) return;
    e.preventDefault(); e.stopPropagation();
    open({ ref: b.dataset.shareRef, url: b.dataset.shareUrl, title: b.dataset.shareTitle });
  });
  window.bbvShare = { open };
})();
