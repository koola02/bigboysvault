/* A seller editing their own store where it is shown (new look).
   On your store: Edit on any card opens a short sheet (price, condition, how many, on sale or not, a note);
   "Out of stock" is one tap; the cover photo and the store's name change from the top of the page.
   On a listing's own page: the same Edit sheet. What visitors see updates as soon as you save. */
(() => {
  const CONDS = ['Near Mint', 'LP-NM', 'Lightly Played', 'MP-LP', 'Moderately Played', 'HP-MP', 'Heavily Played', 'Damaged-HP', 'Damaged', 'Sealed'];
  const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const toast = (msg, bad) => { if (window.bbvToast) window.bbvToast(msg, bad); };
  const later = (msg) => { try { sessionStorage.setItem('bbv:toast', msg); } catch (_) {} location.reload(); };
  const store = document.getElementById('storefront');

  async function send(url, body) {
    const opts = { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' } };
    if (body instanceof FormData) opts.body = body; else { opts.headers['Content-Type'] = 'application/json'; opts.body = JSON.stringify(body || {}); }
    let r;
    try { r = await fetch(url, opts); } catch (_) { throw new Error('No connection. Nothing was lost; please try again.'); }
    const j = await r.json().catch(() => ({}));
    if (r.status === 401) { location.href = '/login'; throw new Error('Please sign in again.'); }
    if (!r.ok) throw new Error(j.error || 'That didn’t save. Please try again.');
    return j;
  }

  /* ---------- a sheet: in the middle on a big screen, the whole screen on a phone ---------- */
  let open = null;
  function sheet(title, build) {
    close();
    const s = el('div', 'sheet'); s.setAttribute('role', 'dialog'); s.setAttribute('aria-modal', 'true'); s.setAttribute('aria-label', title);
    const f = el('form', 'sheet-in ls'); f.noValidate = true;
    const h = el('header', 'sheet-h'); const b = el('b', '', title); const x = el('button', 'ibtn', '✕'); x.type = 'button'; x.setAttribute('aria-label', 'Close'); h.append(b, x);
    f.append(h); s.append(f);
    const back = document.activeElement;
    const api = { node: s, form: f, close: () => { s.remove(); document.documentElement.classList.remove('sheet-open'); open = null; if (back && back.isConnected) back.focus(); } };
    x.addEventListener('click', api.close);
    s.addEventListener('mousedown', (e) => { if (e.target === s) api.close(); });
    build(f, api);
    document.body.append(s); document.documentElement.classList.add('sheet-open');
    open = api;
    return api;
  }
  function close() { if (open) open.close(); }
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && open && !document.querySelector('.cc')) open.close(); });

  /* ---------- one listing ---------- */
  function listing(src, reload) {
    const d = src.dataset, id = d.id;
    const own = Number(d.own) || 0, market = (d.market || '').replace(/[^\d.]/g, ''), marketN = Number(market) || 0;
    const was = { price: own, cond: d.cond || '', qty: Number(d.qty) || 0, status: d.status || 'Active', notes: (d.notes || '').trim() };
    sheet('Edit listing', (f, api) => {
      const top = el('div', 'ls-card');
      if (d.image) { const i = el('img'); i.src = d.image; i.alt = ''; top.append(i); }
      const tt = el('span'); tt.append(el('b', '', d.title || 'Your card'), el('small', '', [d.set, d.number].filter(Boolean).join(' · '))); top.append(tt);
      f.append(top);

      const price = el('label', 'f ls-f'); price.append('Price');
      const pw = el('span', 'ce-price'); const peso = el('i', '', '₱'); peso.setAttribute('aria-hidden', 'true');
      const pin = el('input'); pin.type = 'number'; pin.name = 'price'; pin.min = '0'; pin.step = '1'; pin.inputMode = 'numeric'; pin.setAttribute('aria-label', 'Price in pesos');
      pin.value = own ? String(own) : ''; if (marketN) pin.placeholder = String(Math.round(marketN));
      pw.append(peso, pin); price.append(pw);
      price.append(el('small', '', marketN ? `Leave it empty to follow today’s market price for this condition (₱${Math.round(marketN).toLocaleString('en-PH')}).` : 'Leave it empty and buyers see “Ask for a price”.'));
      f.append(price);

      const row = el('div', 'ls-row');
      const cl = el('label', 'f ls-f'); cl.append('Condition');
      const sel = el('select'); sel.name = 'cond';
      (CONDS.includes(was.cond) || !was.cond ? CONDS : [was.cond, ...CONDS]).forEach((c) => { const o = el('option', '', c); o.value = c; sel.append(o); });
      sel.value = was.cond; cl.append(sel);
      const ql = el('div', 'f ls-f'); ql.append(el('span', '', 'How many'));
      const st = el('span', 'ls-step'); const minus = el('button', 'ibtn', '−'); minus.type = 'button'; minus.setAttribute('aria-label', 'One fewer');
      const qin = el('input'); qin.type = 'number'; qin.name = 'qty'; qin.min = '0'; qin.max = '9999'; qin.inputMode = 'numeric'; qin.value = String(was.qty); qin.setAttribute('aria-label', 'How many you have');
      const plus = el('button', 'ibtn', '+'); plus.type = 'button'; plus.setAttribute('aria-label', 'One more');
      minus.addEventListener('click', () => { qin.value = String(Math.max(0, (Number(qin.value) || 0) - 1)); paint(); });
      plus.addEventListener('click', () => { qin.value = String(Math.min(9999, (Number(qin.value) || 0) + 1)); paint(); });
      st.append(minus, qin, plus); ql.append(st);
      row.append(cl, ql); f.append(row);

      const sl = el('div', 'f ls-f'); sl.append(el('span', '', 'In your store'));
      const seg = el('div', 'ls-seg'); seg.setAttribute('role', 'group');
      let status = ['Active', 'Out of stock', 'Hidden'].includes(was.status) ? was.status : '';
      const segs = [['Active', 'On sale'], ['Out of stock', 'Out of stock'], ['Hidden', 'Hidden']].map(([v, t]) => {
        const b = el('button', 'chip', t); b.type = 'button'; b.dataset.v = v;
        b.addEventListener('click', () => { status = v; paint(); });
        seg.append(b); return b;
      });
      sl.append(seg);
      const held = { Reserved: 'A buyer has this card reserved right now.', Sold: 'This one is marked sold. Pick “On sale” to list it again.', Expired: 'This listing ran out. Pick “On sale” to list it again.' }[was.status];
      if (held) sl.append(el('small', '', held));
      f.append(sl);

      const nl = el('label', 'f ls-f'); nl.append('Note for buyers');
      const ta = el('textarea'); ta.name = 'notes'; ta.maxLength = 300; ta.rows = 2; ta.placeholder = 'Small whitening on the back, ships in a toploader…'; ta.value = was.notes;
      nl.append(ta); f.append(nl);

      const err = el('p', 'ce-err'); err.hidden = true; err.setAttribute('role', 'alert'); f.append(err);
      const acts = el('div', 'sheet-acts ls-acts');
      const stop = el('button', 'linkbtn ls-stop', 'Stop selling'); stop.type = 'button';
      const cancel = el('button', 'btn ghost', 'Cancel'); cancel.type = 'button';
      const save = el('button', 'btn', 'Save'); save.type = 'submit';
      acts.append(stop, cancel, save); f.append(acts);

      const changes = () => {
        const out = {};
        const p = pin.value.trim() === '' ? 0 : Math.round(Number(pin.value));
        if (Number.isFinite(p) && p !== was.price) out.price = p;
        if (sel.value && sel.value !== was.cond) out.cond = sel.value;
        const q = Math.round(Number(qin.value));
        if (Number.isFinite(q) && q !== was.qty) out.qty = q;
        if (status && status !== was.status) out.status = status;
        if (ta.value.trim() !== was.notes) out.notes = ta.value.trim();
        return out;
      };
      let busy = false;
      function paint() {
        segs.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.v === status)));
        save.disabled = busy || !Object.keys(changes()).length;
        save.textContent = busy ? 'Saving…' : 'Save';
      }
      f.addEventListener('input', paint);
      cancel.addEventListener('click', api.close);
      const done = async (body, msg) => {
        busy = true; paint(); err.hidden = true;
        try {
          await send(`/me/listings/${id}`, body);
          api.close();
          if (reload) later(msg); else { await refresh(id); toast(msg); }
        } catch (e) { busy = false; paint(); err.textContent = e.message; err.hidden = false; }
      };
      f.addEventListener('submit', (e) => {
        e.preventDefault();
        if (busy) return;
        if (pin.value.trim() !== '' && !(Number(pin.value) >= 0)) { err.textContent = 'Please type the price as a number, like 250.'; err.hidden = false; pin.focus(); return; }
        const ch = changes();
        if (!Object.keys(ch).length) { api.close(); return; }
        done(ch, 'Saved. Buyers see it now.');
      });
      // Taking a card off sale asks once, on the button itself.
      stop.addEventListener('click', () => {
        if (!stop.classList.contains('sure')) { stop.textContent = 'Take it off sale? Tap again'; stop.classList.add('sure'); setTimeout(() => { if (stop.isConnected) { stop.textContent = 'Stop selling'; stop.classList.remove('sure'); } }, 4000); return; }
        done({ kind: 'own' }, 'Taken off sale. It is still in My cards.');
      });
      paint();
      setTimeout(() => { pin.focus(); pin.select(); }, 30);
    });
  }

  // The card as the page shows it now, taken from a fresh copy of the page, so it is exactly what buyers see.
  async function refresh(id) {
    if (!store) return;
    try {
      const html = await (await fetch(location.href, { credentials: 'same-origin', headers: { Accept: 'text/html' } })).text();
      const doc = new DOMParser().parseFromString(html, 'text/html');
      const now = doc.querySelector(`.sf-card[data-id="${CSS.escape(String(id))}"]`), old = store.querySelector(`.sf-card[data-id="${CSS.escape(String(id))}"]`);
      if (old && now) {
        [...old.attributes].forEach((a) => { if (a.name !== 'class') old.removeAttribute(a.name); });
        [...now.attributes].forEach((a) => old.setAttribute(a.name, a.value));
        old.innerHTML = now.innerHTML;
      } else if (old) old.remove();
      const stats = doc.querySelector('.sf-stats'), mine = store.querySelector('.sf-stats');
      if (stats && mine) mine.innerHTML = stats.innerHTML;
      const n = document.getElementById('sfN'); if (n) n.textContent = store.querySelectorAll('.sf-card:not([hidden])').length;
    } catch (_) { location.reload(); }
  }

  document.addEventListener('click', async (e) => {
    const edit = e.target.closest('[data-own-edit]');
    if (edit) { e.preventDefault(); const card = edit.closest('.sf-card'); listing(card || edit, edit.hasAttribute('data-reload')); return; }
    const stock = e.target.closest('[data-own-stock]');
    if (stock) {
      const card = stock.closest('.sf-card'); if (!card) return;
      const out = card.dataset.status !== 'Out of stock';
      stock.disabled = true;
      try { await send(`/me/listings/${card.dataset.id}`, { status: out ? 'Out of stock' : 'Active' }); await refresh(card.dataset.id); toast(out ? 'Marked out of stock. Buyers can’t reserve it for now.' : 'Back in stock and on sale.'); }
      catch (err) { toast(err.message, true); stock.disabled = false; }
      return;
    }
    const shop = e.target.closest('[data-own-store]');
    if (shop) {
      const d = shop.dataset;
      sheet('Edit store', (f, api) => {
        const nl = el('label', 'f ls-f'); nl.append('Store name');
        const name = el('input'); name.type = 'text'; name.maxLength = 40; name.value = d.name || ''; name.placeholder = d.fallback || 'Your store'; name.autocomplete = 'off';
        nl.append(name, el('small', '', `Shown at the top of your store. Leave it empty to use your trainer name${d.fallback ? ` (${d.fallback})` : ''}.`));
        const ll = el('label', 'f ls-f'); ll.append('Where you are');
        const loc = el('input'); loc.type = 'text'; loc.maxLength = 60; loc.value = d.location || ''; loc.placeholder = 'Quezon City, meetups at SM North';
        ll.append(loc, el('small', '', 'Helps buyers know if a meetup is possible.'));
        const err = el('p', 'ce-err'); err.hidden = true; err.setAttribute('role', 'alert');
        const acts = el('div', 'sheet-acts ls-acts');
        const more = el('a', 'linkbtn ls-stop', 'All store settings'); more.href = '/me/settings';
        const cancel = el('button', 'btn ghost', 'Cancel'); cancel.type = 'button'; cancel.addEventListener('click', api.close);
        const save = el('button', 'btn', 'Save'); save.type = 'submit';
        acts.append(more, cancel, save);
        f.append(nl, ll, err, acts);
        f.addEventListener('submit', async (ev) => {
          ev.preventDefault();
          if (name.value.trim() === (d.name || '') && loc.value.trim() === (d.location || '')) { api.close(); return; }
          save.disabled = true; save.textContent = 'Saving…';
          const fd = new FormData(); fd.append('store_name', name.value.trim()); fd.append('location', loc.value.trim());
          try { await send('/me/settings', fd); later('Saved. This is how your store looks now.'); }
          catch (e2) { save.disabled = false; save.textContent = 'Save'; err.textContent = e2.message; err.hidden = false; }
        });
        setTimeout(() => name.focus(), 30);
      });
    }
  });

  // The cover photo: pick one and it is up.
  document.addEventListener('change', async (e) => {
    const inp = e.target.closest && e.target.closest('[data-own-cover]'); if (!inp || !inp.files || !inp.files[0]) return;
    const label = inp.closest('label'), text = label && label.querySelector('span'), was = text && text.textContent;
    if (text) text.textContent = 'Uploading…';
    const fd = new FormData(); fd.append('cover', inp.files[0], inp.files[0].name || 'cover.jpg');
    try {
      const j = await send('/me/settings', fd);
      const box = document.getElementById('sfCover');
      if (box && j.cover) { let img = box.querySelector(':scope > img'); if (!img) { img = el('img'); img.alt = ''; box.prepend(img); } img.src = j.cover; }
      if (text) text.textContent = 'Change cover';
      toast('Cover photo updated.');
    } catch (err) { if (text) text.textContent = was; toast(err.message, true); }
    inp.value = '';
  });
})();
