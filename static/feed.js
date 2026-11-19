/* The feed: likes and other reactions (on posts and on comments), comments (words, a photo, or both; replies to
   replies; your own to edit or delete), the photo window, sharing, the post menu, and writing a post.
   The posts themselves are drawn by the server; more arrive as you scroll (the last element asks for the next page).
   Everything here is one set of listeners on the feed and on the photo window (which holds a post while it is open),
   so posts that arrive later work without any setup. */
(() => {
  const feed = document.getElementById('feed');
  if (!feed) return;
  const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const REACTIONS = [['like', '👍', 'Like'], ['love', '❤️', 'Love'], ['fire', '🔥', 'Fire'], ['wow', '😮', 'Wow'], ['haha', '😆', 'Haha'], ['sad', '😢', 'Sad']];
  const THUMB = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 10v11H3V10zM7 10l4-8a2.5 2.5 0 0 1 2.5 3L13 9h6a2 2 0 0 1 2 2.3l-1.3 8A2 2 0 0 1 17.700 21H7"/></svg>';
  const SEND = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 12l16-8-6 16-3-7z"/></svg>';
  const PHOTO = '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg>';
  const CAMERA = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/></svg>';
  let me = null, staff = false, full = false;
  const who = window.bbvWho ? window.bbvWho() : Promise.resolve({});
  who.then((j) => {
    me = (j && j.member) || null; staff = !!(j && j.admin); full = !!(j && j.full);
    document.querySelectorAll('[data-my-face]').forEach((n) => face(n, me));
  });
  const face = (n, m) => { n.replaceChildren(); if (m && m.avatar) { const i = el('img'); i.src = m.avatar; i.alt = ''; n.append(i); } else n.textContent = m ? (m.initial || m.name[0].toUpperCase()) : ''; };
  const toast = (msg, bad) => { let t = document.querySelector('.cart-toast'); if (!t) { t = el('div', 'cart-toast'); t.setAttribute('role', 'status'); document.body.append(t); } t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on'); clearTimeout(toast.t); toast.t = setTimeout(() => t.classList.remove('on'), bad ? 5000 : 2600); };
  const toLogin = (why) => { try { sessionStorage.setItem('bbv:after-login', location.pathname + location.search); sessionStorage.setItem('bbv:login-why', why || 'react'); } catch (_) {} location.href = '/login'; };
  // "Are you sure?" in the site's own look (fb-shell.js), with the browser's box only if that script is missing.
  const sure = (q, opt) => (window.bbvConfirm ? window.bbvConfirm(q, opt) : Promise.resolve(confirm(q)));
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
  // A photo with its own likes and comments is drawn as a small post of its own (it says which photo: data-image).
  // What is liked or said there goes to that photo, not to the post it came with.
  const onPhoto = (post) => post.dataset.image || '';
  const qPhoto = (post) => (onPhoto(post) ? `?image=${onPhoto(post)}` : '');
  // The photo window (further down). A post moves into it while its photo is open, so everything below listens on the
  // feed and on the window alike.
  const pv = el('div', 'pv'); pv.hidden = true; pv.setAttribute('role', 'dialog'); pv.setAttribute('aria-modal', 'true'); pv.setAttribute('aria-label', 'Photo');
  pv.innerHTML = '<div class="pv-stage"><img class="pv-img" alt="Photo"><button type="button" class="pv-nav pv-prev" aria-label="The photo before"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 5l-7 7 7 7"/></svg></button><button type="button" class="pv-nav pv-next" aria-label="The next photo"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 5l7 7-7 7"/></svg></button><span class="pv-count" aria-live="polite"></span></div><div class="pv-side"></div><button type="button" class="pv-x" aria-label="Close"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg></button>';
  document.body.append(pv);
  const listen = (type, fn) => { feed.addEventListener(type, fn); pv.addEventListener(type, fn); };

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
    try { paintLike(post, await api(`/posts/${post.dataset.post}/react`, onPhoto(post) ? { kind, image: Number(onPhoto(post)) } : { kind })); } catch (e) { toast(e.message, true); }
  }
  // A comment's Like: the same six reactions as a post; how many sits beside it, and its writer hears.
  function paintCmLike(node, j) {
    const b = node.querySelector('[data-cm-like]'); if (!b) return;
    b.dataset.mine = j.mine || ''; b.setAttribute('aria-pressed', String(!!j.mine)); b.classList.toggle('on', !!j.mine);
    b.textContent = j.mine ? `${j.emoji} ${(REACTIONS.find((r) => r[0] === j.mine) || [])[2] || 'Like'}` : 'Like';
    const chip = node.querySelector('.cm-reacts');
    if (chip) { chip.hidden = !j.likes; chip.replaceChildren(...(j.tops || []).map((e) => el('i', '', e)), el('span', '', String(j.likes || 0))); }
  }
  async function reactComment(node, kind) {
    closePick();
    if (!me) { toLogin(); return; }
    try { paintCmLike(node, await api(`/comments/${node.dataset.cm}/react`, { kind })); } catch (e) { toast(e.message, true); }
  }
  let pick = null, pickTimer = 0;
  const closePick = () => { clearTimeout(pickTimer); pick?.remove(); pick = null; };
  // `onPick(kind)` is what a reaction does: a post's Like, or a comment's.
  function openPick(btn, onPick) {
    closePick();
    pick = el('div', 'react-pick'); pick.setAttribute('role', 'menu');
    REACTIONS.forEach(([k, e, label]) => { const b = el('button', '', e); b.type = 'button'; b.title = label; b.setAttribute('aria-label', label); b.addEventListener('click', (ev) => { ev.stopPropagation(); onPick(k); }); pick.append(b); });
    btn.parentElement.append(pick);
    const r = btn.getBoundingClientRect(), host = btn.parentElement.getBoundingClientRect();
    // Under the button, kept 8px inside the screen on both sides.
    const room = document.documentElement.clientWidth - 8 - pick.offsetWidth;
    pick.style.left = `${Math.max(8, Math.min(r.left, room)) - host.left}px`;
  }
  const LIKES = '[data-like], [data-cm-like]';
  const pickFor = (b) => (b.hasAttribute('data-cm-like') ? (k) => reactComment(b.closest('.cm'), k) : (k) => react(postOf(b), k));
  // A mouse resting on Like, or a finger held on it, opens the other reactions.
  listen('pointerover', (e) => { const b = e.target.closest(LIKES); if (!b || e.pointerType !== 'mouse' || pick) return; clearTimeout(pickTimer); pickTimer = setTimeout(() => openPick(b, pickFor(b)), 450); });
  listen('pointerout', (e) => { const b = e.target.closest(`${LIKES}, .react-pick`); if (!b || e.pointerType !== 'mouse') return; clearTimeout(pickTimer); if (pick && !e.relatedTarget?.closest?.(`${LIKES}, .react-pick`)) pickTimer = setTimeout(closePick, 350); });
  let held = false;
  listen('pointerdown', (e) => { const b = e.target.closest(LIKES); if (!b || e.pointerType === 'mouse') return; held = false; clearTimeout(pickTimer); pickTimer = setTimeout(() => { held = true; openPick(b, pickFor(b)); if (navigator.vibrate) navigator.vibrate(8); }, 420); });
  listen('pointerup', () => { if (!held) clearTimeout(pickTimer); });
  listen('contextmenu', (e) => { if (e.target.closest(LIKES)) e.preventDefault(); });

  /* ---------- comments ---------- */
  // One comment: its writer and when, its words and photo, then Like, Reply and (your own) Edit and Delete, each big
  // enough to tap and always on one row, however far in the reply sits.
  // The replies to it sit under it (.cm-kids), each a comment of its own that can be answered in turn.
  function commentNode(k) {
    const li = el('div', 'cm'); li.dataset.cm = k.id;
    const row = el('div', 'cm-row');
    const av = el('a', 'av sm'); av.href = `/trainers/${k.member}`; face(av, { avatar: k.avatar, initial: k.initial, name: k.name });
    const body = el('div', 'cm-body');
    const bub = el('div', 'cm-bub'); const name = el('a', 'cm-name', k.name); name.href = `/trainers/${k.member}`;
    const head = el('div', 'cm-head'); head.append(name, el('span', 'cm-when', k.edited ? `${k.when} · Edited` : k.when));
    bub.append(head);
    if (k.text) bub.append(el('p', '', k.text));
    if (k.image) { const a = el('a', 'cm-photo'); a.href = k.image; a.dataset.photo = ''; const im = el('img'); im.src = k.thumb || k.image; im.alt = 'Photo'; im.loading = 'lazy'; a.append(im); bub.append(a); }
    const meta = el('div', 'cm-meta');
    const act = (words, key, val) => { const b = el('button', 'linkbtn cm-act', words); b.type = 'button'; b.dataset[key] = val; meta.append(b); return b; };
    act('Like', 'cmLike', '').classList.add('cm-like');
    act('Reply', 'reply', k.id).dataset.name = k.name;
    if (k.own) act('Edit', 'cmEdit', k.id);
    if (k.can_remove) { const d = act('Delete', 'cmDel', k.id); if (!k.own) d.classList.add('staff-only'); }
    if (me && !k.own) act('Report', 'cmReport', k.id);
    meta.append(el('span', 'cm-reacts'));
    body.append(bub, meta); row.append(av, body);
    li.append(row, el('div', 'cm-kids'));
    paintCmLike(li, k);
    return li;
  }
  async function openComments(post, focus) {
    const box = post.querySelector('[data-comments]');
    if (!box.dataset.ready) {
      box.dataset.ready = '1'; box.hidden = false;
      const list = el('div', 'cm-list'); box.append(list);
      const form = el('form', 'cm-form');
      form.innerHTML = `<span class="av sm" data-my-face></span><span class="cm-in"><span class="cm-to-row" hidden><span>Replying to <b></b></span><button type="button" class="cm-to-x" aria-label="Cancel the reply">✕</button></span><input type="text" maxlength="600" placeholder="Write a comment…" aria-label="Write a comment" autocomplete="off" enterkeyhint="send"><span class="cm-pic" hidden><img alt=""><button type="button" class="cm-pic-x" aria-label="Take the photo off">✕</button></span></span>`
        + `<label class="ibtn cm-add" title="Add a photo" aria-label="Add a photo">${PHOTO}<input type="file" accept="image/*" hidden data-cm-file></label>`
        + `<label class="ibtn cm-add" title="Take a photo" aria-label="Take a photo">${CAMERA}<input type="file" accept="image/*" capture="environment" data-card-cam="free" hidden data-cm-file></label>`
        + `<button type="submit" class="ibtn" aria-label="Send">${SEND}</button>`;
      form.querySelectorAll('[data-cm-file]').forEach((inp) => inp.addEventListener('change', () => { const f = inp.files && inp.files[0]; inp.value = ''; if (f) setCmPhoto(form, f); }));
      form.querySelector('.cm-pic-x').addEventListener('click', () => setCmPhoto(form, null));
      form.querySelector('.cm-to-x').addEventListener('click', () => { replyTo(form, null); form.querySelector('input[type="text"]').focus(); });
      face(form.querySelector('[data-my-face]'), me); box.append(form);
      try { const j = await api(`/posts/${post.dataset.post}/comments${qPhoto(post)}`); j.comments.forEach((k) => place(list, commentNode(k), k.parent)); } catch (e) { toast(e.message, true); }
    } else box.hidden = false;
    if (focus) box.querySelector('.cm-form input[type="text"]').focus({ preventScroll: false });
  }
  // A reply sits under the comment it answers, after the replies already there. A reply to a reply says whose it
  // answers: deep down the threads stop stepping further in, so the name is what tells them apart.
  function place(list, node, parent) {
    const top = parent ? list.querySelector(`.cm[data-cm="${parent}"]`) : null;
    if (!top) { list.append(node); return; }
    if (top.parentElement.classList.contains('cm-kids')) node.querySelector('.cm-name').after(el('span', 'cm-to', `to ${top.querySelector('.cm-name').textContent}`));
    top.querySelector(':scope > .cm-kids').append(node);
  }
  // Who the comment box answers: a comment (its writer's name shows above the box, with ✕ to stop), or the post.
  function replyTo(form, id, name) {
    const row = form.querySelector('.cm-to-row'), input = form.querySelector('input[type="text"]');
    if (id) { form.dataset.parent = id; row.querySelector('b').textContent = name; row.hidden = false; input.placeholder = `Reply to ${name}…`; }
    else { delete form.dataset.parent; row.hidden = true; input.placeholder = 'Write a comment…'; }
  }
  const setCount = (post, n) => { const b = post.querySelector('.post-cn'); b.hidden = !n; b.textContent = `${n} comment${n === 1 ? '' : 's'}`; stats(post); };
  // The photo waiting to go with a comment: shown small in the box, with ✕ to take it off.
  function setCmPhoto(form, file) {
    const box = form.querySelector('.cm-pic'), img = box.querySelector('img');
    if (form.cmPhotoUrl) URL.revokeObjectURL(form.cmPhotoUrl);
    form.cmPhoto = file || null; form.cmPhotoUrl = file ? URL.createObjectURL(file) : '';
    img.src = form.cmPhotoUrl; box.hidden = !file;
    if (file) form.querySelector('input[type="text"]').focus({ preventScroll: true });
  }
  listen('submit', async (e) => {
    const f = e.target.closest('.cm-form'); if (!f) return;
    e.preventDefault();
    if (!me) { toLogin(); return; }
    const input = f.querySelector('input[type="text"]'), text = input.value.trim(), post = postOf(f);
    if (!text && !f.cmPhoto) return;
    const fd = new FormData(); fd.append('text', text);
    if (f.dataset.parent) fd.append('parent', f.dataset.parent);
    if (f.cmPhoto) fd.append('photo', f.cmPhoto, f.cmPhoto.name || 'photo.jpg');
    if (onPhoto(post)) fd.append('image', onPhoto(post));
    input.disabled = true;
    try {
      const j = await api(`/posts/${post.dataset.post}/comments`, fd);
      place(post.querySelector('.cm-list'), commentNode(j.comment), j.comment.parent);
      input.value = ''; replyTo(f, null); setCmPhoto(f, null); setCount(post, j.n);
      post.querySelector(`.cm[data-cm="${j.comment.id}"]`)?.scrollIntoView({ block: 'nearest' });
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
    if ((mine && (written || kind === 'listing')) || vaults) item(written ? 'Edit post' : post.querySelector('.post-text') ? 'Edit the words on this post' : 'Add words to this post', () => { closeView(); compose('edit', post); }, mine ? '' : 'staff-only');
    if (mine && kind === 'listing') item('Manage my listings', () => { location.href = '/me/listings'; });
    if (full) item(post.hasAttribute('data-pinned') ? 'Unpin from the top' : 'Pin to the top of the feed', async () => { try { await api(`/posts/${id}/pin`, { on: !post.hasAttribute('data-pinned') }); location.reload(); } catch (e) { toast(e.message, true); } }, 'staff-only');
    item('Copy link', () => share(post, true));
    if (me && !mine && written && by !== 0) item('Report post', () => report('post', id));
    if (post.hasAttribute('data-own')) item('Delete post', () => remove(post, 'Delete this post for good?'));
    else if (staff) item(post.dataset.kind === 'post' || post.dataset.kind === 'share' ? 'Delete post (staff)' : 'Hide from the feed (staff)', () => remove(post, 'Take this post out of the feed?'), 'staff-only');
    // Full admins can stop a whole kind of automatic post from here (and switch it back on at /admin/look).
    if (full && !['post', 'share'].includes(post.dataset.kind)) item('Turn off automatic posts like this', async () => {
      if (!(await sure('Stop showing this kind of automatic post in the feed? You can switch it back on at Admin \u203a The look.', { yes: 'Stop showing them', no: 'Keep them' }))) return;
      try { await api('/admin/feed/auto', { kind: post.dataset.kind, on: false }); location.reload(); } catch (e) { toast(e.message, true); }
    }, 'staff-only');
    btn.parentElement.append(menu);
  }
  // A post or a comment that breaks the rules goes to the vault's team (once each; they decide).
  async function report(kind, id) {
    if (!me) { toLogin('react'); return; }
    if (!(await sure(`Report this ${kind} to the vault’s team? They look at it and take it down if it breaks the rules.`, { yes: 'Report', no: 'Cancel' }))) return;
    try { await api(`/${kind === 'post' ? 'posts' : 'comments'}/${id}/report`, {}); toast('Reported ✓ The vault’s team will look at it.'); } catch (e) { toast(e.message, true); }
  }
  async function remove(post, ask) {
    if (!(await sure(ask, { yes: 'Delete', no: 'Keep it' }))) return;
    try { await api(`/posts/${post.dataset.post}/delete`, {}); closeView(); post.remove(); toast('Removed.'); } catch (e) { toast(e.message, true); }
  }
  async function share(post, copyOnly) {
    // A photo with its own likes and comments is shared as itself: the link opens the post on that photo.
    const url = `${location.origin}/posts/${post.dataset.post}${onPhoto(post) ? `?photo=${onPhoto(post)}` : ''}`, what = onPhoto(post) ? 'photo' : 'post';
    // Share opens the site's share window: a message here, Messenger, Facebook or the link (share.js).
    if (!copyOnly && window.bbvShare) { const who = post.querySelector('.post-who b')?.textContent.trim(); window.bbvShare.open({ ref: `post:${post.dataset.post}`, url, title: who ? `${who}’s ${what}` : `A ${what}` }); return; }
    // The phone's own share menu where there is one. Closing it is fine; if it can't open, the link is copied instead.
    if (!copyOnly && navigator.share) { try { await navigator.share({ url, title: document.title }); return; } catch (err) { if (err && err.name === 'AbortError') return; } }
    try { await navigator.clipboard.writeText(url); toast('Link copied.'); } catch (_) { toast(`Copy this link: ${url}`); }
  }
  // A comment's photo, big: it fits the screen whatever its shape, and ✕ (or a tap beside it, or Esc) closes it.
  function lightbox(src) {
    const o = el('div', 'dm-lightbox'); o.setAttribute('role', 'dialog'); o.setAttribute('aria-label', 'Photo');
    const img = el('img'); img.src = src; img.alt = 'Photo'; o.append(img);
    const x = el('button', 'dm-ib dm-lb-x'); x.type = 'button'; x.textContent = '✕'; x.setAttribute('aria-label', 'Close'); o.append(x);
    const back = document.activeElement;
    const close = () => { o.remove(); document.removeEventListener('keydown', esc); if (back && back.isConnected) back.focus({ preventScroll: true }); };
    const esc = (ev) => { if (ev.key === 'Escape') close(); };
    o.addEventListener('click', (e) => { if (e.target === o || e.target === x) close(); });
    document.addEventListener('keydown', esc);
    document.body.append(o); x.focus();
  }

  /* ---------- the photo window: a post's photo big, with the post beside it ---------- */
  // Like a photo on Facebook (the owner, 19 Nov 2026): the photo fits the screen whatever its shape, ✕ closes it (so do
  // Esc and the phone's Back), arrows (or a swipe, or the arrow keys) go through the post's photos, and the post sits
  // beside it (under it on a phone or an upright tablet) with its likes, its comments and the comment box, photo and
  // camera buttons included. It is the real post, moved here while the window is open: a like or a comment made here
  // is on the post in the feed.
  //
  // When a post a person wrote has two or more photos, each photo has likes and comments of its own (the owner,
  // 19 Nov 2026: "we should be able to have unique comment,react, etc. per photo, not just show the entire post, this
  // doesn't apply to card listings etc. just organic posts"). Then the post stays where it is and what sits beside
  // the photo is that photo's own small post, fetched from the server and swapped as the arrows go through them. A
  // post's only photo is the post itself, as on Facebook.
  const view = { post: null, hold: null, pics: [], ids: [], at: 0, back: null, pushed: false, own: false };
  let panelSeq = 0;
  async function loadPanel() {
    const side = pv.querySelector('.pv-side'), seq = ++panelSeq, id = view.post.dataset.post, img = view.ids[view.at];
    closeMenu(); closePick();
    try {
      const j = await api(`/posts/${id}/photos/${img}`);
      if (seq !== panelSeq || !view.post) return;
      const holder = el('div'); holder.innerHTML = j.html;
      const fresh = holder.querySelector('[data-post]');
      if (!fresh) throw new Error('That photo isn’t here any more.');
      fresh.classList.add('pv-post'); side.replaceChildren(fresh);
      openComments(fresh, false);
    } catch (e) { if (seq === panelSeq && view.post) side.replaceChildren(el('p', 'pv-gone', e.message)); }
  }
  function showPic() {
    const n = view.pics.length;
    pv.querySelector('.pv-img').src = view.pics[view.at];
    pv.querySelectorAll('.pv-nav').forEach((b) => { b.hidden = n < 2; });
    const count = pv.querySelector('.pv-count'); count.hidden = n < 2; count.textContent = `${view.at + 1} of ${n}`;
    if (view.own) loadPanel();
  }
  const step = (by) => { const n = view.pics.length; if (n < 2) return; view.at = (view.at + by + n) % n; showPic(); };
  function openView(link) {
    const post = postOf(link); if (!post || view.post) return;
    const links = [...post.querySelectorAll('.post-media [data-photo]')];
    view.pics = links.map((a) => a.getAttribute('href'));
    view.ids = links.map((a) => a.dataset.img);
    view.at = Math.max(0, view.pics.indexOf(link.getAttribute('href')));
    view.back = link;
    view.own = !!link.closest('[data-photo-threads]');
    if (view.own) view.hold = null;
    else {
      // The post's place in the feed is kept, so the feed doesn't jump while it is away.
      view.hold = el('div', 'pv-hold'); view.hold.style.height = `${post.offsetHeight}px`;
      post.before(view.hold);
      pv.querySelector('.pv-side').append(post); post.classList.add('pv-post');
    }
    view.post = post;
    pv.hidden = false; pv.scrollTop = 0; document.documentElement.classList.add('pv-open');
    showPic();
    if (!view.own && post.querySelector('[data-comments]')) openComments(post, false);
    try { history.pushState({ bbvPhoto: 1 }, ''); view.pushed = true; } catch (_) { view.pushed = false; }
    pv.querySelector('.pv-x').focus({ preventScroll: true });
  }
  // `byBack`: the phone's Back (or the browser's) closed it, so the history is already where it should be.
  function closeView(byBack) {
    const post = view.post; if (!post) return;
    closeMenu(); closePick();
    if (view.own) { panelSeq += 1; pv.querySelector('.pv-side').replaceChildren(); }
    else {
      post.classList.remove('pv-post');
      if (view.hold.isConnected) view.hold.replaceWith(post); else post.remove();
    }
    pv.hidden = true; document.documentElement.classList.remove('pv-open');
    pv.querySelector('.pv-img').removeAttribute('src');
    const back = view.back, pushed = view.pushed;
    view.post = null; view.hold = null; view.pushed = false; view.own = false;
    if (back && back.isConnected) back.focus({ preventScroll: true });
    if (pushed && !byBack) history.back();
  }
  addEventListener('popstate', () => closeView(true));
  pv.querySelector('.pv-x').addEventListener('click', () => closeView());
  pv.querySelector('.pv-prev').addEventListener('click', () => step(-1));
  pv.querySelector('.pv-next').addEventListener('click', () => step(1));
  // A finger swiping across the photo goes to the next or the one before.
  let swipe = null;
  const stage = pv.querySelector('.pv-stage');
  stage.addEventListener('pointerdown', (e) => { swipe = e.pointerType === 'mouse' ? null : { x: e.clientX, y: e.clientY }; });
  stage.addEventListener('pointerup', (e) => {
    if (!swipe) return;
    const dx = e.clientX - swipe.x, dy = e.clientY - swipe.y; swipe = null;
    if (Math.abs(dx) > 48 && Math.abs(dx) > Math.abs(dy) * 1.5) step(dx < 0 ? 1 : -1);
  });
  stage.addEventListener('pointercancel', () => { swipe = null; });

  listen('click', (e) => {
    const t = e.target;
    const like = t.closest('[data-like]');
    if (like) { if (held) { held = false; return; } react(postOf(like), like.dataset.mine ? '' : 'like'); return; }
    const cml = t.closest('[data-cm-like]');
    if (cml) { if (held) { held = false; return; } reactComment(cml.closest('.cm'), cml.dataset.mine ? '' : 'like'); return; }
    if (t.closest('[data-comments-open]')) { openComments(postOf(t), true); return; }
    // The faces and number under a post: who reacted.
    const rx = t.closest('[data-reacts]');
    if (rx && window.bbvPeople) { window.bbvPeople('Reactions', `/posts/${postOf(rx).dataset.post}/reactions${qPhoto(postOf(rx))}`); return; }
    const crp = t.closest('[data-cm-report]');
    if (crp) { report('comment', crp.dataset.cmReport); return; }
    if (t.closest('[data-share]')) { share(postOf(t)); return; }
    const mb = t.closest('[data-post-menu]');
    if (mb) { e.stopPropagation(); menu ? closeMenu() : openMenu(mb); return; }
    const rep = t.closest('[data-reply]');
    if (rep) {
      if (!me) { toLogin(); return; }
      const f = postOf(rep).querySelector('.cm-form'), input = f.querySelector('input[type="text"]');
      replyTo(f, rep.dataset.reply, rep.dataset.name); input.focus(); f.scrollIntoView({ block: 'nearest' });
      return;
    }
    // Your own comment's words, changed where they are: a box with Save and Cancel (Enter saves, Esc cancels).
    const ced = t.closest('[data-cm-edit]');
    if (ced) {
      const node = ced.closest('.cm'), row = node.querySelector(':scope > .cm-row'), id = ced.dataset.cmEdit, bub = row.querySelector('.cm-bub');
      if (bub.querySelector('.cm-edit')) return;
      const par = bub.querySelector('p');
      const box = el('form', 'cm-edit');
      const input = el('input'); input.type = 'text'; input.maxLength = 600; input.value = par ? par.textContent : ''; input.setAttribute('aria-label', 'Edit your comment'); input.enterKeyHint = 'done'; input.autocomplete = 'off';
      const save = el('button', 'btn sm', 'Save'); save.type = 'submit';
      const no = el('button', 'btn ghost sm', 'Cancel'); no.type = 'button';
      box.append(input, save, no);
      if (par) par.hidden = true;
      (par || bub.querySelector('.cm-head')).after(box); input.focus();
      const done = () => { box.remove(); if (par) par.hidden = false; ced.focus({ preventScroll: true }); };
      no.addEventListener('click', done);
      input.addEventListener('keydown', (ev) => { if (ev.key === 'Escape') { ev.preventDefault(); ev.stopPropagation(); done(); } });
      box.addEventListener('submit', async (ev) => {
        ev.preventDefault(); ev.stopPropagation();
        const text = input.value.trim(); if (!text) { input.focus(); return; }
        save.disabled = true;
        try {
          const j = await api(`/comments/${id}/edit`, { text });
          // Its row is drawn again; the replies under it stay where they are.
          const to = row.querySelector('.cm-to'), fresh = commentNode(j.comment).querySelector('.cm-row');
          if (to) fresh.querySelector('.cm-name').after(to);
          row.replaceWith(fresh); toast('Comment saved.');
        } catch (err) { toast(err.message, true); save.disabled = false; }
      });
      return;
    }
    const del = t.closest('[data-cm-del]');
    if (del) {
      const post = postOf(del), id = del.dataset.cmDel, node = del.closest('.cm'), under = node.querySelectorAll('.cm-kids .cm').length;
      const ask = under ? `Delete this comment and the ${under === 1 ? 'reply' : `${under} replies`} under it?` : 'Delete this comment?';
      sure(ask, { yes: 'Delete', no: 'Keep it' }).then((yes) => yes && api(`/comments/${id}/delete`, {}).then((j) => { node.remove(); setCount(post, j.n); toast('Comment deleted.'); }).catch((err) => toast(err.message, true)));
      return;
    }
    // A post's photo opens the photo window; a comment's photo (and any photo while that window is open) opens big.
    const ph = t.closest('[data-photo]');
    if (ph) { e.preventDefault(); if (ph.closest('.post-media') && !view.post) openView(ph); else lightbox(ph.getAttribute('href')); return; }
    const comp = t.closest('[data-compose]');
    if (comp) { compose(comp.dataset.compose); }
  });
  document.addEventListener('click', (e) => { if (menu && !e.target.closest('.post-pop')) closeMenu(); if (pick && !e.target.closest('.react-pick, [data-like]')) closePick(); });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      if (menu || pick) { closeMenu(); closePick(); return; }
      // The photo window closes last: a bigger photo, a question or the post sheet open over it close first.
      if (view.post && !document.querySelector('.dm-lightbox, .bbv-ask, .cc') && (!sheet || sheet.hidden)) closeView();
      return;
    }
    if (view.post && (e.key === 'ArrowLeft' || e.key === 'ArrowRight') && !e.target.closest('input, textarea, select')) step(e.key === 'ArrowLeft' ? -1 : 1);
  });
  listen('keydown', (e) => { if ((e.key === 'Enter' || e.key === ' ') && e.target.matches && e.target.matches('[data-reacts]')) { e.preventDefault(); e.target.click(); } });

  /* ---------- the words of a post: bold, italic, underline and three sizes ---------- */
  // The owner, 19 Nov 2026: "give the option to bold, change font size in 3 levels of any text within a post".
  // The box is a page the browser lets you type in. What it holds is read back as pieces of words, each with its
  // style, and only those pieces are sent: the server tidies them and draws the post from them (rich.rs), so nothing
  // typed or pasted here is ever kept as HTML.
  const RT_MAX = 2000;
  function runsOf(ed) {
    const base = parseFloat(getComputedStyle(ed).fontSize) || 16, out = [], none = { b: false, i: false, u: false, s: '' };
    const push = (t, f) => { if (!t) return; const last = out[out.length - 1]; if (last && last.b === f.b && last.i === f.i && last.u === f.u && last.s === f.s) last.t += t; else out.push({ t, b: f.b, i: f.i, u: f.u, s: f.s }); };
    const ended = () => !out.length || out[out.length - 1].t.endsWith('\n');
    const styleOf = (node) => {
      if (node === ed) return none;
      const cs = getComputedStyle(node), w = cs.fontWeight, ratio = parseFloat(cs.fontSize) / base;
      let u = false; for (let n = node; n && n !== ed; n = n.parentElement) { if ((getComputedStyle(n).textDecorationLine || '').includes('underline')) { u = true; break; } }
      return { b: w === 'bold' || Number(w) >= 600, i: cs.fontStyle === 'italic' || cs.fontStyle === 'oblique', u, s: ratio < 0.93 ? 's' : ratio > 1.12 ? 'l' : '' };
    };
    const walk = (node) => { for (const n of node.childNodes) {
      if (n.nodeType === 3) push(n.nodeValue.replace(/ /g, ' '), styleOf(n.parentElement));
      else if (n.nodeType === 1) {
        if (n.tagName === 'BR') { push('\n', none); continue; }
        const block = /^(DIV|P|LI|UL|OL|H[1-6]|BLOCKQUOTE|PRE)$/.test(n.tagName);
        if (block && !ended()) push('\n', none);
        walk(n);
        if (block && !ended()) push('\n', none);
      } } };
    walk(ed);
    return out;
  }
  // The writing box and its row of styles. `el` is the box; `bar` the row; `runs()` what was written.
  function wordsBox(ed, bar, onChange) {
    const cmd = (name, value) => { try { document.execCommand('styleWithCSS', false, false); return document.execCommand(name, false, value); } catch (e) { return false; } };
    const inside = () => { const sel = document.getSelection(); return !!sel && sel.rangeCount > 0 && ed.contains(sel.anchorNode); };
    let pending = null;
    const sizeAt = () => {
      const sel = document.getSelection(); if (!inside()) return '';
      if (pending && sel.isCollapsed && pending.node === sel.focusNode && pending.at === sel.focusOffset) return pending.s;
      let n = sel.focusNode; if (n && n.nodeType === 3) n = n.parentElement;
      if (!n || !ed.contains(n) || n === ed) return '';
      const ratio = parseFloat(getComputedStyle(n).fontSize) / (parseFloat(getComputedStyle(ed).fontSize) || 16);
      return ratio < 0.93 ? 's' : ratio > 1.12 ? 'l' : '';
    };
    const paint = () => {
      ed.classList.toggle('is-empty', !ed.textContent.trim());
      const live = inside();
      bar.querySelectorAll('[data-cmd]').forEach((b) => { let on = false; try { on = live && document.queryCommandState(b.dataset.cmd); } catch (e) { on = false; } b.classList.toggle('on', on); b.setAttribute('aria-pressed', on ? 'true' : 'false'); });
      const size = live ? sizeAt() : '';
      bar.querySelectorAll('[data-size]').forEach((b) => { const on = b.dataset.size === size; b.classList.toggle('on', on); b.setAttribute('aria-pressed', on ? 'true' : 'false'); });
    };
    const count = () => {
      const n = [...runsOf(ed).map((r) => r.t).join('').trim()].length, out = bar.querySelector('[data-count]');
      out.hidden = n < RT_MAX - 200; out.textContent = `${n.toLocaleString()} of ${RT_MAX.toLocaleString()}`; out.classList.toggle('over', n > RT_MAX);
    };
    // A tap on a style button must not take the cursor out of the words.
    ['mousedown', 'pointerdown'].forEach((t) => bar.addEventListener(t, (e) => { if (e.target.closest('button')) e.preventDefault(); }));
    bar.addEventListener('click', (e) => {
      const b = e.target.closest('button'); if (!b || b.disabled) return;
      if (!inside()) { ed.focus(); const r = document.createRange(); r.selectNodeContents(ed); r.collapse(false); const sel = document.getSelection(); sel.removeAllRanges(); sel.addRange(r); }
      if (b.dataset.cmd) cmd(b.dataset.cmd);
      else if ('size' in b.dataset) {
        cmd('fontSize', b.dataset.size === 's' ? '2' : b.dataset.size === 'l' ? '5' : '3');
        const sel = document.getSelection(); pending = sel && sel.isCollapsed ? { node: sel.focusNode, at: sel.focusOffset, s: b.dataset.size } : null;
      }
      paint(); count(); if (onChange) onChange();
    });
    ed.addEventListener('input', () => { pending = null; paint(); count(); if (onChange) onChange(); });
    document.addEventListener('selectionchange', () => { if (!ed.isConnected || ed.closest('[hidden]')) return; paint(); });
    // Pasted or dropped words come in plain: no pictures, no outside styles.
    ed.addEventListener('paste', (e) => { e.preventDefault(); const t = (e.clipboardData || window.clipboardData)?.getData('text/plain') || ''; if (t) cmd('insertText', t.replace(/\r\n?/g, '\n')); });
    ed.addEventListener('drop', (e) => { e.preventDefault(); const t = e.dataTransfer?.getData('text/plain') || ''; if (t) { ed.focus(); cmd('insertText', t.replace(/\r\n?/g, '\n')); } });
    return {
      el: ed,
      runs: () => runsOf(ed),
      text: () => runsOf(ed).map((r) => r.t).join('').trim(),
      // `html` is a post's words as the server drew them (already safe), or '' for an empty box.
      set(html) { ed.innerHTML = html || ''; pending = null; paint(); count(); },
      off(is) { ed.contentEditable = is ? 'false' : 'true'; ed.classList.toggle('is-off', is); bar.querySelectorAll('button').forEach((b) => { b.disabled = is; }); },
      focus() { ed.focus(); const r = document.createRange(); r.selectNodeContents(ed); r.collapse(false); const sel = document.getSelection(); sel.removeAllRanges(); sel.addRange(r); },
    };
  }

  /* ---------- the post sheet: writing a new post, or editing one ---------- */
  // Laid out the way Facebook's is (the owner, 19 Nov 2026: "the space for typing is too small and the font is too
  // big ... take inspo from how Facebook does it"): who is posting, a tall box to type in (the whole screen on a
  // phone) in the size the post will be read in, the styles, "Add To Your Post", and one wide Post button. On a
  // phone Post sits in the top bar and the styles and photo buttons share one row that stays just above the keyboard.
  let sheet = null, files = [], kept = [], editing = null, words = null;
  function compose(start, post) {
    if (!me) { toLogin('post'); return; }
    if (!sheet) {
      // Not a <dialog>: the camera opens above it, and a modal dialog would cover the camera.
      sheet = el('div', 'sheet'); sheet.hidden = true; sheet.setAttribute('role', 'dialog'); sheet.setAttribute('aria-modal', 'true');
      sheet.innerHTML = `<form class="sheet-in sheet-post">
        <header class="sheet-h"><button type="button" class="ibtn" data-x aria-label="Close">✕</button><b data-title>Create Post</b><button type="submit" class="btn sheet-go-top" data-go>Post</button></header>
        <div class="row"><span class="av" data-face></span><span class="row-t"><b data-name></b><small data-sub>Everyone on the site can see this</small></span>
          <select data-as aria-label="Post As" hidden><option value="me">Post As Me</option><option value="vault">Post As Big Boys Vault</option></select></div>
        <div class="rt is-empty" data-ed contenteditable="true" role="textbox" aria-multiline="true" aria-label="Your post" data-ph="What did you pull today?" spellcheck="true"></div>
        <div class="sheet-pics" data-pics></div>
        <div class="sheet-tools">
        <div class="rt-bar" data-bar role="toolbar" aria-label="Text Style">
          <button type="button" class="rt-b" data-cmd="bold" aria-label="Bold" title="Bold"><b>B</b></button>
          <button type="button" class="rt-b" data-cmd="italic" aria-label="Italic" title="Italic"><i>I</i></button>
          <button type="button" class="rt-b" data-cmd="underline" aria-label="Underline" title="Underline"><u>U</u></button>
          <span class="rt-sep" aria-hidden="true"></span>
          <button type="button" class="rt-b rt-a1" data-size="s" aria-label="Small Text" title="Small Text">A</button>
          <button type="button" class="rt-b rt-a2" data-size="" aria-label="Normal Text" title="Normal Text">A</button>
          <button type="button" class="rt-b rt-a3" data-size="l" aria-label="Large Text" title="Large Text">A</button>
          <small class="rt-count" data-count hidden></small>
        </div>
        <div class="sheet-add"><span>Add To Your Post</span>
          <label class="btn ghost" data-pic-btn><svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg><span>Photos</span><input type="file" accept="image/*" multiple hidden data-add></label>
          <label class="btn ghost" data-pic-btn><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/></svg><span>Camera</span><input type="file" accept="image/*" capture="environment" data-card-cam="free" hidden data-add></label>
        </div>
        </div>
        <button type="submit" class="btn sheet-go" data-go>Post</button></form>`;
      document.body.append(sheet);
      const pics = sheet.querySelector('[data-pics]'), as = sheet.querySelector('[data-as]');
      words = wordsBox(sheet.querySelector('[data-ed]'), sheet.querySelector('[data-bar]'));
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
      // Post is there twice (the top bar on a phone, the wide button elsewhere): both say and do the same.
      sheet.go = (label, off) => sheet.querySelectorAll('[data-go]').forEach((g) => { if (label) g.textContent = label; g.disabled = !!off; });
      // The box takes the part of the screen that can be seen: with a phone's keyboard up, the tools sit just above it.
      const vv = window.visualViewport;
      sheet.fit = () => {
        const on = !!vv && !sheet.hidden && Math.abs(vv.scale - 1) < 0.01;
        sheet.style.top = on ? `${vv.offsetTop}px` : ''; sheet.style.height = on ? `${vv.height}px` : '';
        sheet.style.setProperty('--seen', on ? `${vv.height}px` : '100dvh');
      };
      if (vv) { vv.addEventListener('resize', sheet.fit); vv.addEventListener('scroll', sheet.fit); }
      sheet.close = () => { sheet.hidden = true; document.documentElement.classList.remove('sheet-open'); sheet.fit(); };
      sheet.querySelector('[data-x]').addEventListener('click', () => sheet.close());
      sheet.addEventListener('click', (e) => { if (e.target === sheet) sheet.close(); });
      document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !sheet.hidden && !document.querySelector('.cc')) sheet.close(); });
      sheet.querySelector('form').addEventListener('submit', async (e) => {
        e.preventDefault();
        const was = sheet.querySelector('[data-go]').textContent;
        const runs = words.runs(), text = runs.map((r) => r.t).join('').trim();
        if (!editing && !text && !files.length) { words.focus(); return; }
        if ([...text].length > RT_MAX) { toast(`That is too long. A post holds ${RT_MAX.toLocaleString()} letters.`, true); return; }
        const fd = new FormData(); fd.append('text', text); files.forEach((f) => fd.append('photo', f, f.name || 'photo.jpg'));
        // Styles ride along only when there are some; plain words go as they always did.
        if (runs.some((r) => r.b || r.i || r.u || r.s)) fd.append('rich', JSON.stringify(runs));
        if (editing) fd.append('keep', kept.map((k) => k.id).join(',')); else if (!as.hidden && as.value === 'vault') fd.append('as', 'vault');
        sheet.go(editing ? 'Saving…' : 'Posting…', true);
        try {
          const j = await api(editing ? `/posts/${editing.id}/edit` : '/posts', fd);
          const holder = el('div'); holder.innerHTML = j.html;
          const fresh = holder.querySelector('[data-post]');
          if (editing) { const old = feed.querySelector(`[data-post="${editing.id}"]`); if (old && fresh) old.replaceWith(fresh); toast('Saved.'); }
          else { const anchor = feed.querySelector('.composer'); if (fresh) anchor.after(fresh); toast('Posted.'); }
          words.set(''); files = []; kept = []; paint(); sheet.close();
        } catch (err) { toast(err.message, true); }
        sheet.go(was, false);
      });
    }
    const ed = words.el, as = sheet.querySelector('[data-as]');
    files = []; kept = [];
    if (start === 'edit' && post) {
      const kind = post.dataset.kind, written = kind === 'post' || kind === 'share';
      editing = { id: post.dataset.post, by: Number(post.dataset.member), written };
      // The words as the page shows them: the server drew them, styles and all, so they go in as they are.
      words.set(post.querySelector('.post-text')?.innerHTML || '');
      if (written) kept = [...post.querySelectorAll('[data-img]')].filter((a) => a.dataset.img !== '0').map((a) => ({ id: a.dataset.img, src: a.dataset.thumb || a.getAttribute('href') }));
      sheet.querySelector('[data-title]').textContent = written ? 'Edit Post' : 'Words On This Post';
      sheet.go('Save', false);
      sheet.querySelector('[data-sub]').textContent = written ? 'Everyone on the site can see this' : 'The cards on this post stay as they are; you are adding your own words';
      ed.dataset.ph = written ? 'What did you pull today?' : 'Say something about these cards';
      sheet.querySelectorAll('[data-pic-btn]').forEach((b) => { b.hidden = !written; });
      as.hidden = true;
    } else {
      editing = null; words.set('');
      sheet.querySelector('[data-title]').textContent = 'Create Post';
      sheet.go('Post', false);
      sheet.querySelector('[data-sub]').textContent = 'Everyone on the site can see this';
      ed.dataset.ph = 'What did you pull today?';
      sheet.querySelectorAll('[data-pic-btn]').forEach((b) => { b.hidden = false; });
      as.hidden = !(full && me.role === 'admin'); as.value = 'me';
    }
    sheet.querySelector('.sheet-add').hidden = [...sheet.querySelectorAll('[data-pic-btn]')].every((b) => b.hidden);
    // A trainer whose page the vault's team hid can't post: say so before they write anything, not after.
    const waiting = !editing && me.page_hidden === true && !staff;
    let wait = sheet.querySelector('[data-wait]');
    if (!wait) { wait = el('p', 'sheet-wait', 'The vault\'s team has hidden your page, so you can\'t post for now. You can still react, comment and message.'); wait.dataset.wait = ''; sheet.querySelector('.row').after(wait); }
    wait.hidden = !waiting; words.off(waiting); sheet.go('', waiting);
    if (waiting) { sheet.querySelectorAll('[data-pic-btn]').forEach((b) => { b.hidden = true; }); sheet.querySelector('.sheet-add').hidden = true; }
    sheet.setAttribute('aria-label', sheet.querySelector('[data-title]').textContent);
    sheet.who(); sheet.paint();
    sheet.hidden = false; document.documentElement.classList.add('sheet-open'); sheet.fit();
    if (waiting) sheet.querySelector('[data-x]').focus();
    else if (start === 'photo') sheet.querySelector('input[multiple]').click();
    else if (start === 'camera') sheet.querySelector('input[data-card-cam]').click();
    else words.focus();
  }

  // A shared link opens one post with its comments showing.
  // With ?photo= it opens on that photo, where its own likes and comments are (what a notice about a photo links to).
  if (feed.hasAttribute('data-single')) {
    const p = feed.querySelector('[data-post]');
    if (p && p.querySelector('[data-comments]')) who.then(() => {
      openComments(p, false);
      const want = new URLSearchParams(location.search).get('photo'), link = want && /^\d+$/.test(want) ? p.querySelector(`.post-media [data-photo][data-img="${want}"]`) : null;
      if (link) openView(link);
    });
  }
})();
