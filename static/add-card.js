// "Add a card" for sellers (the owner, 18 Nov 2026: nothing is added or priced automatically). Tapping a card from the
// card list opens this window: the seller picks the condition, types their own price (the market price is only a
// hint), chooses the picture (the card list's online one, a photo taken now, or one uploaded), how many, a note and the binder. Only "Add to my
// store" makes the listing (POST /me/cards/new). Used by the Store Manager's Add cards (store.js) and a binder's Add
// cards (binder-tools.js): window.bbvAddCard.open({ card, binderId, onAdded }). Scan a card (scan.js) also passes
// `versions: true`, and the window then lets the seller choose the version (Normal, Holo, Reverse Holo...), starting
// from the one on the tile they tapped (`card.finish`); everywhere else the card's usual version is used, as before.
(() => {
  const CONDS = ['Near Mint', 'LP-NM', 'Lightly Played', 'MP-LP', 'Moderately Played', 'HP-MP', 'Heavily Played', 'Damaged-HP', 'Damaged', 'Sealed'];
  const make = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const peso = (n) => '₱' + Math.round(Number(n) || 0).toLocaleString('en-US');
  const json = (url) => fetch(url, { credentials: 'same-origin', headers: { Accept: 'application/json' } }).then((r) => r.json()).catch(() => ({}));
  const say = (msg, bad) => {
    if (window.Admin && window.Admin.toast) { window.Admin.toast(msg, { kind: bad ? 'err' : 'ok' }); return; }
    let t = document.querySelector('.me-toast');
    if (!t) { t = make('div', 'me-toast'); t.setAttribute('role', 'status'); }
    (document.querySelector('dialog[open]:last-of-type') || document.body).append(t);
    t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on');
    clearTimeout(say.t); say.t = setTimeout(() => t.classList.remove('on'), bad ? 5000 : 2800);
  };
  // Phone photos are huge: make them smaller here, so they go up quickly on mobile data.
  async function shrink(file, max = 1600) {
    const url = URL.createObjectURL(file);
    try {
      const img = new Image(); img.src = url; await img.decode();
      const k = Math.min(1, max / Math.max(img.naturalWidth, img.naturalHeight));
      const c = document.createElement('canvas'); c.width = Math.round(img.naturalWidth * k); c.height = Math.round(img.naturalHeight * k);
      c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
      return await new Promise((res) => c.toBlob(res, 'image/jpeg', 0.9));
    } catch (_) { return file; } finally { URL.revokeObjectURL(url); }
  }
  let binders = null;
  const loadBinders = () => (binders ||= json('/me/binders').then((j) => {
    if (!j.binders) binders = null; // didn't load: ask again next time
    return (j.binders || []).map((b) => ({ id: b.key ?? b.id, title: b.title }));
  }).catch(() => { binders = null; return []; }));
  const prices = new Map();
  // The card's versions with their market prices (Near Mint), the usual one first.
  const finishesOf = (id) => {
    if (!prices.has(id)) prices.set(id, json(`/me/cards/price?id=${encodeURIComponent(id)}`).then((j) => (j.matches && j.matches[0] && j.matches[0].finishes) || []));
    return prices.get(id);
  };
  const nearMint = (id) => finishesOf(id).then((f) => (f.length ? Number(f[0].php) || 0 : 0));

  // `listing`: a card already in the store (GET /me/listings/{id}): the window opens filled in with it, and saving puts
  // it in the binder too (POST /me/listings/{id}/place). Without it, `card` is a new card from the card list.
  function open({ card, listing = null, binderId = null, onAdded = null, versions = false }) {
    const old = !!listing;
    if (old) card = { id: '', name: listing.title, set: listing.set, number: listing.number, image: listing.image };
    const goWord = old ? (binderId ? 'Save and add to this binder' : 'Save') : 'Add to my store';
    const d = make('dialog', 'float ac');
    const form = make('form', 'float-panel'); form.noValidate = true;
    const head = make('header', 'sv-h');
    // The version chosen (a key from the card list's prices, "reverse-holofoil" say), shown under the card's name.
    let finish = (!old && versions && card.finish) || '';
    const sub = make('p', null, finish && card.finishLabel ? `${card.name || 'This card'} · ${card.finishLabel}` : card.name || 'This card');
    const titles = make('div', 'sv-t'); titles.append(make('h2', null, old ? (binderId ? 'Add to this binder' : 'Check this card') : 'Add to your store'), sub);
    const x = make('button', 'sv-x'); x.type = 'button'; x.setAttribute('aria-label', 'Close');
    x.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>';
    head.append(titles, x);
    const body = make('div', 'ac-body');

    // The card, with the picture the store will show.
    const top = make('div', 'ac-top');
    const pic = make('span', 'ac-pic'); const img = make('img'); img.alt = ''; img.src = card.image || ''; pic.append(img);
    const who = make('div', 'ac-who'); who.append(make('b', null, card.name || 'Card'), make('small', null, [card.en, card.set, card.number].filter(Boolean).join(' · ')));
    top.append(pic, who);

    // Which picture (the owner, 18 Nov 2026): the card list's own picture from online, a photo taken now with the card
    // camera (card-cam.js: card-shaped and straight), or a photo uploaded from the phone or computer.
    const picBox = make('fieldset', 'ac-f ac-pics');
    picBox.append(make('legend', null, 'Picture'));
    const icon = (d) => { const s = make('span', 'ac-ico'); s.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true">${d}</svg>`; return s; };
    const useList = make('button', 'ac-opt ac-src'); useList.type = 'button'; useList.setAttribute('aria-pressed', 'true');
    const thumb = make('span', 'ac-ico ac-thumb');
    if (card.image) { const t = make('img'); t.src = card.image; t.alt = ''; t.onerror = () => t.remove(); thumb.append(t); }
    useList.append(thumb, make('span', null, old ? 'Keep this picture' : 'Online picture'));
    const choice = (word, d, cam) => {
      const l = make('label', 'ac-opt ac-src'); l.append(icon(d), make('span', null, word));
      const f = make('input', 'sr'); f.type = 'file'; f.accept = 'image/*'; if (cam) f.dataset.cardCam = ''; l.append(f);
      return [l, f];
    };
    const [takeL, takeIn] = choice('Take a photo', '<path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/>', true);
    const [upL, upIn] = choice('Upload a photo', '<rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/>', false);
    const row = make('div', 'ac-opts ac-srcs'); row.append(useList, takeL, upL);
    picBox.append(row);
    let photo = null, from = null;
    const showChoice = () => {
      useList.setAttribute('aria-pressed', String(!photo));
      takeL.classList.toggle('on', !!photo && from === takeIn); upL.classList.toggle('on', !!photo && from === upIn);
    };
    useList.addEventListener('click', () => { photo = null; img.src = card.image || ''; showChoice(); });
    let preparing = false;
    [takeIn, upIn].forEach((input) => input.addEventListener('change', async () => {
      const f = input.files && input.files[0]; if (!f) return;
      preparing = true; go.disabled = true; go.textContent = 'Getting the photo ready…';
      photo = await shrink(f); from = input; img.src = URL.createObjectURL(photo); showChoice(); input.value = '';
      preparing = false; go.disabled = false; go.textContent = goWord;
    }));

    // Version: only from Scan a card, and only when the card list prices more than one.
    const verBox = make('fieldset', 'ac-f'); verBox.hidden = true; verBox.append(make('legend', null, 'Version'));
    const verRow = make('div', 'ac-opts'); verBox.append(verRow);

    // Condition: chosen by the seller (none picked for them).
    const condL = make('label', 'ac-f'); condL.append(make('b', null, 'Condition'));
    const cond = make('select'); cond.append(new Option('Choose its condition', '', !old, !old));
    const conds = old && listing.cond && !CONDS.includes(listing.cond) ? [listing.cond, ...CONDS] : CONDS;
    conds.forEach((c) => cond.append(new Option(c, c, false, old && c === listing.cond)));
    condL.append(cond);

    // Price: typed by the seller. The market price shows as a hint only.
    const priceL = make('label', 'ac-f'); priceL.append(make('b', null, 'Your price for one card ₱'));
    const price = make('input'); price.type = 'number'; price.min = '1'; price.step = '1'; price.inputMode = 'numeric'; price.placeholder = 'Type your price';
    const hint = make('small', 'ac-how');
    priceL.append(price, hint);
    let nm = 0;
    const paintHint = () => {
      hint.replaceChildren();
      if (!nm) { hint.textContent = cond.value ? 'No market price found for this card. Set the price you want.' : ''; return; }
      const sug = window.Price && cond.value ? window.Price.suggest(nm, cond.value) : null;
      if (cond.value && sug == null && cond.value !== 'Near Mint') { hint.textContent = `No market price for ${cond.value} here. Set the price you want.`; return; }
      const v = sug || nm;
      hint.append(`${cond.value ? `Market price in ${cond.value}` : 'Market price in Near Mint'}: about ${peso(v)}. `);
      const use = make('button', 'linkbtn inline', `Use ${peso(v)}`); use.type = 'button';
      use.addEventListener('click', () => { price.value = String(Math.round(v)); price.focus(); });
      hint.append(use);
    };
    // A card already in the store: its own price stays as it is; one that followed the market waits for the seller's price.
    if (old) { if (Number(listing.value_own) > 0) price.value = String(Math.round(listing.value_own)); nm = Number(listing.market) || 0; paintHint(); }
    else if (versions) {
      finishesOf(card.id).then((all) => {
        const list = all.filter((f) => f.key && f.key !== 'market');
        const usual = list.find((f) => f.key === finish) || all[0];
        nm = usual ? Number(usual.php) || 0 : 0; paintHint();
        if (list.length < 2) return;
        const choose = (f, b) => {
          finish = f.key; nm = Number(f.php) || 0;
          verRow.querySelectorAll('.ac-opt').forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
          sub.textContent = `${card.name || 'This card'} · ${f.label || f.key}`;
          paintHint();
        };
        const start = list.find((f) => f.key === finish) || list[0];
        list.forEach((f) => {
          const b = make('button', 'ac-opt', f.label || f.key); b.type = 'button'; b.setAttribute('aria-pressed', 'false');
          b.addEventListener('click', () => choose(f, b));
          verRow.append(b);
          if (f === start) choose(f, b);
        });
        verBox.hidden = false;
      });
    } else nearMint(card.id).then((v) => { nm = v; paintHint(); });
    cond.addEventListener('change', paintHint);

    const qtyL = make('label', 'ac-f ac-half'); qtyL.append(make('b', null, 'How many'));
    const qty = make('input'); qty.type = 'number'; qty.min = old ? '0' : '1'; qty.max = old ? '9999' : '99'; qty.value = old ? String(listing.qty ?? 1) : '1'; qty.inputMode = 'numeric';
    qtyL.append(qty);
    const binderL = make('label', 'ac-f ac-half'); binderL.append(make('b', null, 'Binder'));
    const binder = make('select'); binder.append(new Option('Not in a binder', ''));
    binderL.append(binder);
    // The binder it is meant for is chosen at once (named "This binder" until the list comes), so Save is right even if
    // it is tapped before the list of binders has loaded, or if loading it failed.
    const pickBinder = binderId ?? (old ? listing.binder_id : null);
    const picked = pickBinder != null && pickBinder !== '' ? new Option('This binder', String(pickBinder), true, true) : null;
    if (picked) binder.append(picked);
    loadBinders().then((list) => {
      list.forEach((b) => { if (picked && String(b.id) === picked.value) picked.text = b.title; else binder.append(new Option(b.title, String(b.id))); });
    });
    const pair = make('div', 'ac-pair'); pair.append(qtyL, binderL);

    const notesL = make('label', 'ac-f'); notesL.append(make('b', null, 'A note for buyers '), make('small', null, '(optional)'));
    const notes = make('textarea'); notes.rows = 2; notes.maxLength = old ? 500 : 300; notes.placeholder = 'Centering, a tiny whitening on the back, comes in a sleeve…';
    if (old) notes.value = listing.notes || '';
    notesL.append(notes);

    const err = make('p', 'vc-err'); err.setAttribute('role', 'alert');
    const acts = make('div', 'ac-acts');
    const cancel = make('button', 'btn ghost', 'Cancel'); cancel.type = 'button';
    const go = make('button', 'btn', goWord); go.type = 'submit';
    acts.append(cancel, go);
    body.append(top, verBox, picBox, condL, priceL, pair, notesL, err);
    form.append(head, body, acts); d.append(form);
    document.body.append(d);   // above any window it was opened from (the binder's Add cards)

    const close = () => { d.close(); d.remove(); };
    x.addEventListener('click', close); cancel.addEventListener('click', close);
    d.addEventListener('cancel', (e) => { e.preventDefault(); close(); });
    d.addEventListener('keydown', (e) => { if (e.key === 'Escape') e.stopPropagation(); });
    let downOutside = false;
    d.addEventListener('pointerdown', (e) => { downOutside = e.target === d; });
    d.addEventListener('click', (e) => { if (downOutside && e.target === d) close(); downOutside = false; });

    form.addEventListener('input', () => { err.textContent = ''; });
    form.addEventListener('change', () => { err.textContent = ''; });
    form.addEventListener('submit', async (e) => {
      e.preventDefault(); err.textContent = '';
      if (preparing) return;
      if (!cond.value) { err.textContent = 'Choose the card’s condition.'; cond.focus(); return; }
      const p = Number(price.value);
      if (!(p >= 1)) { err.textContent = 'Type your price for one card.'; price.focus(); return; }
      const n = Math.round(Number(qty.value));
      if (!(n >= (old ? 0 : 1) && n <= (old ? 9999 : 99))) { err.textContent = old ? 'How many: from 0 to 9999 (0 means sold out).' : 'How many: from 1 to 99.'; qty.focus(); return; }
      go.disabled = true; go.textContent = photo ? 'Sending the photo…' : old ? 'Saving…' : 'Adding…';
      const fd = new FormData();
      if (!old) { fd.append('id', card.id); fd.append('image', card.image || ''); fd.append('image_large', card.image_large || ''); fd.append('en', card.en || ''); }
      fd.append('cond', cond.value); fd.append('price', String(Math.round(p))); fd.append('qty', String(n)); fd.append('notes', notes.value); fd.append('binder_id', binder.value);
      if (!old && finish) fd.append('finish', finish);
      if (photo) fd.append('photo', photo, 'card.jpg');
      try {
        const r = await fetch(old ? `/me/listings/${listing.id}/place` : '/me/cards/new', { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' }, body: fd });
        const j = await r.json().catch(() => ({}));
        if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
        close();
        const shown = (j.card && (j.card.price_text || j.card.price)) || peso(p);
        say(old ? (binder.value ? `${card.name || 'Card'} is in the binder at ${shown} ✓` : `${card.name || 'Card'} saved at ${shown} ✓`) : `${card.name || 'Card'} is in your store at ${shown} ✓`);
        if (onAdded) onAdded(j.card);
        // The Add cards page lists what was added on this visit, whichever way it came.
        document.dispatchEvent(new CustomEvent('bbv:added', { detail: j.card }));
      } catch (x2) { err.textContent = x2.message; go.disabled = false; go.textContent = goWord; }
    });
    d.showModal();
    cond.focus();
  }
  window.bbvAddCard = { open };
})();
