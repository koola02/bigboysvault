// Vouches: the floating button, writing a vouch or a reply, and the buttons that depend on who's looking.
// Pages are shared and saved, so those buttons are added here, once the site has said who this is.
(() => {
  if (document.querySelector('.admin')) return;
  let me = null, owner = false, full = false, known = false;

  const make = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const button = (cls, text, fn) => { const b = make('button', cls, text); b.type = 'button'; b.addEventListener('click', fn); return b; };

  const say = (msg, bad) => {
    let t = document.querySelector('.me-toast');
    if (!t) { t = make('div', 'me-toast'); t.setAttribute('role', 'status'); }
    (document.querySelector('dialog[open]:last-of-type') || document.body).append(t); // above any open window
    t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on');
    clearTimeout(say.t); say.t = setTimeout(() => t.classList.remove('on'), bad ? 5000 : 2400);
  };
  async function send(url, body) {
    const opts = { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' } };
    if (body instanceof FormData) opts.body = body;
    else { opts.headers['Content-Type'] = 'application/json'; opts.body = JSON.stringify(body ?? {}); }
    let r;
    try { r = await fetch(url, opts); } catch (_) { throw new Error('No connection. Please try again.'); }
    const j = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
    return j;
  }

  const ago = (t) => {
    const s = Math.max(0, Date.now() / 1000 - t);
    const say = (n, unit) => { const f = Math.floor(n); return `${f} ${unit}${f === 1 ? '' : 's'} ago`; };
    if (s < 60) return 'just now';
    if (s < 3600) return say(s / 60, 'minute');
    if (s < 86400) return say(s / 3600, 'hour');
    if (s < 1209600) return say(s / 86400, 'day');
    if (s < 5184000) return say(s / 604800, 'week');
    if (s < 31536000) return say(s / 2592000, 'month');
    return say(s / 31536000, 'year');
  };

  /* ---------- buttons on each vouch ---------- */
  function scan(root = document) {
    root.querySelectorAll('time[data-at]').forEach((n) => { n.textContent = ago(Number(n.dataset.at)); });
    if (!known) return; // the buttons wait until the site has said who this is
    root.querySelectorAll('[data-vouch]').forEach((v) => {
      const acts = v.querySelector(':scope > [data-acts]');
      if (!acts || acts.dataset.ready) return;
      acts.dataset.ready = '1';
      const id = v.dataset.vouch, mine = me && String(me.id) === v.dataset.owner;
      // Moderators don't overrule other staff.
      const canManage = owner && (full || mine || !v.hasAttribute('data-staff'));
      if (me && !me.locked && v.classList.contains('vouch')) acts.append(button('v-act', '↩ Reply', () => compose(v)));
      if (canManage) acts.append(button('v-act', 'Hide', () => act(id, 'hide')));
      if (mine || canManage) acts.append(button('v-act bad', 'Delete', () => act(id, 'delete')));
    });
  }
  const gone = (id) => document.querySelectorAll(`[data-vouch="${CSS.escape(id)}"]`).forEach((n) => n.remove());
  async function act(id, action) {
    if (action === 'delete' && !confirm('Delete this for good? Replies under it go too.')) return;
    try {
      await send(`/vouches/${id}/act`, { action });
      gone(id);
      say(action === 'hide' ? 'Hidden. You can bring it back from Admin › Trainers.' : 'Deleted');
    } catch (err) { say(err.message, true); }
  }

  /* ---------- the floating button ---------- */
  function addButton() {
    // Only where vouching belongs: the home page, the trainers and the vouches. Browsing cards and binders stays clear.
    if (!/^\/($|trainers|vouches)/.test(location.pathname)) return;
    const b = button('v-fab', '', () => {
      if (me.locked) { say(`Your trainer is locked for another ${me.lock_left}.`, true); return; }
      compose(null);
    });
    b.setAttribute('aria-label', 'Write a vouch');
    b.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/></svg><span>Vouch</span>';
    document.body.append(b);
  }

  /* ---------- writing ---------- */
  // Phone photos are huge: shrink here first, so they go up quickly on mobile data.
  async function shrink(file, max = 1600) {
    const url = URL.createObjectURL(file);
    try {
      const img = new Image(); img.src = url; await img.decode();
      const k = Math.min(1, max / Math.max(img.naturalWidth, img.naturalHeight));
      const c = document.createElement('canvas'); c.width = Math.round(img.naturalWidth * k); c.height = Math.round(img.naturalHeight * k);
      c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
      return await new Promise((res) => c.toBlob(res, 'image/jpeg', 0.88));
    } finally { URL.revokeObjectURL(url); }
  }

  function compose(parent) {
    const dialog = make('dialog', 'float vc');
    const form = make('form', 'float-panel'); form.tabIndex = -1; form.noValidate = true;
    const name = parent?.dataset.name;
    const head = make('h2', 'vc-h', parent ? `Reply to ${name}` : 'Write a vouch');
    form.append(head);
    if (parent) {
      const said = parent.querySelector(':scope > blockquote')?.textContent;
      if (said) form.append(make('p', 'vc-to', said));
    }
    const text = make('textarea'); text.maxLength = 1000; text.rows = 4; text.placeholder = parent ? 'Write your reply' : 'Tell people about the vault, or show them with photos';
    text.setAttribute('aria-label', parent ? 'Your reply' : 'Your vouch');
    const pics = make('div', 'vc-pics');
    const add = make('label', 'vc-add');
    add.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 8h3l2-3h6l2 3h3a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1z"/><circle cx="12" cy="13.5" r="3.5"/></svg><span>Add photos</span>';
    const file = make('input', 'sr'); file.type = 'file'; file.accept = 'image/*'; file.multiple = true; file.setAttribute('aria-label', 'Add photos');
    add.append(file); pics.append(add);
    const err = make('p', 'vc-err'); err.setAttribute('role', 'alert');
    const foot = make('div', 'vc-foot');
    const count = make('small', null, '0 of 1000');
    const post = make('button', 'btn', parent ? 'Send reply' : 'Post vouch'); post.type = 'submit';
    foot.append(count, post);
    form.append(text, pics, err, foot);
    dialog.append(form, make('p', 'float-hint', 'Tap outside to close'));
    document.body.append(dialog);

    const items = [];
    let queue = Promise.resolve(), busy = false;
    text.addEventListener('input', () => { count.textContent = `${text.value.length} of 1000`; });

    // Photos go up as soon as they're picked, one at a time, with no limit on how many.
    file.addEventListener('change', () => {
      for (const f of file.files) {
        const it = { id: null, node: make('div', 'vc-pic busy') };
        const img = make('img'); img.alt = ''; it.node.append(img);
        it.node.append(button('vc-x', '✕', () => {
          items.splice(items.indexOf(it), 1); it.node.remove();
          if (it.id) send(`/me/photos/${it.id}/drop`).catch(() => {});
          it.gone = true;
        }));
        it.node.lastChild.setAttribute('aria-label', 'Take this photo out');
        pics.insertBefore(it.node, add);
        items.push(it);
        queue = queue.then(async () => {
          if (it.gone) return;
          try {
            const blob = await shrink(f).catch(() => f);
            img.src = URL.createObjectURL(blob);
            const fd = new FormData(); fd.append('photo', blob, 'photo.jpg');
            const j = await send('/me/photos', fd);
            if (it.gone) { send(`/me/photos/${j.id}/drop`).catch(() => {}); return; }
            it.id = j.id; it.node.classList.remove('busy');
          } catch (e) {
            items.splice(items.indexOf(it), 1); it.node.remove(); it.gone = true;
            err.textContent = e.message;
          }
        });
      }
      file.value = '';
    });

    const touched = () => text.value.trim() || items.length;
    const close = () => { dialog.close(); dialog.remove(); };
    const discard = () => {
      if (busy) return;
      if (touched() && !confirm(parent ? 'Discard your reply?' : 'Discard your vouch?')) return;
      items.forEach((it) => { it.gone = true; if (it.id) send(`/me/photos/${it.id}/drop`).catch(() => {}); });
      close();
    };
    let downOutside = false;
    dialog.addEventListener('pointerdown', (e) => { downOutside = !form.contains(e.target); });
    dialog.addEventListener('click', (e) => { if (downOutside && !form.contains(e.target)) discard(); downOutside = false; });
    dialog.addEventListener('cancel', (e) => { e.preventDefault(); discard(); });

    form.addEventListener('submit', async (e) => {
      e.preventDefault();
      if (busy) return;
      err.textContent = '';
      busy = true; post.disabled = true; post.textContent = items.some((i) => !i.id) ? 'Sending photos…' : 'Posting…';
      try {
        await queue; // the post waits for any photos still going up
        post.textContent = 'Posting…';
        const j = await send('/vouches', { text: text.value, parent: parent ? Number(parent.dataset.vouch) : null, photos: items.map((i) => i.id).filter(Boolean) });
        const html = await (await fetch(`/vouches/${j.id}`, { credentials: 'same-origin' })).text();
        close();
        place(html, parent);
        say(parent ? 'Reply posted ✓' : 'Vouch posted ✓');
      } catch (ex) {
        err.textContent = ex.message;
        busy = false; post.disabled = false; post.textContent = parent ? 'Send reply' : 'Post vouch';
      }
    });

    dialog.showModal();
    text.focus();
  }

  const parse = (html) => { const t = document.createElement('template'); t.innerHTML = html.trim(); return t.content; };
  function place(html, parent) {
    if (parent) {
      // The same vouch can be on the page and in a window above it.
      document.querySelectorAll(`[data-vouch="${CSS.escape(parent.dataset.vouch)}"] > [data-replies]`).forEach((box) => { box.append(parse(html)); scan(box); });
      return;
    }
    const grids = [...document.querySelectorAll(`.vouch-grid[data-wall], .vouch-grid[data-home], .vouch-grid[data-trainer="${me.id}"]`)];
    grids.forEach((g) => { g.prepend(parse(html)); scan(g); });
    document.querySelector('[data-empty]')?.remove();
    if (!grids.length) say('Vouch posted ✓ See it under Trainers › Read the vouches.');
  }

  /* ---------- show more ---------- */
  document.addEventListener('click', async (e) => {
    const b = e.target.closest('.v-load'); if (!b) return;
    b.disabled = true;
    try {
      const html = await (await fetch(`/vouches?part=1&before=${b.dataset.before}`, { credentials: 'same-origin' })).text();
      const part = parse(html);
      const next = part.querySelector('.v-load');
      next?.remove();
      const grid = document.querySelector('.vouch-grid[data-wall]');
      grid.append(part); scan(grid);
      if (next) { b.dataset.before = next.dataset.before; b.disabled = false; } else b.closest('.v-load-row').remove();
    } catch (_) { b.disabled = false; say('Couldn’t load more just now.', true); }
  });

  // Anything new in the floating window gets its buttons too.
  document.addEventListener('float:filled', (e) => scan(e.detail));

  scan();
  fetch('/me/who', { credentials: 'same-origin', headers: { Accept: 'application/json' } }).then((r) => r.json()).then((j) => {
    me = j.member; owner = !!j.admin; full = !!j.full; known = true;
    if (me) addButton();
    scan();
  }).catch(() => {});
})();
