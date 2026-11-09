// Manual-entry helper: live preview, drafts, client-side photo resize, in-page save, duplicate check.
// Everything degrades gracefully: without JS the form still posts normally.
// It can also run inside My cards' floating window: that page calls window.mountCardForm() once the form is on screen.
window.mountCardForm = () => {
  const $ = (id) => document.getElementById(id);
  const form = $('entry');
  if (!form) return;
  const root = form.closest('[data-editing]');
  const editing = root.dataset.editing === '1';
  const el = { title: $('title'), cardno: $('cardno'), variant: $('variant'), price: $('price'), notes: $('notes'), tags: $('tags'),
    photo: $('photo'), shot: $('shot'), dropT: $('dropT'), retake: $('retake'), status: $('status'), toast: $('toast'),
    titleHint: $('titleHint'), binderHint: $('binderHint'), chips: $('chips'), draft: $('draft') };
  const DRAFT = 'selxjames:draft:new';
  const safe = (fn) => { try { return fn(); } catch (_) { return null; } };
  let blob = null;      // resized photo, ready to upload
  let busy = false;

  /* ---------- helpers ---------- */
  const peso = (v) => '₱' + Math.round(v).toLocaleString('en-US');
  const num = (s) => parseFloat(String(s).replace(/[^0-9.]/g, '')) || 0;
  const binders = () => [...form.querySelectorAll('input[name=binder]:checked')].map((i) => i.value);
  function toast(msg, kind) {
    el.toast.textContent = msg; el.toast.className = 'toast ' + (kind || ''); el.toast.hidden = false;
    clearTimeout(toast.t); toast.t = setTimeout(() => (el.toast.hidden = true), kind === 'err' ? 8000 : 2600);
  }
  const grade = (v) => {
    const m = /^(psa|cgc|bgs|sgc|ace)\s*([0-9]+(?:\.[0-9])?)/i.exec(v.trim());
    return m ? m[1].toUpperCase() + ' ' + m[2].replace(/\.0$/, '') : '';
  };

  /* ---------- live preview ---------- */
  function preview() {
    $('pvTitle').textContent = el.title.value.trim() || 'Card name';
    $('pvPrice').textContent = peso(num(el.price.value));
    const g = grade(el.variant.value);
    $('pvGrade').hidden = !g; $('pvGrade').textContent = g;
    const src = el.shot.getAttribute('src');
    $('pvImg').hidden = !src; if (src) $('pvImg').src = src; $('pvEmpty').hidden = !!src;
    el.chips.querySelectorAll('button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.v === el.variant.value.trim())));
    const n = binders().length;
    el.binderHint.textContent = n ? '' : 'Not in a binder yet — it will only appear under All cards.';
  }

  /* ---------- draft autosave (survives refresh / dropped connection) ---------- */
  const fields = ['title', 'cardno', 'variant', 'price', 'notes', 'tags'];
  function saveDraft() {
    if (editing) return;
    const d = Object.fromEntries(fields.map((f) => [f, el[f].value])); d.binders = binders();
    const empty = !fields.some((f) => d[f]);
    safe(() => (empty ? localStorage.removeItem(DRAFT) : localStorage.setItem(DRAFT, JSON.stringify(d))));
  }
  function restoreDraft() {
    if (editing || el.title.value) return;
    const raw = safe(() => localStorage.getItem(DRAFT)); if (!raw) return;
    const d = safe(() => JSON.parse(raw)); if (!d || !fields.some((f) => d[f])) return;
    fields.forEach((f) => (el[f].value = d[f] || ''));
    if (!/binders=/.test(location.search)) form.querySelectorAll('input[name=binder]').forEach((i) => (i.checked = (d.binders || []).includes(i.value)));
    el.draft.hidden = false;
  }
  $('discard')?.addEventListener('click', () => { safe(() => localStorage.removeItem(DRAFT)); clearFields(); el.draft.hidden = true; preview(); el.title.focus(); });

  /* ---------- photo: resize on the device (fast uploads, handles HEIC/rotation) ---------- */
  async function toBlob(file, max = 1600) {
    let bmp;
    try { bmp = await createImageBitmap(file, { imageOrientation: 'from-image' }); }
    catch (_) { bmp = await new Promise((res, rej) => { const i = new Image(); i.onload = () => res(i); i.onerror = rej; i.src = URL.createObjectURL(file); }); }
    const w = bmp.width, h = bmp.height, k = Math.min(1, max / Math.max(w, h));
    const c = document.createElement('canvas'); c.width = Math.round(w * k); c.height = Math.round(h * k);
    c.getContext('2d').drawImage(bmp, 0, 0, c.width, c.height);
    return new Promise((res, rej) => c.toBlob((b) => (b ? res(b) : rej(new Error('encode'))), 'image/jpeg', 0.86));
  }
  el.photo.addEventListener('change', async () => {
    const f = el.photo.files[0]; if (!f) return; mk.purl.value = '';
    el.status.textContent = 'Preparing photo…';
    try {
      blob = await toBlob(f);
      el.shot.src = URL.createObjectURL(blob);
    } catch (_) { blob = null; el.shot.src = URL.createObjectURL(f); }
    el.shot.hidden = false; el.dropT.hidden = true; el.retake.hidden = false; el.status.textContent = ''; preview();
  });
  el.retake.addEventListener('click', () => el.photo.click());
  const rmBtn = $('rmphoto');
  if (rmBtn) rmBtn.addEventListener('click', async () => {
    try {
      const r = await fetch('/admin/photo/' + rmBtn.dataset.id + '/remove', { method: 'POST', headers: { Accept: 'application/json' }, credentials: 'same-origin' });
      if (!r.ok) throw new Error('x');
      el.shot.removeAttribute('src'); el.shot.hidden = true; el.dropT.hidden = false; el.retake.hidden = true; rmBtn.hidden = true; blob = null; el.photo.value = '';
      toast('Photo removed', 'ok'); preview();
    } catch (_) { toast('Couldn’t remove the photo.', 'err'); }
  });

  /* ---------- quick-pick chips, price formatting ---------- */
  el.chips.addEventListener('click', (e) => {
    const b = e.target.closest('button'); if (!b) return;
    el.variant.value = el.variant.value.trim() === b.dataset.v ? '' : b.dataset.v; saveDraft(); preview();
  });
  el.price.addEventListener('input', () => { el.price.value = el.price.value.replace(/[^0-9.,]/g, ''); });
  el.price.addEventListener('blur', () => { const v = num(el.price.value); el.price.value = v ? v.toLocaleString('en-US') : ''; preview(); });
  el.price.addEventListener('focus', () => { el.price.value = el.price.value.replace(/,/g, ''); });

  /* ---------- duplicate guard ---------- */
  let dupT;
  el.title.addEventListener('input', () => {
    clearTimeout(dupT);
    dupT = setTimeout(async () => {
      const t = el.title.value.trim(); el.titleHint.textContent = ''; el.titleHint.className = 'hint';
      if (t.length < 3) return;
      const r = await safe(() => fetch(`/admin/check?title=${encodeURIComponent(t)}&id=${root.dataset.id || 0}`, { headers: { Accept: 'application/json' } }));
      const j = r && r.ok ? await r.json().catch(() => null) : null;
      if (j && j.similar.length) { el.titleHint.className = 'hint warn'; el.titleHint.textContent = 'Already have: ' + j.similar.map((s) => s.title).join(' · '); }
    }, 350);
  });

  /* ---------- save ---------- */
  function clearFields() {
    fields.forEach((f) => (el[f].value = ''));
    resetMarket();
    blob = null; el.photo.value = ''; el.shot.removeAttribute('src'); el.shot.hidden = true; el.dropT.hidden = false; el.retake.hidden = true;
    el.titleHint.textContent = '';
  }
  function setBusy(b) { busy = b; form.querySelectorAll('button[type=submit]').forEach((x) => (x.disabled = b)); }

  form.addEventListener('submit', async (e) => {
    e.preventDefault(); if (busy) return;
    const again = e.submitter && e.submitter.name === 'again';
    if (!el.title.value.trim()) { toast('Please enter the card’s name.', 'err'); el.title.focus(); return; }
    const fd = new FormData(form);
    fd.set('price', String(num(el.price.value)));
    fd.set('again', again ? '1' : '0');
    if (blob) fd.set('photo', blob, 'card.jpg');
    setBusy(true); el.status.textContent = 'Saving…';
    try {
      // In My cards' floating window every copy saves with its own finish, condition and price.
      if (form.saveCopies) { await form.saveCopies(fd); form.dispatchEvent(new CustomEvent('card-saved', { bubbles: true })); return; }
      const r = await fetch('/admin/save', { method: 'POST', body: fd, headers: { Accept: 'application/json' }, credentials: 'same-origin' });
      const j = await r.json().catch(() => ({}));
      if (!r.ok || !j.ok) throw new Error(j.error || 'Couldn’t save. Your entry is still here — try again.');
      safe(() => localStorage.removeItem(DRAFT));
      if (again) {
        toast('Saved ✓  ' + j.title, 'ok'); el.status.textContent = '';
        const list = $('recent');
        if (list) { const li = document.createElement('li'); li.innerHTML = '<a hx-boost="false"><span></span><small></small></a>';
          li.querySelector('a').href = '/admin/edit/' + j.id; li.querySelector('span').textContent = j.title; li.querySelector('small').textContent = j.price;
          list.prepend(li); while (list.children.length > 30) list.lastChild.remove(); }
        const t = $('total'); t.textContent = String(+t.textContent + 1);
        clearFields(); preview(); el.title.focus({ preventScroll: false }); window.scrollTo({ top: 0, behavior: 'smooth' });
      } else {
        if (form.closest('dialog')) form.dispatchEvent(new CustomEvent('card-saved', { bubbles: true, detail: j }));
        else location.href = '/products/' + j.handle;
      }
    } catch (err) {
      toast(err.message, 'err'); el.status.textContent = '';
    } finally { setBusy(false); }
  });

  /* ---------- keyboard: ⌘/Ctrl+Enter saves & adds another ---------- */
  form.addEventListener('keydown', (e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); (editing ? $('save') : $('saveAgain')).click(); } });
  // Enter in a single-line field moves to the next one instead of submitting a half-filled form.
  form.addEventListener('keydown', (e) => {
    if (e.key !== 'Enter' || e.metaKey || e.ctrlKey || e.target.tagName !== 'INPUT' || e.target.type === 'checkbox') return;
    e.preventDefault();
    const order = [el.title, el.variant, el.price];
    const i = order.indexOf(e.target); if (i >= 0 && i < order.length - 1) order[i + 1].focus();
  });

  form.addEventListener('input', () => { saveDraft(); preview(); });
  form.addEventListener('change', () => { saveDraft(); preview(); });


  /* ---------- today's market value: name + card number -> pesos ---------- */
  const mk = { box: $('market'), state: $('mkState'), card: $('mkCard'), img: $('mkImg'), name: $('mkName'), set: $('mkSet'), fin: $('mkFinish'),
    price: $('mkPrice'), use: $('mkUse'), photo: $('mkPhoto'), list: $('mkList'), api: $('apiId'), cset: $('cardSet'), finish: $('finishKey'), src: $('priceSrc'), purl: $('photoUrl') };
  let priceDirty = !!el.price.value, current = null, curFinish = null, seq = 0, lookT;
  const baseName = () => el.title.value.split(' - ')[0].trim();
  function hideMarket() { mk.box.hidden = true; mk.card.hidden = true; mk.list.hidden = true; mk.state.textContent = ''; }
  function say(msg) { mk.box.hidden = false; mk.card.hidden = true; mk.list.hidden = true; mk.state.hidden = false; mk.state.textContent = msg; }
  function resetMarket() { hideMarket(); current = null; curFinish = null; mk.api.value = ''; mk.cset.value = ''; mk.finish.value = ''; mk.src.value = ''; mk.purl.value = ''; priceDirty = false; }
  function setPrice(php) { el.price.value = php ? Math.round(php).toLocaleString('en-US') : ''; priceDirty = false; mk.src.value = 'market'; saveDraft(); preview(); }
  function showFinish(f) {
    curFinish = f; mk.finish.value = f.key;
    const cond = el.variant.value.trim();
    mk.fin.querySelectorAll('button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.k === f.key)));
    const A = window.Admin;
    A.renderSug(mk.price, el.price.value.replace(/[^\d.]/g, ''), Math.round(f.php), cond, f.label, current.source, current.updated, (sug) => {
      setPrice(sug); mk.src.value = 'suggested'; mk.use.hidden = true;
    });
    const sugNode = mk.price.nextElementSibling;
    if (sugNode && sugNode.classList.contains('qa-sug')) {
      const btn = sugNode.querySelector('button');
      if (btn) mk.use = btn;
    }
    const sug = A.suggest(f.php, cond);
    if (!priceDirty && sug) { setPrice(sug); mk.src.value = 'suggested'; }
  }
  
  $('chips').addEventListener('click', (e) => {
    if (e.target.tagName !== 'BUTTON') return;
    el.variant.value = e.target.dataset.v;
    saveDraft(); preview();
    if (curFinish) showFinish(curFinish);
  });
  function choose(m, rate) {
    current = { ...m, rate }; mk.box.hidden = false; mk.state.hidden = true; mk.list.hidden = true; mk.card.hidden = false;
    mk.img.src = m.image || ''; mk.name.textContent = m.name; mk.set.textContent = `${m.set} · #${m.number}/${m.total}${m.rarity ? ' · ' + m.rarity : ''}`;
    mk.api.value = m.id; mk.cset.value = m.set;
    mk.fin.replaceChildren(...m.finishes.map((f) => { const b = document.createElement('button'); b.type = 'button'; b.dataset.k = f.key; b.textContent = `${f.label} · ${peso(f.php)}`; b.onclick = () => showFinish(f); return b; }));
    if (!m.finishes.length) { mk.price.textContent = 'No price is listed for this card yet. Type the value yourself.'; mk.use.hidden = true; return; }
    showFinish(m.finishes.find((f) => f.key === mk.finish.value) || m.finishes[0]);
    // First match: complete the name once, e.g. "Pikachu ex - 057/191 - Surging Sparks".
    if (!editing && !el.title.value.includes(' - ')) { el.title.value = `${baseName()} - ${String(m.number).padStart(3, '0')}/${m.total} - ${m.set}`; saveDraft(); preview(); }
  }
  async function lookup() {
    const name = baseName(), number = el.cardno.value.trim();
    if (name.length < 2 || !number) { hideMarket(); return; }
    const my = ++seq; say('Looking up today’s price…');
    try {
      const r = await fetch(`/admin/price?name=${encodeURIComponent(name)}&number=${encodeURIComponent(number)}`, { headers: { Accept: 'application/json' }, credentials: 'same-origin' });
      const j = await r.json(); if (my !== seq) return;
      if (j.error) return say('Couldn’t reach the price service. Type the value yourself.');
      if (!j.matches.length) return say('No match for that name and number. Check them, or type the value yourself.');
      if (j.matches.length === 1 || j.exact) return choose(j.matches[0], j.rate);
      mk.box.hidden = false; mk.card.hidden = true; mk.state.hidden = false; mk.state.textContent = 'A few cards match. Tap yours:'; mk.list.hidden = false;
      mk.list.replaceChildren(...j.matches.map((m) => {
        const li = document.createElement('li'), b = document.createElement('button'), im = document.createElement('img'), t = document.createElement('span');
        b.type = 'button'; im.src = m.image; im.alt = ''; im.width = 42; im.height = 58;
        t.textContent = `${m.set} · #${m.number}/${m.total}${m.finishes[0] ? ' · ' + peso(m.finishes[0].php) : ''}`;
        b.append(im, t); b.onclick = () => choose(m, j.rate); li.append(b); return li;
      }));
    } catch (_) { if (my === seq) say('Couldn’t reach the price service. Type the value yourself.'); }
  }
  const scheduleLookup = () => { clearTimeout(lookT); lookT = setTimeout(lookup, 700); };
  el.title.addEventListener('input', scheduleLookup);
  el.cardno.addEventListener('input', scheduleLookup);
  el.price.addEventListener('input', () => { priceDirty = true; mk.src.value = ''; });
  el.variant.addEventListener('input', () => { if (curFinish) showFinish(curFinish); });
  mk.use.addEventListener('click', () => { if (curFinish) { setPrice(curFinish.php); mk.use.hidden = true; } });
  mk.photo.addEventListener('click', () => {
    if (!current) return; const u = current.image_large || current.image;
    mk.purl.value = u; blob = null; el.photo.value = ''; el.shot.src = u; el.shot.hidden = false; el.dropT.hidden = true; el.retake.hidden = false; preview(); toast('Using the official card image ✓', 'ok');
  });

  // Preselect binders carried over from "save & add another" in the no-JS flow.
  restoreDraft(); preview(); if (editing && el.cardno.value && el.title.value) lookup();
};
window.mountCardForm();
