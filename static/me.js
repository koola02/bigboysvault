// Account Settings: everything saves right here, on the page.
(() => {
  const root = document.getElementById('me'); if (!root) return;
  const $ = (id) => document.getElementById(id);

  const say = (msg, bad) => {
    let t = document.querySelector('.me-toast');
    if (!t) { t = document.createElement('div'); t.className = 'me-toast'; t.setAttribute('role', 'status'); document.body.append(t); }
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
    if (r.status === 401) { location.href = '/login'; throw new Error('Please log in again.'); }
    if (!r.ok) throw new Error(j.error || 'That didn’t save. Please try again.');
    return j;
  }

  /* ---------- picture ---------- */
  // Phone photos are huge: shrink here first, so the upload is quick on mobile data.
  async function shrink(file, max = 800) {
    const url = URL.createObjectURL(file);
    try {
      const img = new Image(); img.src = url; await img.decode();
      const k = Math.min(1, max / Math.max(img.naturalWidth, img.naturalHeight));
      const c = document.createElement('canvas'); c.width = Math.round(img.naturalWidth * k); c.height = Math.round(img.naturalHeight * k);
      c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
      return await new Promise((res) => c.toBlob(res, 'image/jpeg', 0.9));
    } finally { URL.revokeObjectURL(url); }
  }
  let picked = null;
  $('meFile').addEventListener('change', async (e) => {
    const f = e.target.files[0]; if (!f) return;
    try {
      picked = await shrink(f);
      const old = $('meAvatar');
      const img = document.createElement('img'); img.className = 'av av-xl'; img.id = 'meAvatar'; img.alt = ''; img.src = URL.createObjectURL(picked);
      old.replaceWith(img);
      saveProfile();
    } catch (_) { say('That picture couldn’t be read. Try a JPEG or PNG.', true); }
    e.target.value = '';
  });

  /* ---------- name and description ---------- */
  const counter = (area, out) => { const paint = () => { out.textContent = area.value.length; }; area.addEventListener('input', paint); paint(); };
  const form = $('meProfile');
  counter(form.elements.bio, $('bioCount'));
  // The page's cover photo: shown here as soon as it is picked, saved with the rest.
  const coverFile = $('pageCoverFile');
  coverFile.addEventListener('change', () => { const f = coverFile.files[0]; if (!f) return; $('pageCoverImg').src = URL.createObjectURL(f); $('pageCoverImg').hidden = false; $('pageCoverNone').hidden = true; form.elements.drop_cover.checked = false; saveProfile(); });
  form.elements.drop_cover.addEventListener('change', () => { if (form.elements.drop_cover.checked) saveProfile(); });
  async function saveProfile() {
    const fd = new FormData(form);
    if (picked) fd.set('avatar', picked, 'avatar.jpg');
    try {
      const cover = coverFile.files[0];
      if (cover) fd.set('cover', await shrink(cover, 1600), 'cover.jpg');
      const j = await send('/me/profile', fd);
      picked = null; coverFile.value = ''; form.elements.drop_cover.checked = false;
      if (j.cover) $('pageCoverImg').src = j.cover;
      $('pageCoverImg').hidden = !j.cover; $('pageCoverNone').hidden = !!j.cover; $('pageCoverDrop').hidden = !j.cover;
      $('pageCoverWord').textContent = j.cover ? 'Change Cover Photo' : 'Upload Cover Photo';
      say('Saved ✓');
    } catch (err) { say(err.message, true); }
  }
  form.addEventListener('submit', (e) => { e.preventDefault(); saveProfile(); });


  /* ---------- account ---------- */
  $('mePassOpen').addEventListener('click', () => { const d = $('meAccount'); d.open = true; d.scrollIntoView({ behavior: 'smooth', block: 'start' }); setTimeout(() => d.querySelector('input[name=current]').focus({ preventScroll: true }), 350); });
  const pass = $('mePass');
  pass.addEventListener('submit', async (e) => {
    e.preventDefault();
    try { await send('/me/password', { current: pass.elements.current.value, new: pass.elements.new.value }); pass.reset(); say('Password Changed ✓'); } catch (err) { say(err.message, true); }
  });
  $('meDelete').addEventListener('click', async () => {
    if (!(await (window.bbvConfirm ? window.bbvConfirm('Delete your trainer, cards and vouch for good? This can’t be undone.', { yes: 'Delete My Trainer', no: 'Keep It' }) : Promise.resolve(confirm('Delete your trainer, cards and vouch for good? This can’t be undone.'))))) return;
    try { await send('/me/delete'); location.href = '/trainers'; } catch (err) { say(err.message, true); }
  });

})();
