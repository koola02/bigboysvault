// The vault chat, live for everyone, and Ask the Vault. Both work on their own page and in the floating window.
(() => {
  const make = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const jsonPost = async (url, body) => {
    const r = await fetch(url, { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json', Accept: 'application/json' }, body: JSON.stringify(body) });
    const j = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(j.error || 'That didn\'t work. Please try again.');
    return j;
  };
  const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));
  const visible = () => document.hidden ? new Promise((ok) => document.addEventListener('visibilitychange', ok, { once: true })) : null;
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  // Who's here is asked once per page and shared (see fb-shell.js).
  const bbvWho = () => (window.bbvWho ? window.bbvWho() : (window.bbvWhoAnswer ||= fetch('/me/who', { credentials: 'same-origin', headers: { Accept: 'application/json' } }).then((r) => (r.ok ? r.json() : {})).catch(() => ({}))));

  /* ---------- the chat ---------- */
  async function chat(root) {
    const log = root.querySelector('[data-log]'), list = root.querySelector('[data-list]'), form = root.querySelector('[data-form]');
    const text = root.querySelector('[data-text]'), note = root.querySelector('[data-note]'), join = root.querySelector('[data-join]');
    const newer = root.querySelector('[data-new]'), older = root.querySelector('[data-older]'), online = root.querySelector('[data-online]');
    const windowId = Math.random().toString(36).slice(2, 14);
    let who = { admin: false, full: false, member: null }, first = 0, last = 0, version = 0;

    const nearEnd = () => log.scrollHeight - log.scrollTop - log.clientHeight < 90;
    const toEnd = (smooth) => log.scrollTo({ top: log.scrollHeight, behavior: smooth && !still ? 'smooth' : 'auto' });
    const clock = (at) => {
      const d = new Date(at * 1000), now = new Date();
      const time = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
      return d.toDateString() === now.toDateString() ? time : `${d.toLocaleDateString([], { month: 'short', day: 'numeric' })}, ${time}`;
    };
    const say = (msg) => { note.textContent = msg || ''; note.hidden = !msg; };

    function bubble(m) {
      const mine = who.member?.id === m.member;
      const n = make('article', `msg${mine ? ' mine' : ''}`);
      n.dataset.id = m.id; n.dataset.member = m.member; n.dataset.at = m.at; n.dataset.role = m.role;
      const face = make('a', 'msg-face'); face.href = `/trainers/${m.member}`; face.setAttribute('aria-label', m.name);
      if (m.avatar) { const i = make('img', 'av'); i.src = m.avatar; i.width = 56; i.height = 56; i.alt = ''; i.loading = 'lazy'; face.append(i); } else face.append(make('span', 'av', (m.name[0] || '?').toUpperCase()));
      const body = make('div', 'msg-body');
      const head = make('header', 'msg-head');
      const name = make('a', 'msg-name', m.name); name.href = `/trainers/${m.member}`;
      head.append(name);
      if (m.role) { const r = make('span', 'role', m.role === 'admin' ? 'Admin' : 'Moderator'); r.dataset.role = m.role; head.append(r); }
      (m.tags || []).forEach((g) => head.append(make('span', 'tag-hl mtag', g)));
      const t = make('time', 'msg-time', clock(m.at)); head.append(t);
      const words = make('p', 'msg-text', m.text);
      body.append(head, words);
      if (mine || who.full || (who.admin && !m.role)) {
        const x = make('button', mine ? 'msg-x' : 'msg-x staff-only', '✕'); x.type = 'button'; x.setAttribute('aria-label', 'Delete this message');
        x.addEventListener('click', async () => {
          if (!(await (window.bbvConfirm ? window.bbvConfirm('Delete this message?', { yes: 'Delete', no: 'Keep it' }) : Promise.resolve(confirm('Delete this message?'))))) return;
          try { await jsonPost(`/chat/messages/${m.id}/delete`, {}); n.remove(); tidy(); } catch (e) { say(e.message); }
        });
        body.append(x);
      }
      n.append(face, body);
      return n;
    }
    // Messages in a row from one trainer sit together under one name.
    function tidy() {
      let prev = null;
      list.querySelectorAll('.msg').forEach((n) => {
        n.classList.toggle('cont', !!prev && prev.dataset.member === n.dataset.member && n.dataset.at - prev.dataset.at < 300);
        prev = n;
      });
      if (!list.querySelector('.msg')) { if (!list.querySelector('.chat-empty')) list.append(make('p', 'chat-empty', 'Nobody has said anything yet. Say hello.')); } else list.querySelector('.chat-empty')?.remove();
    }
    function apply(j, prepend) {
      const stick = nearEnd();
      const had = log.scrollHeight;
      const fresh = (j.messages || []).filter((m) => !list.querySelector(`[data-id="${m.id}"]`));
      if (prepend) list.prepend(...fresh.map(bubble));
      else fresh.forEach((m) => { list.append(bubble(m)); });
      fresh.forEach((m) => { first = first ? Math.min(first, m.id) : m.id; last = Math.max(last, m.id); });
      (j.gone || []).forEach((id) => list.querySelector(`[data-id="${id}"]`)?.remove());
      tidy();
      if (prepend) log.scrollTop = log.scrollHeight - had;
      else if (fresh.length) { if (stick || fresh.some((m) => m.member === who.member?.id)) toEnd(true); else newer.hidden = false; }
      if (j.here != null) online.textContent = j.here === 1 ? 'Just you here now' : `${j.here} here now`;
      if (j.v) version = j.v;
    }

    try { const w = await bbvWho(); if (w && 'member' in w) who = w; } catch (_) {}
    if (who.member) {
      form.hidden = false;
      if (who.member.locked) { form.hidden = true; say(`Your trainer is locked for another ${who.member.lock_left}.`); }
    } else join.hidden = false;

    const firstLoad = await fetch(`/chat/messages?c=${windowId}`, { credentials: 'same-origin' }).then((r) => r.json()).catch(() => null);
    if (firstLoad) {
      apply(firstLoad);
      older.hidden = (firstLoad.messages || []).length < 60;
      toEnd(false);
    }

    log.addEventListener('scroll', () => { if (nearEnd()) newer.hidden = true; }, { passive: true });
    newer.addEventListener('click', () => { newer.hidden = true; toEnd(true); });
    older.addEventListener('click', async () => {
      older.disabled = true;
      try {
        const j = await (await fetch(`/chat/messages?before=${first}`, { credentials: 'same-origin' })).json();
        apply({ messages: j.messages }, true);
        older.hidden = (j.messages || []).length < 60;
      } catch (_) {}
      older.disabled = false;
    });

    const grow = () => { text.style.height = 'auto'; text.style.height = `${Math.min(text.scrollHeight, 132)}px`; };
    text.addEventListener('input', grow);
    text.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.shiftKey && !e.isComposing && matchMedia('(pointer: fine)').matches) { e.preventDefault(); form.requestSubmit(); }
    });
    form.addEventListener('submit', async (e) => {
      e.preventDefault();
      const words = text.value.trim();
      if (!words) return;
      const btn = form.querySelector('button');
      btn.disabled = true; say('');
      try { await jsonPost('/chat/messages', { text: words }); text.value = ''; grow(); text.focus({ preventScroll: true }); } catch (err) { say(err.message); }
      btn.disabled = false;
    });

    // Wait for news. The server answers the moment anything changes.
    let wait = 0;
    while (root.isConnected) {
      const hidden = visible();
      if (hidden) await hidden;
      try {
        const r = await fetch(`/chat/messages?after=${last}&v=${version}&c=${windowId}`, { credentials: 'same-origin' });
        const j = await r.json();
        if (!root.isConnected) return;
        apply(j);
        wait = j.retry || 0;
      } catch (_) { wait = Math.min(15000, (wait || 1000) * 2); }
      if (wait) await sleep(wait);
    }
  }

  /* ---------- Ask the Vault ---------- */
  // A card from any seller. The vault's own cards have a handle (and, while for sale, `id` and `amount`); a store's card
  // has its listing number `lid`, its page `url` and its store (`seller`...), and only comes with a price. `me` is the
  // visitor's member number: a seller gets no "+" or Message on their own store's cards.
  function tile(c, me) {
    const store = c.lid > 0;
    const a = make('a', 'card'); a.href = store ? c.url : `/products/${c.handle}`; a.dataset.foil = c.foil; a.dataset.c = c.c;
    if (store) a.dataset.store = c.seller_id;
    const box = make('div', 'card-img');
    if (c.img) { const i = make('img'); i.src = c.img; i.alt = c.full; i.loading = 'lazy'; i.decoding = 'async'; box.append(i); }
    if (c.grade) box.append(make('span', 'grade', c.grade));
    a.append(box, make('div', 'card-t', c.title));
    const meta = make('div', 'card-m');
    if (c.set) { const s = make('span', 'card-set'); if (c.sym) { const y = make('img', 'card-sym'); y.src = c.sym; y.width = 16; y.height = 16; y.alt = ''; s.append(y); } s.append(c.set); meta.append(s); }
    if (c.cond) { const p = make('span', 'card-cond', c.cond); p.dataset.c = c.c; meta.append(p); }
    a.append(meta);
    if (c.tags?.length) { const t = make('div', 'card-tags'); c.tags.forEach((x) => t.append(make('span', 'tag-hl', x))); a.append(t); }
    if (c.notes) a.append(make('div', 'card-note', c.notes));
    const p = make('div', 'card-p', c.price);
    // The "+" only shows when the server sent the card's own number and real price: that means it is still for sale.
    const mini = () => {
      const d = make('span', 'dm-mini'); d.setAttribute('role', 'button'); d.tabIndex = 0; d.title = 'Message the seller'; d.setAttribute('aria-label', `Message the seller about ${c.title}`);
      const b = make('span', 'cart-mini', '+');
      b.setAttribute('role', 'button'); b.tabIndex = 0; b.title = 'Add to cart'; b.setAttribute('aria-label', `Add ${c.title} to cart`);
      p.append(d, b);
      return [d, b];
    };
    if (store) {
      if (c.amount > 0 && c.seller_id !== me) {
        const [d, b] = mini();
        d.dataset.dmCard = c.lid;
        Object.assign(b.dataset, { cartCard: c.lid, title: c.title, price: c.amount, image: c.img || '', url: c.url, seller: c.seller || '', sellerSlug: c.seller_slug || '' });
      }
    } else if (c.id && c.amount > 0) {
      const [d, b] = mini();
      d.dataset.dmVault = c.id;
      Object.assign(b.dataset, { cartVault: c.id, title: c.title, price: c.amount, image: c.img || '', url: `/products/${c.handle}` });
    }
    a.append(p);
    // Whose card it is, the way The Vault page says it: the store's picture (or its first letter) and name.
    if (store) {
      const by = make('div', 'card-by');
      if (c.seller_img) { const i = make('img', 'card-by-av'); i.src = c.seller_img; i.width = 16; i.height = 16; i.alt = ''; i.loading = 'lazy'; i.decoding = 'async'; by.append(i); } else { const l = make('span', 'card-by-av', ([...(c.seller || '?').trim()][0] || '?').toUpperCase()); l.setAttribute('aria-hidden', 'true'); by.append(l); }
      by.append(make('span', 'card-by-t', c.seller || ''));
      a.append(by);
    }
    return a;
  }

  function ask(root) {
    const log = root.querySelector('[data-log]'), chips = root.querySelector('[data-chips]'), form = root.querySelector('[data-form]'), box = root.querySelector('[data-text]');
    let last_ctx = null;
    const toEnd = () => log.scrollTo({ top: log.scrollHeight, behavior: still ? 'auto' : 'smooth' });
    const typeOut = async (node, words) => {
      if (still) { node.textContent = words; return; }
      const parts = words.split(' ');
      for (let i = 1; i <= parts.length && root.isConnected; i++) {
        node.textContent = parts.slice(0, i).join(' ');
        if (i % 3 === 0) { toEnd(); await sleep(26); }
      }
      node.textContent = words;
    };
    async function send(q, quiet) {
      if (q && !quiet) log.append(make('p', 'bub me-said', q));
      chips.replaceChildren();
      const dots = make('p', 'bub vault-said typing'); dots.append(make('i'), make('i'), make('i'));
      dots.setAttribute('aria-label', 'Thinking');
      log.append(dots); toEnd();
      let j;
      const payload = { q };
      if (last_ctx) payload.ctx = last_ctx;
      try { j = await jsonPost('/api/ask', payload); } catch (_) { j = { say: 'I lost my connection for a moment. Please ask again.', cards: [], chips: [] }; }
      dots.remove();
      const said = make('p', 'bub vault-said');
      log.append(said);
      await typeOut(said, j.say);
      if (j.link) {
        const a = make('a', 'btn ask-link', j.link.text); a.href = j.link.href;
        if (j.link.href === '/chat') a.dataset.float = '';
        log.append(a);
      }
      if (j.cards?.length) {
        let me = 0;
        try { me = (await bbvWho())?.member?.id || 0; } catch (_) {}
        const grid = make('div', 'grid ask-cards');
        j.cards.forEach((c) => grid.append(tile(c, me)));
        log.append(grid);
      }
      if (j.ctx) last_ctx = j.ctx;
      (j.chips || []).forEach((c) => {
        const b = make('button', 'ask-chip', c); b.type = 'button';
        b.addEventListener('click', () => send(c));
        chips.append(b);
      });
      toEnd();
    }
    form.addEventListener('submit', (e) => {
      e.preventDefault();
      const q = box.value.trim();
      if (!q) return;
      box.value = '';
      send(q);
    });
    send('', true);
    if (matchMedia('(pointer: fine)').matches) box.focus({ preventScroll: true });
  }

  const start = () => document.querySelectorAll('[data-chat],[data-ask]').forEach((n) => {
    if (n.dataset.on) return;
    n.dataset.on = '1';
    if (n.matches('[data-chat]')) chat(n); else ask(n);
  });
  start();
  document.addEventListener('float:filled', start);
})();
