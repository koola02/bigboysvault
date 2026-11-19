// "Apply as a seller" (templates/apply.html): photos are made smaller on the phone first, then everything goes up
// together, and the trainer lands in their chat with the vault's team, where the application goes on.
(() => {
  const form = document.getElementById('applyForm'); if (!form) return;
  const pics = document.getElementById('applyPics'), file = document.getElementById('applyFile');
  const err = document.getElementById('applyErr'), go = document.getElementById('applyGo');
  const add = file.closest('label');
  const MAX = 8;
  const items = [];
  const make = (tag, cls) => { const n = document.createElement(tag); if (cls) n.className = cls; return n; };

  // Phone photos are huge: shrink them here, so they go up quickly on mobile data.
  async function shrink(f, max = 1600) {
    const url = URL.createObjectURL(f);
    try {
      const img = new Image(); img.src = url; await img.decode();
      const k = Math.min(1, max / Math.max(img.naturalWidth, img.naturalHeight));
      const c = document.createElement('canvas'); c.width = Math.round(img.naturalWidth * k); c.height = Math.round(img.naturalHeight * k);
      c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
      return await new Promise((res) => c.toBlob(res, 'image/jpeg', 0.88));
    } catch (_) { return f; } finally { URL.revokeObjectURL(url); }
  }

  file.addEventListener('change', async () => {
    err.textContent = '';
    for (const f of file.files) {
      if (items.length >= MAX) { err.textContent = `Up to ${MAX} photos.`; break; }
      const it = { blob: null, node: make('div', 'vc-pic busy') };
      const img = make('img'); img.alt = ''; it.node.append(img);
      const x = make('button', 'vc-x'); x.type = 'button'; x.textContent = '✕'; x.setAttribute('aria-label', 'Take This Photo Out');
      x.addEventListener('click', () => { items.splice(items.indexOf(it), 1); it.node.remove(); add.hidden = false; });
      it.node.append(x);
      pics.insertBefore(it.node, add);
      items.push(it);
      it.blob = await shrink(f);
      img.src = URL.createObjectURL(it.blob);
      it.node.classList.remove('busy');
    }
    add.hidden = items.length >= MAX;
    file.value = '';
  });

  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    err.textContent = '';
    go.disabled = true; go.textContent = items.length ? 'Sending photos…' : 'Sending…';
    const fd = new FormData();
    fd.append('note', form.note.value);
    items.forEach((it, i) => { if (it.blob) fd.append('proof', it.blob, `proof-${i + 1}.jpg`); });
    try {
      let r;
      try { r = await fetch('/me/sell', { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' }, body: fd }); }
      catch (_) { throw new Error('No connection. Please try again.'); }
      const j = await r.json().catch(() => ({}));
      if (r.status === 401) { location.href = '/login'; return; }
      if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
      try { sessionStorage.setItem('bbv:toast', 'Application sent ✓ The admins answer here.'); } catch (_) {}
      location.href = `/me/messages?t=${j.thread}`;
    } catch (x) {
      err.textContent = x.message;
      go.disabled = false; go.textContent = 'Send Application';
    }
  });
})();
