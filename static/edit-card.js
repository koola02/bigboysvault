/* Editing a vault card right on its page (new look, full admins).
   "Edit" turns the details panel into a short form: price, condition, description, tags and the photo.
   Save writes it and shows the page as visitors now see it; Cancel puts everything back. */
(() => {
  const btn = document.querySelector('[data-card-edit]');
  const detail = document.querySelector('.detail'), info = detail && detail.querySelector('.detail-info'), stage = detail && detail.querySelector('.detail-img');
  if (!btn || !info) return;
  const d = btn.dataset, id = Number(d.id), copies = Number(d.copies) || 1;
  const list = (s) => (s || '').split(',').map((t) => t.trim()).filter(Boolean);
  const was = { price: Number(d.price) || 0, cond: (d.cond || '').trim(), notes: (d.notes || '').trim(), tags: list(d.tags) };
  const CONDS = ['Near Mint', 'Lightly Played', 'Moderately Played', 'Heavily Played', 'Damaged', 'Sealed', 'PSA 10', 'PSA 9'];
  const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const toast = (msg, bad) => (window.bbvToast ? window.bbvToast(msg, bad) : null);
  let form = null, photo = null, busy = false;

  const now = () => {
    const f = form.elements;
    const price = f.price.value.trim() === '' ? was.price : Number(f.price.value);
    return { price, cond: f.cond.value.trim(), notes: f.notes.value.trim(), tags: list(f.tags.value) };
  };
  const changes = () => {
    const v = now(), out = {};
    if (Number.isFinite(v.price) && v.price !== was.price) out.price = v.price;
    if (v.cond && v.cond !== was.cond) out.condition = v.cond;
    if (v.notes !== was.notes) out.notes = v.notes;
    if (v.tags.join('|').toLowerCase() !== was.tags.join('|').toLowerCase()) out.tags = v.tags;
    return out;
  };
  const dirty = () => !!photo || Object.keys(changes()).length > 0;

  function paintConds() {
    const cur = form.elements.cond.value.trim().toLowerCase();
    form.querySelectorAll('.ce-conds .chip').forEach((c) => c.setAttribute('aria-pressed', String(c.textContent.toLowerCase() === cur)));
  }
  function paintSave() {
    const save = form.querySelector('[data-ce-save]');
    save.disabled = busy || !dirty();
    save.textContent = busy ? 'Saving…' : 'Save';
  }

  function open() {
    if (form) return;
    form = el('form', 'ce'); form.noValidate = true; form.setAttribute('aria-label', 'Edit this card');
    form.innerHTML = `
      <div class="ce-top"><b>Edit this card</b><small></small></div>
      <label class="f ce-f">Price
        <span class="ce-price"><i aria-hidden="true">₱</i><input type="number" name="price" min="0" step="any" inputmode="decimal" enterkeyhint="done" aria-label="Price in pesos"></span>
      </label>
      <div class="f ce-f"><span id="ceCondL">Condition</span>
        <div class="ce-conds" role="group" aria-labelledby="ceCondL"></div>
        <input type="text" name="cond" maxlength="80" placeholder="Or type your own, like CGC 9.5" aria-labelledby="ceCondL" autocapitalize="words">
      </div>
      <label class="f ce-f">Description
        <textarea name="notes" maxlength="4000" rows="3" placeholder="Anything a buyer should know (optional)"></textarea>
      </label>
      <label class="f ce-f">Tags
        <input type="text" name="tags" maxlength="300" placeholder="starter, gift idea, vintage" autocapitalize="none">
        <small>Separate tags with commas. People can search by them.</small>
      </label>
      <div class="f ce-f"><span>Photo</span>
        <div class="ce-photo">
          <span class="ce-thumb" hidden><img alt=""><button type="button" class="ibtn" data-ce-nophoto aria-label="Keep the photo it has now">✕</button></span>
          <label class="btn ghost sm">Change photo<input type="file" accept="image/*" data-card-cam hidden></label>
          <small data-ce-photo-t>The card camera lines the card up for you.</small>
        </div>
      </div>
      <p class="ce-err" role="alert" hidden></p>
      <div class="ce-acts">
        <a class="linkbtn" href="/admin/edit/${id}">More options</a>
        <button type="button" class="btn ghost" data-ce-cancel>Cancel</button>
        <button type="submit" class="btn" data-ce-save>Save</button>
      </div>`;
    form.querySelector('.ce-top small').textContent = copies > 1 ? `Your changes apply to all ${copies} copies of this card.` : 'Visitors see your changes as soon as you save.';
    const f = form.elements;
    f.price.value = was.price ? String(was.price) : ''; f.cond.value = was.cond; f.notes.value = was.notes; f.tags.value = was.tags.join(', ');
    const chips = form.querySelector('.ce-conds');
    const all = CONDS.some((c) => c.toLowerCase() === was.cond.toLowerCase()) || !was.cond ? CONDS : [was.cond, ...CONDS];
    all.forEach((c) => { const b = el('button', 'chip', c); b.type = 'button'; b.addEventListener('click', () => { f.cond.value = c; paintConds(); paintSave(); }); chips.append(b); });
    const file = form.querySelector('input[type=file]'), thumb = form.querySelector('.ce-thumb');
    file.addEventListener('change', () => {
      const pic = file.files && file.files[0]; if (!pic) return;
      photo = pic; thumb.querySelector('img').src = URL.createObjectURL(pic); thumb.hidden = false;
      form.querySelector('[data-ce-photo-t]').textContent = 'New photo ready. It goes up when you save.';
      paintSave();
    });
    form.querySelector('[data-ce-nophoto]').addEventListener('click', () => { photo = null; file.value = ''; thumb.hidden = true; form.querySelector('[data-ce-photo-t]').textContent = 'The card camera lines the card up for you.'; paintSave(); });
    form.addEventListener('input', () => { paintConds(); paintSave(); const c = form.querySelector('[data-ce-cancel]'); c.textContent = 'Cancel'; c.classList.remove('bad'); });
    form.addEventListener('submit', (e) => { e.preventDefault(); save(); });
    form.querySelector('[data-ce-cancel]').addEventListener('click', cancel);
    form.addEventListener('keydown', (e) => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); save(); } });
    info.prepend(form);
    detail.classList.add('editing'); btn.hidden = true;
    paintConds(); paintSave();
    f.price.focus(); f.price.select();
    info.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }

  function close() {
    if (!form) return;
    form.remove(); form = null; photo = null;
    detail.classList.remove('editing'); btn.hidden = false; btn.focus();
  }
  // Leaving with changes asks once, on the button itself, instead of a pop-up.
  function cancel() {
    const c = form.querySelector('[data-ce-cancel]');
    if (dirty() && !c.classList.contains('bad')) { c.textContent = 'Discard changes'; c.classList.add('bad'); return; }
    close();
  }

  async function send(url, opts) {
    let r;
    try { r = await fetch(url, { method: 'POST', credentials: 'same-origin', ...opts, headers: { Accept: 'application/json', ...(opts.headers || {}) } }); }
    catch (_) { throw new Error('No connection. Your changes are still here; try Save again.'); }
    const j = await r.json().catch(() => ({}));
    if (r.status === 401 || r.status === 403) throw new Error('Please sign in as the vault’s admin to edit cards.');
    if (!r.ok) throw new Error(j.error || 'That didn’t save. Please try again.');
    return j;
  }

  async function save() {
    if (busy || !form) return;
    const err = form.querySelector('.ce-err'); err.hidden = true;
    const v = now();
    if (!Number.isFinite(v.price) || v.price < 0) { err.textContent = 'Please type the price as a number, like 250.'; err.hidden = false; form.elements.price.focus(); return; }
    if (!v.cond && was.cond) { err.textContent = 'Please pick a condition.'; err.hidden = false; form.elements.cond.focus(); return; }
    const ch = changes();
    if (!photo && !Object.keys(ch).length) { close(); return; }
    busy = true; paintSave();
    try {
      if (Object.keys(ch).length) await send('/admin/quick-edit', { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ ids: [id], copies: true, ...ch }) });
      if (photo) { const fd = new FormData(); fd.append('photo', photo, photo.name || 'photo.jpg'); await send(`/admin/photo/${id}`, { body: fd }); }
      try { sessionStorage.setItem('bbv:toast', 'Saved. This is how visitors see it now.'); } catch (_) {}
      location.reload();
    } catch (e) {
      busy = false; paintSave();
      err.textContent = e.message; err.hidden = false;
      toast(e.message, true);
    }
  }

  btn.addEventListener('click', open);
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && form && !document.querySelector('.cc, .dm-lightbox')) cancel(); });
  // A change that was never saved shouldn't vanish because of a stray tap on a link.
  addEventListener('beforeunload', (e) => { if (form && !busy && dirty()) e.preventDefault(); });
  // "/products/x#edit" opens the editor straight away (the link staff get from other pages).
  if (location.hash === '#edit') window.bbvWho && window.bbvWho().then((j) => { if (j && j.full) open(); });
})();
