/* The feed: likes and other reactions, comments, sharing, the post menu, and writing a post.
   The posts themselves are drawn by the server; more arrive as you scroll (the last element asks for the next page).
   Everything here is one set of listeners on the page, so posts that arrive later work without any setup. */
(() => {
  const feed = document.getElementById('feed');
  if (!feed) return;
  const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const REACTIONS = [['like', '👍', 'Like'], ['love', '❤️', 'Love'], ['fire', '🔥', 'Fire'], ['wow', '😮', 'Wow'], ['haha', '😆', 'Haha'], ['sad', '😢', 'Sad']];
  const THUMB = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 10v11H3V10zM7 10l4-8a2.5 2.5 0 0 1 2.5 3L13 9h6a2 2 0 0 1 2 2.3l-1.3 8A2 2 0 0 1 17.700 21H7"/></svg>';
  const SEND = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 12l16-8-6 16-3-7z"/></svg>';
  let me = null, staff = false, full = false;
  const who = window.bbvWho ? window.bbvWho() : Promise.resolve({});
  who.then((j) => {
    me = (j && j.member) || null; staff = !!(j && j.admin); full = !!(j && j.full);
    document.querySelectorAll('[data-my-face]').forEach((n) => face(n, me));
  });
  const face = (n, m) => { n.replaceChildren(); if (m && m.avatar) { const i = el('img'); i.src = m.avatar; i.alt = ''; n.append(i); } else n.textContent = m ? (m.initial || m.name[0].toUpperCase()) : ''; };
  const toast = (msg, bad) => { let t = document.querySelector('.cart-toast'); if (!t) { t = el('div', 'cart-toast'); t.setAttribute('role', 'status'); document.body.append(t); } t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on'); clearTimeout(toast.t); toast.t = setTimeout(() => t.classList.remove('on'), bad ? 5000 : 2600); };
  const toLogin = () => { try { sessionStorage.setItem('bbv:after-login', location.pathname + location.search); } catch (_) {} location.href = '/login'; };
  async function api(url, body) {
    const init = { method: body === undefined ? 'GET' : 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' } };
    if (body instanceof FormData) init.body = body; else if (body !== undefined) { init.headers['Content-Type'] = 'application/json'; init.body = JSON.stringify(body); }
    const r = await fetch(url, init);
    if (r.status === 401) { toLogin(); throw new Error('Please log in.'); }
    const type = r.headers.get('content-type') || '';
    const j = type.includes('json') ? await r.json().catch(() => ({})) : { html: await r.text() };
    if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
    return j;
  }
  const postOf = (n) => n.closest('[data-post]');

  /* ---------- reactions ---------- */
  function paintLike(post, j) {
    const b = post.querySelector('[data-like]'), ico = b.querySelector('[data-like-ico]'), word = b.querySelector('[data-like-word]');
    b.dataset.mine = j.mine || ''; b.setAttribute('aria-pressed', String(!!j.mine));
    if (j.mine) { ico.textContent = j.emoji; word.textContent = (REACTIONS.find((r) => r[0] === j.mine) || [])[2] || 'Like'; } else { ico.innerHTML = THUMB; word.textContent = 'Like'; }
    const box = post.querySelector('[data-reacts]');
    box.hidden = !j.n;
    box.replaceChildren(...(j.tops || []).map((e) => el('i', '', e)), Object.assign(el('span', '', String(j.n)), { }));
    box.lastChild.dataset.likeN = '';
    stats(post);
  }
  const stats = (post) => { const s = post.querySelector('.post-stats'); if (s) s.hidden = post.querySelector('[data-reacts]').hidden && post.querySelector('.post-cn').hidden; };
  async function react(post, kind) {
    closePick();
    if (!me) { toLogin(); return; }
    try { paintLike(post, await api(`/posts/${post.dataset.post}/react`, { kind })); } catch (e) { toast(e.message, true); }
  }
  let pick = null, pickTimer = 0;
  const closePick = () => { clearTimeout(pickTimer); pick?.remove(); pick = null; };
  function openPick(btn) {
    closePick();
    pick = el('div', 'react-pick'); pick.setAttribute('role', 'menu');
    REACTIONS.forEach(([k, e, label]) => { const b = el('button', '', e); b.type = 'button'; b.title = label; b.setAttribute('aria-label', label); b.addEventListener('click', (ev) => { ev.stopPropagation(); react(postOf(btn), k); }); pick.append(b); });
    btn.parentElement.append(pick);
    const r = btn.getBoundingClientRect(), host = btn.parentElement.getBoundingClientRect();
    pick.style.left = `${Math.max(0, r.left - host.left)}px`;
  }
  // A mouse resting on Like, or a finger held on it, opens the other reactions.
  feed.addEventListener('pointerover', (e) => { const b = e.target.closest('[data-like]'); if (!b || e.pointerType !== 'mouse' || pick) return; clearTimeout(pickTimer); pickTimer = setTimeout(() => openPick(b), 450); });
  feed.addEventListener('pointerout', (e) => { const b = e.target.closest('[data-like], .react-pick'); if (!b || e.pointerType !== 'mouse') return; clearTimeout(pickTimer); if (pick && !e.relatedTarget?.closest?.('[data-like], .react-pick')) pickTimer = setTimeout(closePick, 350); });
  let held = false;
  feed.addEventListener('pointerdown', (e) => { const b = e.target.closest('[data-like]'); if (!b || e.pointerType === 'mouse') return; held = false; clearTimeout(pickTimer); pickTimer = setTimeout(() => { held = true; openPick(b); if (navigator.vibrate) navigator.vibrate(8); }, 420); });
  feed.addEventListener('pointerup', () => { if (!held) clearTimeout(pickTimer); });
  feed.addEventListener('contextmenu', (e) => { if (e.target.closest('[data-like]')) e.preventDefault(); });

  /* ---------- comments ---------- */
  function commentNode(k) {
    const li = el('div', `cm${k.parent ? ' reply' : ''}`); li.dataset.cm = k.id;
    const av = el('a', 'av sm'); av.href = `/trainers/${k.member}`; face(av, { avatar: k.avatar, initial: k.initial, name: k.name });
    const body = el('div', 'cm-body');
    const bub = el('div', 'cm-bub'); const name = el('a', 'cm-name', k.name); name.href = `/trainers/${k.member}`;
    bub.append(name, el('p', '', k.text));
    const meta = el('div', 'cm-meta'); meta.append(el('span', '', k.edited ? `${k.when} · Edited` : k.when));
    const rep = el('button', 'linkbtn', 'Reply'); rep.type = 'button'; rep.dataset.reply = k.parent || k.id; rep.dataset.name = k.name; meta.append(rep);
    if (k.own) { const ed = el('button', 'linkbtn', 'Edit'); ed.type = 'button'; ed.dataset.cmEdit = k.id; meta.append(ed); }
    if (k.can_remove) { const d = el('button', `linkbtn${k.own ? '' : ' staff-only'}`, 'Delete'); d.type = 'button'; d.dataset.cmDel = k.id; meta.append(d); }
    body.append(bub, meta); li.append(av, body);
    return li;
  }
  async function openComments(post, focus) {
    const box = post.querySelector('[data-comments]');
    if (!box.dataset.ready) {
      box.dataset.ready = '1'; box.hidden = false;
      const list = el('div', 'cm-list'); box.append(list);
      const form = el('form', 'cm-form'); form.innerHTML = `<span class="av sm" data-my-face></span><input type="text" maxlength="600" placeholder="Write a comment…" aria-label="Write a comment" autocomplete="off" enterkeyhint="send"><button type="submit" class="ibtn" aria-label="Send">${SEND}</button>`;
      face(form.querySelector('[data-my-face]'), me); box.append(form);
      try { const j = await api(`/posts/${post.dataset.post}/comments`); j.comments.forEach((k) => place(list, commentNode(k), k.parent)); } catch (e) { toast(e.message, true); }
    } else box.hidden = false;
    if (focus) box.querySelector('input').focus({ preventScroll: false });
  }
  // A reply sits under the comment it answers, after the replies already there.
  function place(list, node, parent) {
    if (!parent) { list.append(node); return; }
    const top = list.querySelector(`[data-cm="${parent}"]`);
    if (!top) { list.append(node); return; }
    let after = top; while (after.nextElementSibling && after.nextElementSibling.classList.contains('reply')) after = after.nextElementSibling;
    after.after(node);
  }
  const setCount = (post, n) => { const b = post.querySelector('.post-cn'); b.hidden = !n; b.textContent = `${n} comment${n === 1 ? '' : 's'}`; stats(post); };
  feed.addEventListener('submit', async (e) => {
    const f = e.target.closest('.cm-form'); if (!f) return;
    e.preventDefault();
    if (!me) { toLogin(); return; }
    const input = f.querySelector('input'), text = input.value.trim(), post = postOf(f);
    if (!text) return;
    const parent = f.dataset.parent ? Number(f.dataset.parent) : null;
    input.disabled = true;
    try {
      const j = await api(`/posts/${post.dataset.post}/comments`, { text, parent });
      place(post.querySelector('.cm-list'), commentNode(j.comment), j.comment.parent);
      input.value = ''; delete f.dataset.parent; input.placeholder = 'Write a comment…'; setCount(post, j.n);
    } catch (err) { toast(err.message, true); }
    input.disabled = false; input.focus();
  });

  /* ---------- the post menu, sharing, photos ---------- */
  let menu = null;
  const closeMenu = () => { menu?.remove(); menu = null; };
  function openMenu(btn) {
    closeMenu();
    const post = postOf(btn), id = post.dataset.post;
    menu = el('div', 'pop post-pop'); menu.setAttribute('role', 'menu');
    const item = (label, fn, cls) => { const b = el('button', `row${cls ? ' ' + cls : ''}`); b.type = 'button'; b.append(el('span', 'row-t', label)); b.addEventListener('click', () => { closeMenu(); fn(); }); menu.append(b); };
    const kind = post.dataset.kind, written = kind === 'post' || kind === 'share', by = Number(post.dataset.member);
    const mine = !!me && by === me.id, vaults = full && by === 0;
    // Yours to edit: what you wrote, the post about your own listings, and (full admins) anything the vault posted.
    if ((mine && (written || kind === 'listing')) || vaults) item(written ? 'Edit post' : post.querySelector('.post-text') ? 'Edit the words on this post' : 'Add words to this post', () => compose('edit', post), mine ? '' : 'staff-only');
    if (mine && kind === 'listing') item('Manage my listings', () => { location.href = '/me/listings'; });
    if (full) item(post.hasAttribute('data-pinned') ? 'Unpin from the top' : 'Pin to the top of the feed', async () => { try { await api(`/posts/${id}/pin`, { on: !post.hasAttribute('data-pinned') }); location.reload(); } catch (e) { toast(e.message, true); } }, 'staff-only');
    item('Copy link', () => share(post, true));
    if (post.hasAttribute('data-own')) item('Delete post', () => remove(post, 'Delete this post for good?'));
    else if (staff) item(post.dataset.kind === 'post' || post.dataset.kind === 'share' ? 'Delete post (staff)' : 'Hide from the feed (staff)', () => remove(post, 'Take this post out of the feed?'), 'staff-only');
    // Full admins can stop a whole kind of automatic post from here (and switch it back on at /admin/look).
    if (full && !['post', 'share'].includes(post.dataset.kind)) item('Turn off automatic posts like this', async () => {
      if (!confirm('Stop showing this kind of automatic post in the feed? You can switch it back on at Admin \u203a The look.')) return;
      try { await api('/admin/feed/auto', { kind: post.dataset.kind, on: false }); location.reload(); } catch (e) { toast(e.message, true); }
    }, 'staff-only');
    btn.parentElement.append(menu);
  }
  async function remove(post, ask) {
    if (!confirm(ask)) return;
    try { await api(`/posts/${post.dataset.post}/delete`, {}); post.remove(); toast('Removed.'); } catch (e) { toast(e.message, true); }
  }
  async function share(post, copyOnly) {
    const url = `${location.origin}/posts/${post.dataset.post}`;
    if (!copyOnly && navigator.share) { try { await navigator.share({ url, title: document.title }); } catch (_) {} return; }
    try { await navigator.clipboard.writeText(url); toast('Link copied.'); } catch (_) { prompt('Copy this link:', url); }
  }
  function lightbox(src) {
    const o = el('div', 'dm-lightbox'); o.setAttribute('role', 'dialog'); o.setAttribute('aria-label', 'Photo');
    const img = el('img'); img.src = src; img.alt = 'Photo'; o.append(img);
    const x = el('button', 'dm-ib dm-lb-x'); x.type = 'button'; x.textContent = '✕'; x.setAttribute('aria-label', 'Close'); o.append(x);
    o.addEventListener('click', (e) => { if (e.target === o || e.target === x) o.remove(); });
    document.body.append(o); x.focus();
  }

  feed.addEventListener('click', (e) => {
    const t = e.target;
    const like = t.closest('[data-like]');
    if (like) { if (held) { held = false; return; } react(postOf(like), like.dataset.mine ? '' : 'like'); return; }
    if (t.closest('[data-comments-open]')) { openComments(postOf(t), true); return; }
    if (t.closest('[data-share]')) { share(postOf(t)); return; }
    const mb = t.closest('[data-post-menu]');
    if (mb) { e.stopPropagation(); menu ? closeMenu() : openMenu(mb); return; }
    const rep = t.closest('[data-reply]');
    if (rep) { const f = postOf(rep).querySelector('.cm-form'), input = f.querySelector('input'); f.dataset.parent = rep.dataset.reply; input.placeholder = `Reply to ${rep.dataset.name}…`; input.focus(); return; }
    const ced = t.closest('[data-cm-edit]');
    if (ced) {
      const node = ced.closest('.cm'), par = node.querySelector('.cm-bub p'), id = ced.dataset.cmEdit;
      if (node.querySelector('.cm-edit')) return;
      const input = el('input', 'cm-edit'); input.type = 'text'; input.maxLength = 600; input.value = par.textContent; input.setAttribute('aria-label', 'Edit your comment');
      par.hidden = true; par.after(input); input.focus();
      const done = () => { input.remove(); par.hidden = false; };
      input.addEventListener('keydown', async (ev) => {
        if (ev.key === 'Escape') { ev.stopPropagation(); done(); return; }
        if (ev.key !== 'Enter') return;
        ev.preventDefault();
        const text = input.value.trim(); if (!text) return;
        try { const j = await api(`/comments/${id}/edit`, { text }); const fresh = commentNode(j.comment); node.replaceWith(fresh); } catch (err) { toast(err.message, true); }
      });
      return;
    }
    const del = t.closest('[data-cm-del]');
    if (del) {
      if (!confirm('Delete this comment?')) return;
      const post = postOf(del), id = del.dataset.cmDel;
      api(`/comments/${id}/delete`, {}).then((j) => { post.querySelectorAll(`[data-cm="${id}"]`).forEach((n) => { let nx = n.nextElementSibling; while (nx && nx.classList.contains('reply') && !n.classList.contains('reply')) { const rm = nx; nx = nx.nextElementSibling; rm.remove(); } n.remove(); }); setCount(post, j.n); }).catch((err) => toast(err.message, true));
      return;
    }
    const ph = t.closest('[data-photo]');
    if (ph) { e.preventDefault(); lightbox(ph.getAttribute('href')); return; }
    const comp = t.closest('[data-compose]');
    if (comp) { compose(comp.dataset.compose); }
  });
  document.addEventListener('click', (e) => { if (menu && !e.target.closest('.post-pop')) closeMenu(); if (pick && !e.target.closest('.react-pick, [data-like]')) closePick(); });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape') { closeMenu(); closePick(); } });

  /* ---------- the post sheet: writing a new post, or editing one ---------- */
  let sheet = null, files = [], kept = [], editing = null;
  function compose(start, post) {
    if (!me) { toLogin(); return; }
    if (!sheet) {
      // Not a <dialog>: the camera opens above it, and a modal dialog would cover the camera.
      sheet = el('div', 'sheet'); sheet.hidden = true; sheet.setAttribute('role', 'dialog'); sheet.setAttribute('aria-modal', 'true');
      sheet.innerHTML = `<form class="sheet-in">
        <header class="sheet-h"><b data-title>Create post</b><button type="button" class="ibtn" data-x aria-label="Close">✕</button></header>
        <div class="row"><span class="av" data-face></span><span class="row-t"><b data-name></b><small data-sub>Everyone on the site can see this</small></span>
          <select data-as aria-label="Post as" hidden><option value="me">Post as me</option><option value="vault">Post as Big Boys Vault</option></select></div>
        <textarea maxlength="2000" placeholder="What did you pull today?" aria-label="Your post"></textarea>
        <div class="sheet-pics" data-pics></div>
        <div class="sheet-acts">
          <label class="btn ghost" data-pic-btn>Photos<input type="file" accept="image/*" multiple hidden data-add></label>
          <label class="btn ghost" data-pic-btn>Camera<input type="file" accept="image/*" capture="environment" data-card-cam="free" hidden data-add></label>
          <button type="submit" class="btn" data-go>Post</button>
        </div></form>`;
      document.body.append(sheet);
      const pics = sheet.querySelector('[data-pics]'), as = sheet.querySelector('[data-as]');
      const pic = (src, off) => { const d = el('div', 'sheet-pic'); const img = el('img'); img.src = src; img.alt = ''; const x = el('button', 'ibtn', '✕'); x.type = 'button'; x.setAttribute('aria-label', 'Remove this photo'); x.addEventListener('click', off); d.append(img, x); return d; };
      const paint = () => pics.replaceChildren(
        ...kept.map((k, i) => pic(k.src, () => { kept.splice(i, 1); paint(); })),
        ...files.map((f, i) => pic(URL.createObjectURL(f), () => { files.splice(i, 1); paint(); })));
      sheet.paint = paint;
      // Who the post is from: you, or (full admins) the vault itself, with its logo.
      sheet.who = () => {
        const vault = as.value === 'vault' && !as.hidden || (editing && editing.by === 0);
        const f = sheet.querySelector('[data-face]');
        if (vault) { const i = el('img'); i.src = '/static/img/logo-sm-144.png?v=1'; i.alt = ''; f.replaceChildren(i); } else face(f, me);
        sheet.querySelector('[data-name]').textContent = vault ? 'Big Boys Vault' : me.name;
      };
      as.addEventListener('change', sheet.who);
      sheet.querySelectorAll('[data-add]').forEach((inp) => inp.addEventListener('change', () => { for (const f of inp.files) { if (kept.length + files.length < 4) files.push(f); else toast('Four photos at most.'); } inp.value = ''; paint(); }));
      sheet.close = () => { sheet.hidden = true; document.documentElement.classList.remove('sheet-open'); };
      sheet.querySelector('[data-x]').addEventListener('click', () => sheet.close());
      sheet.addEventListener('click', (e) => { if (e.target === sheet) sheet.close(); });
      document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !sheet.hidden && !document.querySelector('.cc')) sheet.close(); });
      sheet.querySelector('form').addEventListener('submit', async (e) => {
        e.preventDefault();
        const ta = sheet.querySelector('textarea'), go = sheet.querySelector('[data-go]'), was = go.textContent;
        if (!editing && !ta.value.trim() && !files.length) { ta.focus(); return; }
        const fd = new FormData(); fd.append('text', ta.value); files.forEach((f) => fd.append('photo', f, f.name || 'photo.jpg'));
        if (editing) fd.append('keep', kept.map((k) => k.id).join(',')); else if (!as.hidden && as.value === 'vault') fd.append('as', 'vault');
        go.disabled = true; go.textContent = editing ? 'Saving…' : 'Posting…';
        try {
          const j = await api(editing ? `/posts/${editing.id}/edit` : '/posts', fd);
          const holder = el('div'); holder.innerHTML = j.html;
          const fresh = holder.querySelector('[data-post]');
          if (editing) { const old = feed.querySelector(`[data-post="${editing.id}"]`); if (old && fresh) old.replaceWith(fresh); toast('Saved.'); }
          else { const anchor = feed.querySelector('.composer'); if (fresh) anchor.after(fresh); toast('Posted.'); }
          ta.value = ''; files = []; kept = []; paint(); sheet.close();
        } catch (err) { toast(err.message, true); }
        go.disabled = false; go.textContent = was;
      });
    }
    const ta = sheet.querySelector('textarea'), as = sheet.querySelector('[data-as]');
    files = []; kept = [];
    if (start === 'edit' && post) {
      const kind = post.dataset.kind, written = kind === 'post' || kind === 'share';
      editing = { id: post.dataset.post, by: Number(post.dataset.member), written };
      ta.value = post.querySelector('.post-text')?.textContent || '';
      if (written) kept = [...post.querySelectorAll('[data-img]')].filter((a) => a.dataset.img !== '0').map((a) => ({ id: a.dataset.img, src: a.dataset.thumb || a.getAttribute('href') }));
      sheet.querySelector('[data-title]').textContent = written ? 'Edit post' : 'Words on this post';
      sheet.querySelector('[data-go]').textContent = 'Save';
      sheet.querySelector('[data-sub]').textContent = written ? 'Everyone on the site can see this' : 'The cards on this post stay as they are; you are adding your own words';
      ta.placeholder = written ? 'What did you pull today?' : 'Say something about these cards';
      sheet.querySelectorAll('[data-pic-btn]').forEach((b) => { b.hidden = !written; });
      as.hidden = true;
    } else {
      editing = null; ta.value = '';
      sheet.querySelector('[data-title]').textContent = 'Create post';
      sheet.querySelector('[data-go]').textContent = 'Post';
      sheet.querySelector('[data-sub]').textContent = 'Everyone on the site can see this';
      ta.placeholder = 'What did you pull today?';
      sheet.querySelectorAll('[data-pic-btn]').forEach((b) => { b.hidden = false; });
      as.hidden = !(full && me.role === 'admin'); as.value = 'me';
    }
    sheet.setAttribute('aria-label', sheet.querySelector('[data-title]').textContent);
    sheet.who(); sheet.paint();
    sheet.hidden = false; document.documentElement.classList.add('sheet-open');
    if (start === 'photo') sheet.querySelector('input[multiple]').click();
    else if (start === 'camera') sheet.querySelector('input[data-card-cam]').click();
    else ta.focus();
  }

  // A shared link opens one post with its comments showing.
  if (feed.hasAttribute('data-single')) { const p = feed.querySelector('[data-post]'); if (p && p.querySelector('[data-comments]')) who.then(() => openComments(p, false)); }
})();
