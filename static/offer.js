// Make an offer on a seller's card, from the card's own window or page (member_card_part.html: data-offer-card).
// The seller can accept, counter or decline; the answer comes in the bell and My orders.
(() => {
  const make = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const who = () => (window.bbvWho ? window.bbvWho() : (window.bbvWhoAnswer ||= fetch('/me/who', { credentials: 'same-origin', headers: { Accept: 'application/json' } }).then((r) => (r.ok ? r.json() : {})).catch(() => ({}))));
  const peso = (n) => '₱' + Math.round(n).toLocaleString('en-US');
  const say = (msg, bad) => {
    let t = document.querySelector('.me-toast');
    if (!t) { t = make('div', 'me-toast'); t.setAttribute('role', 'status'); }
    (document.querySelector('dialog[open]:last-of-type') || document.body).append(t);
    t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on');
    clearTimeout(say.t); say.t = setTimeout(() => t.classList.remove('on'), bad ? 5000 : 3000);
  };

  document.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-offer-card]'); if (!b) return;
    e.preventDefault(); e.stopPropagation();
    const j = await who();
    if (!j || !j.member) {
      try { sessionStorage.setItem('bbv:after-login', location.pathname + location.search); sessionStorage.setItem('bbv:login-why', 'offer'); } catch (_) {}
      location.href = '/login';
      return;
    }
    const d = b.dataset, left = Math.max(1, Number(d.qty) || 1), price = Number(d.price) || 0;
    const dialog = make('dialog', 'float of');
    const form = make('form', 'float-panel'); form.noValidate = true;
    const head = make('header', 'sv-h');
    const titles = make('div', 'sv-t'); titles.append(make('h2', null, 'Make an offer'), make('p', null, d.title || 'This card'));
    const x = make('button', 'sv-x'); x.type = 'button'; x.setAttribute('aria-label', 'Close');
    x.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>';
    head.append(titles, x);
    const body = make('div', 'of-body');
    const card = make('p', 'of-card');
    if (d.image) { const im = make('img'); im.src = d.image; im.alt = ''; im.width = 60; im.height = 84; card.append(im); }
    card.append(make('span', null, `${d.cond ? d.cond + ' · ' : ''}${price > 0 ? 'asking ' + peso(price) : 'no price yet'} · ${left} available`));
    const amountL = make('label', 'sf-field'); amountL.append(make('span', null, 'Your offer, per card ₱'));
    const amount = make('input', 'sheet-input'); amount.type = 'number'; amount.min = '1'; amount.step = '1'; amount.inputMode = 'numeric'; amount.required = true;
    amountL.append(amount);
    const qtyL = make('label', 'sf-field'); qtyL.append(make('span', null, 'How many'));
    const qty = make('input', 'sheet-input'); qty.type = 'number'; qty.min = '1'; qty.max = String(left); qty.value = '1'; qty.inputMode = 'numeric';
    qtyL.append(qty);
    const noteL = make('label', 'sf-field'); noteL.append(make('span', null, 'A note for the seller (optional)'));
    const note = make('textarea', 'sheet-input'); note.rows = 2; note.maxLength = 300; note.placeholder = 'When can we meet? Can you ship?';
    noteL.append(note);
    const hint = make('p', 'hint show', 'The seller can accept, counter or decline. If they accept, it becomes an order at your price.');
    const err = make('p', 'vc-err'); err.setAttribute('role', 'alert');
    const acts = make('div', 'sheet-acts');
    const cancel = make('button', 'btn ghost', 'Cancel'); cancel.type = 'button';
    const go = make('button', 'btn', 'Send offer'); go.type = 'submit';
    acts.append(cancel, go);
    body.append(card, amountL, qtyL, noteL, hint, err, acts);
    form.append(head, body); dialog.append(form);
    (b.closest('dialog[open]') || document.body).append(dialog);
    const close = () => { dialog.close(); dialog.remove(); if (b.isConnected) b.focus({ preventScroll: true }); };
    x.addEventListener('click', close); cancel.addEventListener('click', close);
    dialog.addEventListener('cancel', (ev) => { ev.preventDefault(); close(); });
    // Escape closes this window only, not the card window under it.
    dialog.addEventListener('keydown', (ev) => { if (ev.key === 'Escape') ev.stopPropagation(); });
    let downOutside = false;
    dialog.addEventListener('pointerdown', (ev) => { downOutside = ev.target === dialog; });
    dialog.addEventListener('click', (ev) => { if (downOutside && ev.target === dialog) close(); downOutside = false; });
    form.addEventListener('submit', async (ev) => {
      ev.preventDefault(); err.textContent = '';
      const a = Number(amount.value);
      if (!(a > 0)) { err.textContent = 'Type how much you offer for one card.'; amount.focus(); return; }
      const n = Math.round(Number(qty.value));
      if (!(n >= 1 && n <= left)) { err.textContent = left === 1 ? 'There is 1 of this card.' : `How many: from 1 to ${left}.`; qty.focus(); return; }
      go.disabled = true; go.textContent = 'Sending…';
      try {
        const r = await fetch(`/store/${encodeURIComponent(d.slug)}/offer`, { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify({ card_id: Number(d.offerCard), qty: n, amount: a, message: note.value }) });
        const res = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error(res.error || 'That didn’t work. Please try again.');
        close();
        say('Offer sent ✓ Watch the bell for the seller’s answer.');
      } catch (x2) { err.textContent = x2.message; go.disabled = false; go.textContent = 'Send offer'; }
    });
    dialog.showModal(); amount.focus();
  }, true);
})();
