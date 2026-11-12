/* Private messages, Messenger style (server: src/messages.rs).
   - A "Message" button beside every add-to-cart button opens a chat with that card's seller (or the vault), with the
     card attached to the first message.
   - One chat window: docked bottom-right on a computer, full screen on a phone; it comes back after moving to another
     page. Text, photos, voice notes and voice calls (WebRTC; Cloudflare's relay when needed).
   - One long request per page (GET /dm/poll) brings everything live: new messages, "Seen", the unread badge, calls.
   - /me/messages lists the conversations; /admin/messages lets admins read any of them without marking it seen.
   - In the new look the header's Messages button drops the inbox down beside it (a phone opens the page).
   - With alerts on (static/alerts.js, static/sw.js), a call rings the phone even with the site closed; its alert
     opens this page on the call, answered at once for "Answer". */
(() => {
  const $ = (s, r = document) => r.querySelector(s);
  const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const icon = (d, extra = '') => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"${extra}>${d}</svg>`;
  const I = {
    chat: '<path d="M21 11.5a8.4 8.4 0 0 1-9 8.3 9 9 0 0 1-3.8-.8L3 20l1.2-4.3A8 8 0 0 1 3 11.5 8.4 8.4 0 0 1 12 3.2a8.4 8.4 0 0 1 9 8.3z"/>',
    phone: '<path d="M22 16.9v3a2 2 0 0 1-2.2 2 19.8 19.8 0 0 1-8.6-3.1 19.5 19.5 0 0 1-6-6A19.8 19.8 0 0 1 2.1 4.2 2 2 0 0 1 4.1 2h3a2 2 0 0 1 2 1.7c.1.9.3 1.8.6 2.7a2 2 0 0 1-.5 2.1L8 9.8a16 16 0 0 0 6 6l1.3-1.3a2 2 0 0 1 2.1-.4c.9.3 1.8.5 2.7.6a2 2 0 0 1 1.7 2z"/>',
    close: '<path d="M18 6L6 18M6 6l12 12"/>',
    min: '<path d="M5 12h14"/>',
    back: '<path d="M15 18l-6-6 6-6"/>',
    photo: '<rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/>',
    mic: '<rect x="9" y="2" width="6" height="12" rx="3"/><path d="M5 10a7 7 0 0 0 14 0M12 17v5"/>',
    cam: '<path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/>',
    send: '<path d="M4 12l16-8-6 16-3-7z"/>',
    play: '<path d="M7 4l13 8-13 8z" fill="currentColor"/>',
    pause: '<path d="M7 4h4v16H7zM13 4h4v16h-4z" fill="currentColor"/>',
    trash: '<path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14"/>',
    mute: '<path d="M1 1l22 22M9 9v3a3 3 0 0 0 5.1 2.1M15 9.3V4a3 3 0 0 0-5.9-.6M17 16.9A7 7 0 0 1 5 12v-2m14 0v2c0 .7-.1 1.4-.3 2M12 19v3"/>',
  };
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const phone = () => matchMedia('(max-width: 699px), (max-height: 520px)').matches;
  // The condition's colour, as on the site's cards.
  const condClass = (t) => { t = (t || '').toLowerCase(); return /psa|bgs|cgc|graded/.test(t) ? 'gr' : /near mint|^nm|mint/.test(t) ? 'nm' : /light|^lp/.test(t) ? 'lp' : /moderate|^mp/.test(t) ? 'mp' : /heav|^hp/.test(t) ? 'hp' : /damag|^dm/.test(t) ? 'dm' : ''; };
  const who = () => (window.bbvWho ? window.bbvWho() : fetch('/me/who', { credentials: 'same-origin' }).then((r) => r.json()).catch(() => ({})));
  const store = { get(k) { try { return JSON.parse(sessionStorage.getItem(k)); } catch (_) { return null; } }, set(k, v) { try { v == null ? sessionStorage.removeItem(k) : sessionStorage.setItem(k, JSON.stringify(v)); } catch (_) {} } };

  async function api(url, body, opts = {}) {
    const init = { method: body === undefined ? 'GET' : 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' } };
    if (body instanceof FormData || body instanceof Blob) { init.body = body; if (body instanceof Blob) init.headers['Content-Type'] = body.type || 'application/octet-stream'; }
    else if (body !== undefined) { init.headers['Content-Type'] = 'application/json'; init.body = JSON.stringify(body); }
    if (opts.signal) init.signal = opts.signal;
    const r = await fetch(url, init);
    const j = await r.json().catch(() => ({}));
    if (r.status === 401) { const e = new Error(j.error || 'Log in to send messages.'); e.login = true; throw e; }
    if (!r.ok) throw new Error(j.error || 'That didn\'t work. Please try again.');
    return j;
  }
  const toast = (msg, bad) => {
    let t = $('.cart-toast');
    if (!t) { t = el('div', 'cart-toast'); t.setAttribute('role', 'status'); document.body.append(t); }
    t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on');
    clearTimeout(toast.t); toast.t = setTimeout(() => t.classList.remove('on'), bad ? 5000 : 2600);
  };
  const toLogin = () => { try { sessionStorage.setItem('bbv:after-login', location.pathname + location.search); } catch (_) {} location.href = '/login'; };

  const clock = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
  const when = (t) => {
    const d = new Date(t * 1000), now = new Date();
    const same = d.toDateString() === now.toDateString();
    const time = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    if (same) return time;
    const days = (now - d) / 864e5;
    if (days < 6) return `${d.toLocaleDateString([], { weekday: 'short' })} ${time}`;
    return `${d.toLocaleDateString([], { month: 'short', day: 'numeric' })} ${time}`;
  };
  const ago = (t) => {
    const s = Math.max(0, Date.now() / 1000 - t);
    if (s < 60) return 'now'; if (s < 3600) return `${Math.floor(s / 60)}m`; if (s < 86400) return `${Math.floor(s / 3600)}h`;
    if (s < 7 * 86400) return `${Math.floor(s / 86400)}d`; return new Date(t * 1000).toLocaleDateString([], { month: 'short', day: 'numeric' });
  };
  function avatar(p, size = 36) {
    const a = el('span', 'dm-av'); a.style.setProperty('--s', `${size}px`);
    if (p && p.avatar) { const img = el('img'); img.src = p.avatar; img.alt = ''; img.loading = 'lazy'; a.append(img); } else a.textContent = (p && p.initial) || '?';
    if (p && p.online) a.classList.add('on');
    return a;
  }

  /* ---------------- the live connection ---------------- */
  // This page's own name: someone can have the site open in two tabs, or on a phone and a computer.
  const PID = `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 10)}`;
  // `s` is the number of the last call message this page has: every open page gets every one.
  const live = { v: 0, s: 0, me: null, unread: 0, running: false, ctrl: null, listeners: new Set() };
  async function startLive() {
    if (live.running) return; live.running = true;
    for (;;) {
      // A tab in the background keeps listening, so a call still rings there; it only stops reading the open chat
      // (reading it would say "Seen" for messages nobody has looked at).
      const w = win.thread && !win.watch && !document.hidden ? win.thread : null;
      const q = new URLSearchParams({ v: String(live.v), s: String(live.s) });
      if (w) { q.set('t', String(w)); q.set('after', String(win.last || 0)); }
      live.ctrl = new AbortController();
      const stop = setTimeout(() => live.ctrl.abort(), 40000);
      let j = null;
      try { j = await api(`/dm/poll?${q}`, undefined, { signal: live.ctrl.signal }); } catch (e) { if (e.login) { live.running = false; return; } }
      clearTimeout(stop);
      if (!j) { await new Promise((ok) => setTimeout(ok, live.kick ? 0 : 3000)); live.kick = false; continue; }
      live.v = j.v; if (j.s != null) live.s = j.s;
      setUnread(j.unread);
      for (const s of j.signals || []) call.signal(s);
      if (j.open && j.open.id === win.thread) win.take(j.open);
      live.listeners.forEach((f) => f(j));
      if (j.retry) await new Promise((ok) => setTimeout(ok, j.retry));
    }
  }
  // Something changed here (a message sent, a chat opened): ask again now instead of waiting out the poll.
  const kick = () => { live.kick = true; if (live.ctrl) live.ctrl.abort(); };

  function setUnread(n) {
    live.unread = n;
    const b = $('#dmBadge'); if (b) { b.hidden = !n; b.textContent = n > 99 ? '99+' : String(n); }
    const head = $('.dm-head-bubble'); if (head) head.dataset.unread = n ? String(n) : '';
  }

  /* ---------------- the chat window ---------------- */
  const win = {
    node: null, thread: null, last: 0, first: 0, other: null, me: null, card: null, watch: false, seen: 0, msgs: new Map(), more: false,
    build() {
      if (this.node) return this.node;
      const n = el('section', 'dm-win'); n.setAttribute('role', 'dialog'); n.setAttribute('aria-label', 'Chat');
      n.innerHTML = `
        <header class="dm-h">
          <button type="button" class="dm-ib dm-back" aria-label="Back">${icon(I.back)}</button>
          <a class="dm-who" data-who></a>
          <button type="button" class="dm-ib dm-call" aria-label="Voice call" title="Voice call">${icon(I.phone)}</button>
          <button type="button" class="dm-ib dm-min" aria-label="Minimise" title="Minimise">${icon(I.min)}</button>
          <button type="button" class="dm-ib dm-x" aria-label="Close" title="Close">${icon(I.close)}</button>
        </header>
        <div class="dm-log" role="log" aria-live="polite" tabindex="0"></div>
        <div class="dm-attach" hidden></div>
        <form class="dm-form">
          <label class="dm-ib dm-cam" title="Take a photo" aria-label="Take a photo">${icon(I.cam)}<input type="file" accept="image/*" capture="environment" data-card-cam="free" hidden></label>
          <label class="dm-ib dm-pic" title="Send a photo from your gallery" aria-label="Send a photo from your gallery">${icon(I.photo)}<input type="file" accept="image/*" hidden></label>
          <button type="button" class="dm-ib dm-rec" title="Voice message" aria-label="Record a voice message">${icon(I.mic)}</button>
          <textarea rows="1" maxlength="1000" placeholder="Aa" aria-label="Message" enterkeyhint="send"></textarea>
          <button type="submit" class="dm-ib dm-send" aria-label="Send">${icon(I.send)}</button>
        </form>
        <div class="dm-recbar" hidden>
          <button type="button" class="dm-ib dm-rec-x" aria-label="Cancel recording">${icon(I.trash)}</button>
          <span class="dm-rec-dot"></span><span class="dm-rec-t">0:00</span><span class="dm-rec-l">Recording…</span>
          <button type="button" class="dm-ib dm-send dm-rec-go" aria-label="Send voice message">${icon(I.send)}</button>
        </div>
        <p class="dm-ro" hidden>You're reading as an admin. The people in this chat can't see that you read it.</p>`;
      document.body.append(n);
      this.node = n;
      const log = $('.dm-log', n), form = $('.dm-form', n), box = $('textarea', n);
      $('.dm-x', n).addEventListener('click', () => this.close());
      $('.dm-back', n).addEventListener('click', () => this.close());
      $('.dm-min', n).addEventListener('click', () => this.minimise());
      $('.dm-call', n).addEventListener('click', () => call.start(this.thread, this.other));
      const grow = () => { box.style.height = 'auto'; box.style.height = `${Math.min(box.scrollHeight, 120)}px`; form.classList.toggle('has-text', !!box.value.trim()); };
      box.addEventListener('input', grow);
      box.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !e.shiftKey && matchMedia('(pointer: fine)').matches) { e.preventDefault(); form.requestSubmit(); } });
      form.addEventListener('submit', async (e) => {
        e.preventDefault();
        const text = box.value.trim(); const card = this.card;
        if (!text && !card) return;
        box.value = ''; grow();
        const body = { text }; if (card) body.card = card.src === 'vault' ? { vault: card.id } : { card: card.id };
        this.setCard(null);
        const temp = this.pending(text || '…');
        try { const j = await api(`/dm/${this.thread}/send`, body); temp.remove(); if (j.message) this.add([j.message]); kick(); }
        catch (err) { temp.remove(); box.value = text; grow(); if (card) this.setCard(card); toast(err.message, true); }
      });
      n.querySelectorAll('.dm-pic input, .dm-cam input').forEach((inp) => inp.addEventListener('change', async (e) => {
        const f = e.target.files[0]; e.target.value = ''; if (!f) return;
        const fd = new FormData(); fd.append('photo', f);
        const temp = this.pending('Sending photo…');
        try { const j = await api(`/dm/${this.thread}/photo`, fd); temp.remove(); if (j.message) this.add([j.message]); kick(); }
        catch (err) { temp.remove(); toast(err.message, true); }
      }));
      $('.dm-rec', n).addEventListener('click', () => rec.start());
      $('.dm-rec-x', n).addEventListener('click', () => rec.stop(false));
      $('.dm-rec-go', n).addEventListener('click', () => rec.stop(true));
      log.addEventListener('scroll', () => { if (log.scrollTop < 40 && this.more && !this.loading) this.older(); }, { passive: true });
      n.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !$('.dm-call-ui')) this.close(); });
      return n;
    },
    async open(tid, opts = {}) {
      const n = this.build();
      if (this.thread !== tid) { this.thread = tid; this.last = 0; this.first = 0; this.msgs.clear(); this.seen = 0; $('.dm-log', n).replaceChildren(); }
      this.watch = !!opts.watch;
      n.classList.toggle('watch', this.watch);
      $('.dm-form', n).hidden = this.watch; $('.dm-ro', n).hidden = !this.watch; $('.dm-call', n).hidden = this.watch;
      n.hidden = false; n.classList.remove('min'); document.documentElement.classList.toggle('dm-full', phone()); this.fitScreen?.();
      $('.dm-head-bubble')?.remove();
      if (opts.card !== undefined) this.setCard(opts.card);
      if (!this.watch) store.set('bbv:dm', { t: tid, min: false, m: live.me });
      try {
        const j = await api(this.watch ? `/admin/dm/${tid}` : `/dm/${tid}`);
        this.me = j.me; this.other = j.other; this.seen = j.seen || 0; this.more = !!j.more;
        this.header(j);
        this.add(j.messages, true);
        if (!this.watch) { this.read(); kick(); }
      } catch (err) { if (opts.quiet) { store.set('bbv:dm', null); this.close(); return; } toast(err.message, true); if (err.login) toLogin(); this.close(); return; }
      if (!phone() && !this.watch) $('textarea', n).focus({ preventScroll: true });
      if (this.watch) { clearInterval(this.watchTimer); this.watchTimer = setInterval(() => this.refreshWatch(), 8000); }
    },
    header(j) {
      const w = $('[data-who]', this.node); w.replaceChildren();
      if (this.watch && j.a) {
        w.append(avatar(j.a, 32), avatar(j.other, 32));
        const t = el('span', 'dm-who-t'); t.append(el('b', '', `${j.a.name} ↔ ${j.other.name}`), el('small', '', `${j.a.name} on the right · read only`));
        w.append(t); w.removeAttribute('href'); return;
      }
      const o = j.other; w.append(avatar(o, 36));
      const t = el('span', 'dm-who-t'); t.append(el('b', '', o.name), el('small', 'dm-status', ''));
      if (j.as_vault) t.querySelector('b').textContent = `${o.name} → Vault`;
      w.append(t);
      if (o.url) w.href = o.url; else w.removeAttribute('href');
      this.asVault = !!j.as_vault;
      this.status(o.online);
    },
    status(online) {
      const s = $('.dm-status', this.node);
      if (s && !this.watch) s.textContent = this.asVault ? 'You answer for the vault' : online ? 'Active now' : this.other && this.other.id === 0 ? 'The vault’s team' : 'Trainer';
      const a = $('[data-who] .dm-av', this.node); if (a) a.classList.toggle('on', !!online);
    },
    async refreshWatch() {
      if (!this.watch || this.node.hidden) { clearInterval(this.watchTimer); return; }
      try { const j = await api(`/admin/dm/${this.thread}`); this.add(j.messages.filter((m) => m.id > this.last)); } catch (_) {}
    },
    async older() {
      this.loading = true;
      try {
        const j = await api(`${this.watch ? '/admin/dm' : '/dm'}/${this.thread}?before=${this.first}`);
        const log = $('.dm-log', this.node), h = log.scrollHeight;
        this.more = !!j.more; this.add(j.messages, false, true);
        log.scrollTop = log.scrollHeight - h;
      } catch (_) {}
      this.loading = false;
    },
    take(o) { if (o.messages.length) this.add(o.messages); if (o.seen > this.seen) { this.seen = o.seen; this.paintSeen(); } this.status(o.online); },
    pending(text) { const b = el('div', 'dm-row mine pending'); b.append(el('div', 'dm-b', text)); $('.dm-log', this.node).append(b); this.bottom(); return b; },
    add(list, first, older) {
      const log = $('.dm-log', this.node);
      const atEnd = log.scrollHeight - log.scrollTop - log.clientHeight < 80;
      const fresh = list.filter((m) => !this.msgs.has(m.id));
      if (!fresh.length) {
        if (first && !this.msgs.size) log.replaceChildren(el('p', 'dm-empty', this.watch ? 'No messages yet.' : 'Say hi! Only you two (and the vault’s team) can read this chat.'));
        if (first) this.bottom(true);
        return;
      }
      fresh.forEach((m) => this.msgs.set(m.id, m));
      // Draw everything again in order: simple, and a chat is short.
      const all = [...this.msgs.values()].sort((a, b) => a.id - b.id);
      this.first = all[0].id; this.last = all[all.length - 1].id;
      log.replaceChildren();
      if (this.more) log.append(el('p', 'dm-more', 'Scroll up for older messages'));
      let prev = null;
      all.forEach((m, i) => {
        const next = all[i + 1];
        if (!prev || m.at - prev.at > 900) log.append(el('p', 'dm-time', when(m.at)));
        const groupEnd = !next || next.side !== m.side || next.at - m.at > 900 || next.kind === 'call';
        log.append(this.bubble(m, groupEnd));
        prev = m;
      });
      this.paintSeen();
      if (first || (!older && (atEnd || fresh.some((m) => m.mine)))) this.bottom(first);
      if (!older && !first && !this.watch && fresh.some((m) => !m.mine) && !document.hidden && !this.node.classList.contains('min')) this.read();
    },
    bubble(m, groupEnd) {
      const row = el('div', `dm-row ${m.mine ? 'mine' : 'theirs'}`); row.dataset.id = m.id;
      if (m.kind === 'call') {
        row.className = 'dm-row dm-callrow';
        const p = el('div', 'dm-callpill'); p.innerHTML = icon(I.phone);
        p.append(el('span', '', m.text));
        if (!this.watch) { const b = el('button', 'dm-callback', 'Call back'); b.type = 'button'; b.addEventListener('click', () => call.start(this.thread, this.other)); p.append(b); }
        row.append(p); return row;
      }
      if (!m.mine) row.append(groupEnd ? avatar(this.other, 28) : el('span', 'dm-av-gap'));
      const b = el('div', `dm-b k-${m.kind}`);
      b.title = when(m.at);
      if (m.kind === 'card' && m.card) b.append(cardView(m.card));
      if (m.kind === 'photo') {
        const a = el('a', 'dm-photo'); a.href = m.media; a.target = '_blank'; a.rel = 'noopener';
        const img = el('img'); img.src = m.thumb || m.media; img.alt = 'Photo'; img.decoding = 'async'; a.append(img);
        img.addEventListener('load', () => { const log = this.node && this.node.querySelector('.dm-log'); if (log && log.scrollHeight - log.scrollTop - log.clientHeight < 260) log.scrollTop = log.scrollHeight; }, { once: true });
        a.addEventListener('click', (e) => { e.preventDefault(); lightbox(m.media); });
        b.append(a);
      }
      if (m.kind === 'voice') b.append(voiceView(m));
      if (m.text) b.append(el('p', 'dm-text', m.text));
      row.append(b);
      return row;
    },
    paintSeen() {
      const log = $('.dm-log', this.node); if (!log) return;
      log.querySelectorAll('.dm-seen').forEach((x) => x.remove());
      const mine = [...log.querySelectorAll('.dm-row.mine:not(.pending)')];
      const last = mine[mine.length - 1]; if (!last) return;
      const id = Number(last.dataset.id);
      const s = el('p', 'dm-seen', this.seen >= id ? 'Seen' : 'Sent');
      last.after(s);
    },
    bottom(now) { const log = $('.dm-log', this.node); requestAnimationFrame(() => log.scrollTo({ top: log.scrollHeight, behavior: now || still ? 'auto' : 'smooth' })); },
    async read() { if (!this.thread || this.watch || !this.last) return; try { await api(`/dm/${this.thread}/read`, { upto: this.last }); } catch (_) {} },
    setCard(card) {
      this.card = card || null;
      const box = $('.dm-attach', this.node); box.replaceChildren(); box.hidden = !card;
      if (!card) { const ta = $('textarea', this.node); ta.placeholder = 'Aa'; $('.dm-form', this.node).classList.toggle('has-text', !!ta.value.trim()); return; }
      box.append(cardView(card, true));
      const x = el('button', 'dm-ib dm-attach-x'); x.type = 'button'; x.innerHTML = icon(I.close); x.setAttribute('aria-label', 'Don\'t attach this card');
      x.addEventListener('click', () => this.setCard(null));
      box.append(x);
      $('.dm-form', this.node).classList.add('has-text');
      $('textarea', this.node).placeholder = 'Ask about this card…';
    },
    minimise() {
      if (!this.node) return;
      this.node.classList.add('min'); this.node.hidden = true; document.documentElement.classList.remove('dm-full');
      store.set('bbv:dm', { t: this.thread, min: true, m: live.me });
      chatHead(this.other);
    },
    close() {
      if (!this.node) return;
      rec.stop(false);
      this.node.hidden = true; document.documentElement.classList.remove('dm-full');
      clearInterval(this.watchTimer);
      if (!this.watch) store.set('bbv:dm', null);
      this.thread = null; this.msgs.clear();
      $('.dm-head-bubble')?.remove();
      kick();
    },
  };

  function chatHead(other) {
    $('.dm-head-bubble')?.remove();
    const b = el('button', 'dm-head-bubble'); b.type = 'button'; b.setAttribute('aria-label', `Open chat with ${other ? other.name : 'chat'}`);
    b.append(avatar(other, 52));
    b.addEventListener('click', () => { const t = win.thread; b.remove(); if (t) win.open(t); });
    if (live.unread) b.dataset.unread = String(live.unread);
    document.body.append(b);
  }

  function cardView(c, small) {
    const a = el('a', `dm-card${small ? ' small' : ''}`); a.href = c.url || '#';
    if (c.image) { const img = el('img'); img.src = c.image; img.alt = ''; img.loading = 'lazy'; a.append(img); }
    const t = el('span', 'dm-card-t');
    t.append(el('b', '', c.title || 'Card'));
    const meta = [c.set, c.number].filter(Boolean).join(' · ');
    if (meta) t.append(el('small', '', meta));
    const row = el('span', 'dm-card-r');
    if (c.cond) { const k = el('span', 'dm-card-c', c.cond); const cc = condClass(c.cond); if (cc) k.dataset.c = cc; row.append(k); }
    if (c.price) row.append(el('span', 'dm-card-p', c.price));
    if (c.sold) row.append(el('span', 'dm-card-c sold', 'Sold'));
    t.append(row);
    if (!small) t.append(el('span', 'dm-card-go', 'View card ›'));
    a.append(t);
    return a;
  }

  function voiceView(m) {
    const w = el('div', 'dm-voice');
    const b = el('button', 'dm-ib dm-play'); b.type = 'button'; b.innerHTML = icon(I.play); b.setAttribute('aria-label', 'Play voice message');
    const bar = el('span', 'dm-vbar'); const fill = el('i'); bar.append(fill);
    const t = el('span', 'dm-vt', clock(m.secs || 0));
    let audio = null;
    b.addEventListener('click', () => {
      if (!audio) {
        audio = new Audio(m.media); audio.preload = 'auto';
        audio.addEventListener('timeupdate', () => { const d = audio.duration && isFinite(audio.duration) ? audio.duration : m.secs || 1; fill.style.width = `${Math.min(100, (audio.currentTime / d) * 100)}%`; t.textContent = clock(Math.max(0, d - audio.currentTime)); });
        audio.addEventListener('ended', () => { b.innerHTML = icon(I.play); fill.style.width = '0%'; t.textContent = clock(m.secs || 0); });
        audio.addEventListener('pause', () => { b.innerHTML = icon(I.play); });
        audio.addEventListener('play', () => { b.innerHTML = icon(I.pause); });
      }
      if (audio.paused) { if (voiceView.now && voiceView.now !== audio) voiceView.now.pause(); voiceView.now = audio; audio.play().catch(() => toast('That voice message couldn\'t play here.', true)); } else audio.pause();
    });
    w.append(b, bar, t);
    return w;
  }

  function lightbox(src) {
    const o = el('div', 'dm-lightbox'); o.setAttribute('role', 'dialog'); o.setAttribute('aria-label', 'Photo');
    const img = el('img'); img.src = src; img.alt = 'Photo'; o.append(img);
    const x = el('button', 'dm-ib dm-lb-x'); x.type = 'button'; x.innerHTML = icon(I.close); x.setAttribute('aria-label', 'Close'); o.append(x);
    const close = () => o.remove();
    o.addEventListener('click', (e) => { if (e.target === o || e.target.closest('.dm-lb-x')) close(); });
    document.addEventListener('keydown', function esc(e) { if (e.key === 'Escape') { close(); document.removeEventListener('keydown', esc); } });
    document.body.append(o); x.focus();
  }

  /* ---------------- voice notes ---------------- */
  const rec = {
    mr: null, chunks: [], t0: 0, timer: null, stream: null,
    async start() {
      if (!navigator.mediaDevices || !window.MediaRecorder) { toast('This browser can\'t record voice messages.', true); return; }
      try { this.stream = await navigator.mediaDevices.getUserMedia({ audio: true }); }
      catch (_) { toast('Allow the microphone to record a voice message.', true); return; }
      const types = ['audio/webm;codecs=opus', 'audio/ogg;codecs=opus', 'audio/mp4', 'audio/webm'];
      const type = types.find((x) => MediaRecorder.isTypeSupported && MediaRecorder.isTypeSupported(x));
      this.mr = new MediaRecorder(this.stream, type ? { mimeType: type, audioBitsPerSecond: 32000 } : undefined);
      this.chunks = []; this.mr.ondataavailable = (e) => { if (e.data.size) this.chunks.push(e.data); };
      this.mr.start(250); this.t0 = Date.now();
      const n = win.node; $('.dm-form', n).hidden = true; $('.dm-recbar', n).hidden = false;
      const t = $('.dm-rec-t', n);
      this.timer = setInterval(() => { const s = (Date.now() - this.t0) / 1000; t.textContent = clock(s); if (s >= 180) this.stop(true); }, 250);
    },
    stop(send) {
      if (!this.mr) return;
      const mr = this.mr, secs = Math.round((Date.now() - this.t0) / 1000);
      this.mr = null; clearInterval(this.timer);
      const n = win.node; if (n) { $('.dm-form', n).hidden = win.watch; $('.dm-recbar', n).hidden = true; }
      mr.onstop = async () => {
        this.stream.getTracks().forEach((x) => x.stop());
        if (!send || secs < 1) return;
        const blob = new Blob(this.chunks, { type: (mr.mimeType || 'audio/webm').split(';')[0] });
        const temp = win.pending('Sending voice message…');
        try { const j = await api(`/dm/${win.thread}/voice?secs=${secs}`, blob); temp.remove(); if (j.message) win.add([j.message]); kick(); }
        catch (err) { temp.remove(); toast(err.message, true); }
      };
      mr.stop();
    },
  };

  /* ---------------- voice calls ---------------- */
  const call = {
    pc: null, stream: null, id: null, thread: null, other: null, role: null, ui: null, pendingIce: [], earlyIce: [], ringTimer: null, tick: null, tone: null, offer: null,
    // Goes up each time a call is put away, so the steps of one that was put down while getting ready stop there.
    gen: 0,
    // Where the call is (calling, ringing, connecting, live), whether the line dropped, whether the microphone is off.
    phase: 'calling', weak: false, muted: false,
    // The call an alert was tapped for ({ call, answer }): it shows (and is answered) as soon as its ring arrives.
    want: null,
    // How the two browsers find each other. Asked with a time limit, so a slow answer never holds up a call.
    async iceServers() {
      const c = new AbortController(), t = setTimeout(() => c.abort(), 6000);
      try { const j = await api('/dm/ice', undefined, { signal: c.signal }); return j.iceServers || []; }
      catch (_) { return [{ urls: ['stun:stun.cloudflare.com:3478', 'stun:stun.l.google.com:19302'] }]; }
      finally { clearTimeout(t); }
    },
    async media() { return navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true } }); },
    async peer() {
      const pc = new RTCPeerConnection({ iceServers: await this.iceServers() });
      // A connection left over from a call that was put down while it got ready speaks for nobody.
      pc.onicecandidate = (e) => { if (!e.candidate || pc !== this.pc) return; if (this.id) this.send('ice', e.candidate.toJSON()); else this.earlyIce.push(e.candidate.toJSON()); };
      pc.ontrack = (e) => { if (pc !== this.pc) return; const a = this.audio(); a.srcObject = e.streams[0]; a.play().catch(() => this.needTap(a)); };
      pc.onconnectionstatechange = () => {
        if (pc !== this.pc) return;
        if (pc.connectionState === 'connected') { this.weak = false; this.connected(); }
        if (pc.connectionState === 'disconnected') this.weak = true;
        if (pc.connectionState === 'failed') { toast('The call couldn\'t connect.', true); this.hang(); }
      };
      // Some phone browsers only report the lower-level state: it counts too.
      pc.oniceconnectionstatechange = () => {
        if (pc !== this.pc) return;
        const st = pc.iceConnectionState;
        if (st === 'connected' || st === 'completed') { this.weak = false; this.connected(); }
        if (st === 'disconnected') this.weak = true;
        if (st === 'failed' && !('connectionState' in pc)) { toast('The call couldn\'t connect.', true); this.hang(); }
      };
      return pc;
    },
    audio() { let a = $('#dmCallAudio'); if (!a) { a = el('audio'); a.id = 'dmCallAudio'; a.autoplay = true; a.playsInline = true; document.body.append(a); } return a; },
    send(type, data) { return api(`/dm/${this.thread}/call`, { type, call: this.id || '', page: PID, data: data ?? null }).catch(() => {}); },
    async start(thread, other) {
      if (this.pc || this.ui) { toast('You\'re already in a call.', true); return; }
      if (!window.RTCPeerConnection) { toast('This browser can\'t make calls.', true); return; }
      this.thread = thread; this.other = other; this.role = 'caller'; this.id = null; this.earlyIce = []; this.pendingIce = [];
      this.show('Calling…', false);
      // Getting the microphone and the connection ready takes a moment, and the call can be put down meanwhile
      // (End call, or the other side's answer to an earlier one): then whatever was opened is closed again here.
      const my = this.gen, over = () => my !== this.gen;
      let stream = null, pc = null;
      const drop = () => { pc?.close(); stream?.getTracks().forEach((t) => t.stop()); };
      try { stream = await this.media(); } catch (_) { if (!over()) { toast('Allow the microphone to call.', true); this.clean(); } return; }
      if (over()) { drop(); return; }
      this.stream = stream;
      stream.getAudioTracks().forEach((t) => { t.enabled = !this.muted; });
      try {
        pc = await this.peer();
        if (over()) { drop(); return; }
        this.pc = pc;
        stream.getTracks().forEach((t) => pc.addTrack(t, stream));
        await pc.setLocalDescription(await pc.createOffer());
      } catch (_) { if (over()) drop(); else { toast('The call couldn\'t start. Please try again.', true); this.clean(); } return; }
      if (over()) { drop(); return; }
      let rung;
      try { rung = await api(`/dm/${thread}/call`, { type: 'ring', page: PID, data: { sdp: pc.localDescription } }); }
      catch (err) { if (!over()) { toast(err.message, true); this.clean(); } return; }
      // Put down while it was ringing out: the other side stops ringing at once.
      if (over()) { api(`/dm/${thread}/call`, { type: 'end', call: rung.call, page: PID, data: null }).catch(() => {}); return; }
      this.id = rung.call;
      this.earlyIce.splice(0).forEach((c) => this.send('ice', c));
      // "Calling…" stays until their page (or phone) says it is ringing. Nobody on the site: they'll find a missed call.
      if (rung.online === false && !rung.push && this.phase === 'calling') this.setStatus('Not on the site right now. Ringing in case they open it…');
      this.ringTimer = setTimeout(() => { if (this.role === 'caller' && !this.started) { toast('No answer.'); this.hang(); } }, 45000);
    },
    async accept() {
      this.ringing(false);
      this.phase = 'connecting'; this.setStatus('Connecting…'); this.buttons(false);
      // As when calling: if the call ends while this is getting ready, what was opened is closed again.
      const my = this.gen, over = () => my !== this.gen;
      let stream = null, pc = null;
      const drop = () => { pc?.close(); stream?.getTracks().forEach((t) => t.stop()); };
      try { stream = await this.media(); } catch (_) { if (!over()) { toast('Allow the microphone to answer.', true); this.decline(); } return; }
      if (over()) { drop(); return; }
      this.stream = stream;
      stream.getAudioTracks().forEach((t) => { t.enabled = !this.muted; });
      try {
        pc = await this.peer();
        if (over()) { drop(); return; }
        this.pc = pc;
        stream.getTracks().forEach((t) => pc.addTrack(t, stream));
        await pc.setRemoteDescription(this.offer);
        await pc.setLocalDescription(await pc.createAnswer());
      } catch (_) { if (over()) drop(); else { toast('The call couldn\'t connect.', true); this.decline(); } return; }
      if (over()) { drop(); return; }
      this.send('accept', { sdp: pc.localDescription });
      this.earlyIce.splice(0).forEach((c) => this.send('ice', c));
      for (const c of this.pendingIce.splice(0)) await pc.addIceCandidate(c).catch(() => {});
    },
    decline() { this.send('decline'); this.clean(); },
    hang() { if (this.id) this.send('end'); this.clean(); },
    connected() {
      if (this.started) return; this.started = Date.now(); this.phase = 'live'; clearTimeout(this.ringTimer); this.ringing(false, false);
      const show = () => this.setStatus(this.weak ? 'Reconnecting…' : clock((Date.now() - this.started) / 1000));
      show(); this.tick = setInterval(show, 500);
    },
    async signal(s) {
      if (s.type === 'ring') {
        if (this.ui) { return; } // busy: the other side keeps ringing until they give up
        this.thread = s.thread; this.id = s.call; this.other = s.from; this.role = 'callee'; this.offer = s.data && s.data.sdp; this.pendingIce = []; this.earlyIce = [];
        this.show('is calling you…', true); this.ringing(true, true);
        // The caller's screen moves from "Calling…" to "Ringing…" now that it really rings somewhere.
        this.send('ringing');
        if (this.want && this.want.call === s.call) { const answer = this.want.answer; this.want = null; if (answer) this.accept(); }
        // The tab says so too, for someone looking at another one.
        this.title = document.title; document.title = `📞 ${(s.from && s.from.name) || 'Someone'} is calling…`;
        if (navigator.vibrate) navigator.vibrate([400, 200, 400, 200, 400]);
        this.ringTimer = setTimeout(() => { if (!this.pc) this.clean(); }, 50000);
        return;
      }
      if (s.call !== this.id || s.thread !== this.thread) return;
      if (s.type === 'ringing') { if (this.role === 'caller' && this.phase === 'calling' && this.ui) { this.phase = 'ringing'; this.setStatus('Ringing…'); this.ringing(true, false); } }
      else if (s.type === 'answer' && this.pc) {
        this.ringing(false);
        if (this.phase !== 'live') { this.phase = 'connecting'; this.setStatus('Connecting…'); }
        await this.pc.setRemoteDescription(s.data.sdp); for (const c of this.pendingIce.splice(0)) await this.pc.addIceCandidate(c).catch(() => {});
      }
      else if (s.type === 'ice') { if (this.pc && this.pc.remoteDescription) await this.pc.addIceCandidate(s.data).catch(() => {}); else this.pendingIce.push(s.data); }
      else if (s.type === 'declined') { toast('The call was declined.'); this.clean(); }
      else if (s.type === 'ended') { if (this.started) toast('Call ended.'); this.clean(); }
      else if (s.type === 'taken') { this.clean(); }
      // Answered or turned down on another of this person's pages (or by another admin): this one stops ringing.
      else if (s.type === 'elsewhere') { if (this.role === 'callee' && s.by !== PID) this.clean(); }
    },
    show(status, incoming) {
      const o = el('div', 'dm-call-ui'); o.setAttribute('role', 'dialog'); o.setAttribute('aria-label', 'Voice call');
      // Their face without the green dot: whether they are there is what the words under it say, as it happens.
      o.append(avatar({ ...(this.other || {}), online: false }, 96));
      o.append(el('b', 'dm-call-name', (this.other && this.other.name) || 'Call'));
      o.append(el('p', 'dm-call-st', status));
      o.append(el('div', 'dm-call-btns'));
      document.body.append(o); this.ui = o; this.phase = incoming ? 'ringing' : 'calling'; this.buttons(incoming);
    },
    buttons(incoming) {
      const box = $('.dm-call-btns', this.ui); box.replaceChildren();
      const btn = (cls, label, svg, fn) => { const b = el('button', `dm-cb ${cls}`); b.type = 'button'; b.innerHTML = icon(svg); b.setAttribute('aria-label', label); b.title = label; b.addEventListener('click', fn); box.append(b); return b; };
      if (incoming) {
        btn('no', 'Decline', I.phone, () => this.decline());
        btn('yes', 'Answer', I.phone, () => this.accept()).focus();
      } else {
        const m = btn('mute', 'Mute', I.mic, () => this.mute(!this.muted));
        m.setAttribute('aria-pressed', 'false');
        this.mute(this.muted);
        btn('no', 'End call', I.phone, () => this.hang());
      }
    },
    setStatus(t) { const s = this.ui && $('.dm-call-st', this.ui); if (s) s.textContent = t; },
    // A browser that won't play the other side's voice until a tap (a call answered from an alert, say): one button.
    needTap(a) {
      if (!this.ui || $('.dm-tap', this.ui)) return;
      const b = el('button', 'btn dm-tap', `Tap to hear ${(this.other && this.other.name) || 'them'}`); b.type = 'button';
      b.addEventListener('click', () => { a.play().then(() => b.remove()).catch(() => {}); });
      $('.dm-call-btns', this.ui).before(b);
    },
    // On: a plain microphone, in colour (you can be heard). Off: the microphone crossed out, dark.
    mute(off) {
      this.muted = off;
      this.stream?.getAudioTracks().forEach((t) => { t.enabled = !off; });
      const b = this.ui && $('.dm-cb.mute', this.ui); if (!b) return;
      b.classList.toggle('on', off); b.innerHTML = icon(off ? I.mute : I.mic);
      const label = off ? 'Unmute' : 'Mute';
      b.setAttribute('aria-label', label); b.title = label; b.setAttribute('aria-pressed', String(off));
    },
    ringing(on, incoming) {
      if (this.tone) { clearInterval(this.tone.timer); this.tone.ctx.close().catch(() => {}); this.tone = null; }
      if (!on) return;
      try {
        const ctx = new (window.AudioContext || window.webkitAudioContext)();
        const beep = () => {
          const o = ctx.createOscillator(), g = ctx.createGain();
          o.frequency.value = incoming ? 660 : 440; g.gain.value = 0.0001; o.connect(g); g.connect(ctx.destination);
          const t = ctx.currentTime; g.gain.exponentialRampToValueAtTime(incoming ? 0.2 : 0.06, t + 0.03); g.gain.exponentialRampToValueAtTime(0.0001, t + 0.9);
          o.start(t); o.stop(t + 1);
        };
        beep(); this.tone = { ctx, timer: setInterval(beep, incoming ? 1600 : 3000) };
      } catch (_) {}
    },
    clean() {
      clearTimeout(this.ringTimer); clearInterval(this.tick); this.ringing(false);
      this.pc?.close(); this.stream?.getTracks().forEach((t) => t.stop());
      const a = $('#dmCallAudio'); if (a) a.srcObject = null;
      this.ui?.remove();
      if (this.title != null) document.title = this.title;
      this.gen++;
      Object.assign(this, { pc: null, stream: null, id: null, ui: null, started: 0, offer: null, pendingIce: [], earlyIce: [], title: null, phase: 'calling', weak: false, muted: false });
      kick();
    },
  };
  if (window.visualViewport) {
    const vv = window.visualViewport;
    const fitScreen = () => {
      const n = win.node; if (!n || n.hidden) return;
      if (!phone()) { n.style.height = ''; n.style.top = ''; return; }
      n.style.height = `${Math.round(vv.height)}px`; n.style.top = `${Math.round(vv.offsetTop)}px`;
      const log = n.querySelector('.dm-log'); if (log && log.scrollHeight - log.scrollTop - log.clientHeight < 160) log.scrollTop = log.scrollHeight;
    };
    vv.addEventListener('resize', fitScreen); vv.addEventListener('scroll', fitScreen);
    win.fitScreen = fitScreen;
  }
  document.addEventListener('visibilitychange', () => { if (document.hidden) return; if (win.thread && win.node && !win.node.hidden && !win.watch) win.read(); if (live.running) kick(); });
  window.addEventListener('pagehide', () => { if (call.id && (call.pc || call.role === 'caller')) navigator.sendBeacon?.(`/dm/${call.thread}/call`, new Blob([JSON.stringify({ type: 'end', call: call.id })], { type: 'application/json' })); });

  /* ---------------- message buttons on cards ---------------- */
  document.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-dm-vault], [data-dm-card], [data-dm-to]'); if (!b) return;
    e.preventDefault(); e.stopPropagation();
    const body = b.dataset.dmVault != null ? { vault: Number(b.dataset.dmVault) } : b.dataset.dmCard != null ? { card: Number(b.dataset.dmCard) } : { to: Number(b.dataset.dmTo) };
    b.classList.add('busy');
    try {
      const j = await api('/dm/open', body);
      await win.open(j.thread, { card: j.card && j.card.title ? j.card : null });
    } catch (err) { if (err.login) toLogin(); else toast(err.message, true); }
    b.classList.remove('busy');
  }, true);
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    const b = e.target.closest && e.target.closest('.dm-mini'); if (!b) return;
    e.preventDefault(); b.click();
  });

  /* ---------------- New message: anyone on the site, about anything ---------------- */
  let picker = null;
  function people() {
    if (picker) picker.remove();
    const o = el('div', 'dm-people'); o.setAttribute('role', 'dialog'); o.setAttribute('aria-modal', 'true'); o.setAttribute('aria-label', 'New message');
    const box = el('div', 'dm-people-in');
    const head = el('header', 'dm-people-h'); head.append(el('b', '', 'New message'));
    const x = el('button', 'dm-ib', '✕'); x.type = 'button'; x.setAttribute('aria-label', 'Close'); head.append(x);
    const q = el('input'); q.type = 'search'; q.placeholder = 'Search trainers by name'; q.setAttribute('aria-label', 'Search trainers by name'); q.autocomplete = 'off';
    const list = el('ul', 'dm-list dm-people-list');
    box.append(head, q, list); o.append(box); document.body.append(o); picker = o;
    const esc = (e) => { if (e.key === 'Escape') close(); };
    const close = () => { o.remove(); if (picker === o) picker = null; document.removeEventListener('keydown', esc); };
    x.addEventListener('click', close);
    o.addEventListener('mousedown', (e) => { if (e.target === o) close(); });
    document.addEventListener('keydown', esc);
    let seq = 0, timer = 0;
    const draw = async () => {
      const mine = ++seq;
      let j;
      try { j = await api(`/dm/people?q=${encodeURIComponent(q.value.trim())}`); }
      catch (err) { if (err.login) { toLogin(); return; } list.replaceChildren(el('li', 'dm-empty', err.message)); return; }
      if (mine !== seq) return;
      list.replaceChildren();
      if (!(j.people || []).length) { list.append(el('li', 'dm-empty', q.value.trim() ? 'No trainer by that name.' : 'Nobody else is here yet.')); return; }
      j.people.forEach((p) => {
        const li = el('li'); const a = el('button', 'dm-li'); a.type = 'button';
        a.append(avatar(p, 44));
        const t = el('span', 'dm-li-t'); const top = el('span', 'dm-li-top'); top.append(el('b', '', p.name));
        if (p.role) top.append(el('span', 'dm-tag', p.role));
        t.append(top);
        const sub = [p.store, p.online ? 'Active now' : ''].filter(Boolean).join(' · ');
        if (sub) t.append(el('span', 'dm-li-last', sub));
        a.append(t);
        a.addEventListener('click', async () => {
          a.disabled = true;
          try { const r = await api('/dm/open', { to: p.id }); close(); await win.open(r.thread); }
          catch (err) { a.disabled = false; if (err.login) toLogin(); else toast(err.message, true); }
        });
        li.append(a); list.append(li);
      });
    };
    q.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(draw, 180); });
    draw();
    setTimeout(() => q.focus(), 30);
  }
  document.addEventListener('click', (e) => {
    const b = e.target.closest && e.target.closest('[data-dm-new]'); if (!b) return;
    e.preventDefault();
    if (!live.me) { toLogin(); return; }
    people();
  });

  /* ---------------- a conversation's line: who, what was said last, how many are unread ---------------- */
  function threadRow(r, admin, after) {
    const li = el('li'); const a = el('button', `dm-li${r.unread ? ' unread' : ''}`); a.type = 'button';
    if (admin) { const pair = el('span', 'dm-pair'); pair.append(avatar(r.a, 40), avatar(r.b, 40)); a.append(pair); }
    else a.append(avatar(r.other, 52));
    const t = el('span', 'dm-li-t');
    const name = admin ? `${r.a.name} ↔ ${r.b.name}` : r.other.name;
    const top = el('span', 'dm-li-top'); top.append(el('b', '', name));
    if (r.as_vault || (admin && r.vault)) top.append(el('span', 'dm-tag', 'Vault'));
    t.append(top, el('span', 'dm-li-last', `${r.last_mine ? 'You: ' : ''}${r.last} · ${ago(r.at)}`));
    a.append(t);
    if (r.unread) a.append(el('span', 'dm-dot', String(r.unread)));
    a.addEventListener('click', () => { win.open(r.id, { watch: admin && !r.vault }); a.classList.remove('unread'); a.querySelector('.dm-dot')?.remove(); if (after) after(); });
    li.append(a);
    return li;
  }

  /* ---------------- the inbox under the header's Messages button (the new look) ----------------
     Pressing the button drops the inbox down beside it; a conversation opens in the chat window, and "Open the full
     inbox" goes to the Messages page. A phone has no room for a panel: there the button opens that page. */
  const drop = { menu: null, btn: null, list: null, open: false, rows: null, seq: 0 };
  function dropDraw() {
    const rows = drop.rows || [];
    drop.list.replaceChildren();
    if (!rows.length) { drop.list.append(el('p', 'dm-empty', 'No messages yet. Tap “Message” on any card to ask its seller, or start one with the pencil above.')); return; }
    const ul = el('ul', 'dm-list');
    rows.slice(0, 40).forEach((r) => ul.append(threadRow(r, false, () => dropSet(false))));
    drop.list.append(ul);
  }
  async function dropLoad() {
    const my = ++drop.seq;
    if (!drop.rows) drop.list.replaceChildren(el('p', 'dm-empty', 'Loading…'));
    try {
      const j = await api('/dm/inbox'); if (my !== drop.seq) return;
      drop.rows = j.threads || [];
      if (drop.open) dropDraw();
    } catch (err) {
      if (my !== drop.seq) return;
      if (err.login) { toLogin(); return; }
      if (drop.open && !drop.rows) drop.list.replaceChildren(el('p', 'dm-empty', err.message));
    }
  }
  function dropSet(open) {
    if (!drop.menu || drop.open === open) return;
    drop.open = open; drop.menu.hidden = !open; drop.btn.setAttribute('aria-expanded', String(open));
    // What was there last time shows at once, then what is true now.
    if (open) { if (drop.rows) dropDraw(); dropLoad(); }
  }
  function dropSetup() {
    const menu = $('#dmMenu'), btn = $('#barDm'), list = $('#dmMenuList');
    if (!menu || !btn || !list) return;
    Object.assign(drop, { menu, btn, list });
    if (/^\/me\/messages/.test(location.pathname)) btn.setAttribute('aria-current', 'page');
    btn.addEventListener('click', (e) => {
      // The whole inbox instead: on a phone, in a new tab, and on the Messages page (it is already on the screen).
      if (phone() || e.button || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || $('[data-dm-page]')) return;
      e.preventDefault(); dropSet(!drop.open);
    });
    // Pressing anywhere else puts it away. Heard on the way down, so a button that keeps its click to itself counts too.
    document.addEventListener('click', (e) => { if (drop.open && !(e.target.closest && e.target.closest('#dmDropdown'))) dropSet(false); }, true);
    menu.addEventListener('click', (e) => { if (e.target.closest('a, [data-dm-new]')) dropSet(false); });
    document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && drop.open) { e.stopPropagation(); dropSet(false); btn.focus(); } }, true);
    let lastV = -1;
    live.listeners.add((j) => { if (j.v !== lastV) { lastV = j.v; if (drop.open) dropLoad(); } });
  }

  /* ---------------- the inbox page ---------------- */
  async function inboxPage(root) {
    const box = $('[data-dm-inbox]', root), admin = root.hasAttribute('data-dm-admin');
    const draw = async () => {
      let j;
      try { j = await api(admin ? '/admin/dm' : '/dm/inbox'); } catch (err) { if (err.login) toLogin(); box.replaceChildren(el('p', 'dm-empty', err.message)); return; }
      const rows = j.threads || [];
      box.replaceChildren();
      if (!rows.length) { box.append(el('p', 'dm-empty', admin ? 'No conversations yet.' : 'No messages yet. Tap “Message” on any card to ask its seller.')); return; }
      const ul = el('ul', 'dm-list');
      rows.forEach((r) => ul.append(threadRow(r, admin)));
      box.append(ul);
      return rows;
    };
    const rows = await draw();
    const t = Number(new URLSearchParams(location.search).get('t'));
    // An admin answers the vault's own conversations; anyone else's they only read.
    if (t) win.open(t, { watch: admin && !(rows || []).some((r) => r.id === t && r.vault) });
    if (!admin) { let lastV = -1; live.listeners.add((j) => { if (j.v !== lastV) { lastV = j.v; draw(); } }); }
    else setInterval(draw, 15000);
  }

  /* ---------------- a call's alert was tapped ----------------
     The phone's alert (static/sw.js) opens this page with ?call=…[&answer=1], or tells an open page by message. The
     call shows (and is answered, for "Answer") when its ring arrives; if it never does, the call is already over. */
  function wantCall(id, answer) {
    if (!id) return;
    if (call.ui && call.id === id && call.role === 'callee') { if (answer && !call.pc) call.accept(); return; }
    call.want = { call: id, answer: !!answer };
    if (live.running) kick();
    setTimeout(() => { if (call.want && call.want.call === id) { call.want = null; toast('That call has ended.'); } }, 9000);
  }
  navigator.serviceWorker?.addEventListener('message', (e) => {
    const d = e.data || {};
    if (d.bbv === 'call') wantCall(d.call, d.answer);
    else if (d.bbv === 'open' && typeof d.url === 'string' && d.url.startsWith('/')) location.href = d.url;
  });

  /* ---------------- start ---------------- */
  async function boot() {
    const w = await who();
    const me = w && w.member;
    const icon2 = $('#barDm'), holder = $('#dmDropdown');
    if (!me) { if (icon2) icon2.hidden = true; if (holder) holder.hidden = true; return; }
    live.me = me.id;
    // Nobody messages themselves: a Message button that points at you is put away.
    document.querySelectorAll(`[data-dm-to="${me.id}"]`).forEach((n) => { n.hidden = true; });
    if (icon2) icon2.hidden = false;
    if (holder) holder.hidden = false;
    dropSetup();
    startLive();
    const page = $('[data-dm-page]'); if (page) inboxPage(page);
    // The chat that was open on the last page comes back (as a bubble on a phone, or when it was minimised).
    // Only the chat this person had open: someone else may have used this tab before them.
    let saved = store.get('bbv:dm');
    if (saved && saved.m !== me.id) { store.set('bbv:dm', null); saved = null; }
    if (saved && saved.t && !page) {
      if (saved.min || phone()) { win.thread = saved.t; try { const j = await api(`/dm/${saved.t}`); win.other = j.other; chatHead(j.other); win.build().hidden = true; win.node.classList.add('min'); } catch (_) { store.set('bbv:dm', null); } }
      else win.open(saved.t, { quiet: true });
    }
    // Opened from a call's alert: that call, answered if "Answer" was tapped. The address loses the call's part.
    const q = new URLSearchParams(location.search);
    if (q.get('call')) {
      wantCall(q.get('call'), q.get('answer') === '1');
      q.delete('call'); q.delete('answer');
      history.replaceState(history.state, '', location.pathname + (q.toString() ? `?${q}` : '') + location.hash);
    }
    // A notification link to a chat opens it.
    const t = Number(new URLSearchParams(location.search).get('t'));
    if (t && !page && /\/me\/messages/.test(location.pathname)) win.open(t);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', boot); else boot();
})();
