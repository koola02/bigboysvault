// Scan a card, one of the four ways on the Add cards page (sellers: /me/add; the vault: /admin/quick?by=scan; src/scan.rs).
// The card camera (card-cam.js, data-card-cam="scan") hands back a photo of the card; the site reads it (POST to the
// file input's data-scan address, /me/scan by default) and this shows what the card list has for it: the card itself,
// one tile per version (Normal, Holo, Reverse Holo...), the same card in other sets, and its evolution family. A tile
// opens "Add to your store" (add-card.js; on the vault's page, quick.js puts it in the tray) with that version chosen:
// the seller still picks the condition, the price and how many, and from a binder the card goes into it. Everything
// comes from the full online card list, not only the vault's cards. Nothing is added by itself. When a scan can't help,
// the way out is Search.
// On the Scanner (/scanner, data-mode="look") it is for looking: a tile opens the card big, with its set, number and what
// it sells for, and Add To My Binder puts it in one of the person's own binders (no price or condition).
(() => {
  const file = document.getElementById('scanFile');
  if (!file) return;
  const url = file.dataset.scan || '/me/scan';
  // From a binder's Add cards: the card goes into that binder.
  const binderId = Number(document.getElementById('addPage')?.dataset.binder || 0) || null;
  const out = document.getElementById('scanOut'), word = document.getElementById('scanWord'), leftLine = document.getElementById('scanLeft');
  const shoot = file.closest('label');
  const look = file.dataset.mode === 'look';
  let binders = [];
  try { binders = JSON.parse(document.getElementById('scanBinders')?.textContent || '[]'); } catch (_) {}
  const make = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const peso = (n) => '₱' + Math.round(Number(n) || 0).toLocaleString('en-US');
  const LANG = { en: 'English', ja: 'Japanese', 'zh-tw': 'Chinese', 'zh-cn': 'Chinese', ko: 'Korean' };
  const PROBLEM = {
    not_a_card: 'That doesn’t look like a Pokémon card. Hold one card inside the frame and try again.',
    card_back: 'That’s the back of a card. Turn it over and scan the front.',
    blurry: 'The card is too blurry to read. Hold still, come a little closer, and try again.',
    several_cards: 'There’s more than one card in the photo. Scan one card at a time.',
  };
  let photo = null, busy = false, added = 0;
  // Scans left today, from the line under the button ("3 Of 3 Scans Left Today"); none left, a tap shows the box.
  const leftNow = () => { const m = /^(\d+)\s+Of\s+(\d+)/.exec(leftLine?.textContent || ''); return m ? { left: Number(m[1]), per: Number(m[2]) } : null; };

  // The day's scans are used up (the owner, 20 Nov 2026): a box that says so, with a way to ask an admin for more.
  function usedUp(per) {
    const d = make('dialog', 'float sc-max');
    const panel = make('div', 'float-panel'); panel.tabIndex = -1; panel.setAttribute('role', 'alertdialog');
    panel.append(make('h2', 'h2', 'Daily Scans Used Up'));
    panel.append(make('p', null, per > 0 ? `You've used all ${per} of your scans for today. They start again at midnight.` : 'You have no scans for now.'));
    panel.append(make('p', null, 'Need more each day? Send an admin a message to raise your daily limit.'));
    const acts = make('div', 'sc-max-acts');
    const dm = make('button', 'btn', 'Message An Admin'); dm.type = 'button'; dm.dataset.dmTo = '0';
    const okb = make('button', 'btn ghost', 'OK'); okb.type = 'button';
    acts.append(dm, okb); panel.append(acts); d.append(panel); document.body.append(d);
    // Message An Admin: the box steps aside first (before the chat's own tap handler, dm.js), so the chat opens on top.
    const first = (e) => { if (dm.contains(e.target)) close(); };
    const close = () => { window.removeEventListener('click', first, true); if (d.open) d.close(); d.remove(); };
    window.addEventListener('click', first, true);
    okb.addEventListener('click', close);
    d.addEventListener('click', (e) => { if (e.target === d) close(); });
    d.addEventListener('cancel', (e) => { e.preventDefault(); close(); });
    d.showModal(); panel.focus({ preventScroll: true });
  }
  // Before the camera opens: with none left, the box instead.
  window.addEventListener('click', (e) => {
    if (!shoot.contains(e.target)) return;
    const n = leftNow();
    if (n && n.left <= 0) { e.preventDefault(); e.stopPropagation(); usedUp(n.per); }
  }, true);

  file.addEventListener('change', async () => {
    const f = file.files && file.files[0];
    if (!f || busy) return;
    busy = true; shoot.classList.add('off'); shoot.setAttribute('aria-disabled', 'true');
    if (photo) URL.revokeObjectURL(photo);
    photo = URL.createObjectURL(f);
    out.replaceChildren(waiting());
    out.scrollIntoView({ block: 'start', behavior: 'smooth' });
    const fd = new FormData(); fd.append('photo', f, 'card.jpg');
    try {
      const r = await fetch(url, { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' }, body: fd });
      const j = await r.json().catch(() => ({}));
      if (j.limit) {
        if (leftLine) leftLine.textContent = leftLine.textContent.replace(/^\d+/, '0');
        out.replaceChildren(card(null, j.error || 'You’ve used today’s scans.'));
        usedUp(j.per || leftNow()?.per || 0);
        return;
      }
      if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
      show(j);
    } catch (e) {
      out.replaceChildren(card(null, e.message || 'That didn’t work. Please try again.'));
    } finally {
      busy = false; shoot.classList.remove('off'); shoot.removeAttribute('aria-disabled');
      file.value = '';
      word.textContent = 'Scan Another Card';
    }
  });

  // The Scanner: one card big, what it is, and Add To My Binder.
  function info(c, fin, b) {
    const d = make('dialog', 'float sc-dlg');
    const panel = make('div', 'float-panel'); panel.tabIndex = -1;
    const head = make('header', 'sv-h');
    const titles = make('div', 'sv-t'); titles.append(make('h2', null, c.name || 'Card'));
    if (c.en && c.en !== c.name) titles.append(make('p', null, c.en));
    const x = make('button', 'sv-x'); x.type = 'button'; x.setAttribute('aria-label', 'Close');
    x.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>';
    head.append(titles, x);
    const body = make('div', 'sc-dlg-b');
    const pic = make('span', 'sc-dlg-pic');
    if (c.image_large || c.image) { const i = make('img'); i.src = c.image_large || c.image; i.alt = c.name || ''; i.onerror = () => { if (c.image && i.src !== c.image) i.src = c.image; }; pic.append(i); }
    else pic.append(make('span', 'sc-nopic', 'No Picture Yet'));
    const facts = make('dl', 'sc-facts');
    const fact = (label, value) => { if (!value) return; const r = make('div'); r.append(make('dt', null, label), make('dd', null, value)); facts.append(r); };
    fact('Set', c.set);
    fact('Number', numberOf(c));
    fact('Version', fin ? fin.label : '');
    fact('Language', LANG[c.lang] || '');
    // What it sells for: each version the card list prices (the tile's own first).
    const fins = (c.finishes || []).filter((f) => f.key && f.key !== 'market' && f.php > 0);
    if (fins.length) {
      const list = make('div', 'sc-prices');
      (fin ? [fin, ...fins.filter((f) => f.key !== fin.key)] : fins).filter((f) => f.php > 0).forEach((f) => {
        const r = make('div', 'sc-price'); r.append(make('span', null, f.label), make('b', null, `About ${peso(f.php)}`)); list.append(r);
      });
      const r = make('div'); r.append(make('dt', null, 'Market Price'), make('dd', null)); r.lastChild.append(list); facts.append(r);
    } else fact('Market Price', 'Not Known Yet');
    const add = make('div', 'sc-dlg-add');
    if (binders.length) {
      const lab = make('label', 'sc-dlg-l'); lab.append(make('span', null, 'Add To My Binder'));
      const sel = make('select'); binders.forEach((bd) => { const o = make('option', null, bd.title); o.value = bd.id; sel.append(o); });
      lab.append(sel);
      const go = make('button', 'btn', 'Add To My Binder'); go.type = 'button';
      const said = make('p', 'sc-dlg-said'); said.setAttribute('role', 'status');
      go.addEventListener('click', async () => {
        go.disabled = true; said.textContent = 'Adding…'; said.classList.remove('bad');
        try {
          const r = await fetch('/me/cards', { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify({ id: c.id, image: c.image, image_large: c.image_large, en: c.en, collection: Number(sel.value) }) });
          const j = await r.json().catch(() => ({}));
          if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
          const name = sel.options[sel.selectedIndex].textContent;
          said.replaceChildren(`Added To ${name} ✓ `);
          const v = make('a', null, 'See My Binders'); v.href = `/me/collection#b${sel.value}`; said.append(v);
          b.classList.add('in');
          go.textContent = 'Add Another Copy';
        } catch (e) { said.textContent = e.message; said.classList.add('bad'); }
        go.disabled = false;
      });
      add.append(lab, go, said);
    } else {
      add.append(make('p', null, 'Make a binder first, then add your cards to it.'));
      const mk = make('a', 'btn', 'Make A Binder'); mk.href = '/me/collection#new'; add.append(mk);
    }
    body.append(pic, facts, add);
    panel.append(head, body);
    d.append(panel);
    document.body.append(d);
    const close = () => { d.close(); d.remove(); b.focus({ preventScroll: true }); };
    x.addEventListener('click', close);
    d.addEventListener('click', (e) => { if (e.target === d) close(); });
    d.addEventListener('cancel', (e) => { e.preventDefault(); close(); });
    d.showModal(); panel.focus({ preventScroll: true });
  }

  // The photo while it is read.
  function waiting() {
    const box = make('div', 'sc-read sc-wait');
    const pic = make('img', 'sc-shot'); pic.src = photo; pic.alt = '';
    const t = make('div', 'sc-what'); t.append(make('b', null, 'Reading the card…'), make('small', null, 'This takes a few seconds.'));
    box.append(pic, t);
    return box;
  }

  // The photo with what was read (or what went wrong).
  function card(r, trouble) {
    const box = make('div', 'sc-read');
    const pic = make('img', 'sc-shot'); pic.src = photo; pic.alt = 'Your Photo';
    const t = make('div', 'sc-what');
    if (trouble) t.append(make('b', 'sc-bad', trouble));
    else {
      t.append(make('small', null, 'We Read'));
      const name = r.name || r.name_en || 'This Card';
      t.append(make('b', null, r.name_en && r.name_en !== name ? `${name} (${r.name_en})` : name));
      const bits = [r.number, r.set_name, LANG[r.language]].filter(Boolean).join(' · ');
      if (bits) t.append(make('span', null, bits));
      if (r.sure !== 'high') t.append(make('small', 'sc-unsure', 'Not fully sure: check the name and number on your card.'));
      const done = make('small', 'sc-added'); done.hidden = true; t.append(done);
    }
    box.append(pic, t);
    return box;
  }

  function note(text) { return make('p', 'sc-note', text); }

  // "92/132" when the card list knows the set's size; letters-first numbers (TG05, SV049) as they are.
  const numberOf = (c) => (c.number ? (/^\d+$/.test(c.number) && c.total ? `${c.number}/${c.total}` : c.number) : '');

  function tile(c, fin) {
    const b = make('button', 'sc-tile'); b.type = 'button';
    const pic = make('span', 'sc-pic');
    const none = make('span', 'sc-nopic', 'No Picture Yet');
    if (c.image) { const i = make('img'); i.src = c.image; i.alt = ''; i.loading = 'lazy'; i.decoding = 'async'; i.width = 245; i.height = 342; i.onerror = () => i.replaceWith(none); pic.append(i); } else pic.append(none);
    const plus = make('span', 'sc-plus', '+'); plus.setAttribute('aria-hidden', 'true');
    if (look) b.append(pic, make('b', 'sc-name', c.name || 'Card'));
    else b.append(pic, plus, make('b', 'sc-name', c.name || 'Card'));
    if (c.guess) { b.classList.add('guess'); b.append(make('span', 'sc-guess', 'Best Guess')); }
    if (fin) b.append(make('span', 'sc-fin', fin.label));
    b.append(make('small', null, [c.en && c.en !== c.name ? c.en : '', c.set, numberOf(c)].filter(Boolean).join(' · ')));
    if (fin && fin.php > 0) b.append(make('small', 'sc-mk', `Market About ${peso(fin.php)}`));
    b.setAttribute('aria-label', look ? `${c.name || 'This Card'}${fin ? `, ${fin.label}` : ''}, ${[c.set, numberOf(c)].filter(Boolean).join(' ')}` : `Add ${c.name || 'this card'}${fin ? `, ${fin.label}` : ''}, ${[c.set, numberOf(c)].filter(Boolean).join(' ')}, To Your Store`);
    b.addEventListener('click', () => {
      if (look) { info(c, fin, b); return; }
      if (!window.bbvAddCard) { out.prepend(note('Please reload the page and try again.')); return; }
      const pick = { ...c };
      if (fin) { pick.finish = fin.key; pick.finishLabel = fin.label; }
      window.bbvAddCard.open({ card: pick, versions: true, binderId, onAdded: () => {
        b.classList.add('in'); plus.textContent = '✓';
        added++;
        const done = out.querySelector('.sc-added');
        if (done) { done.hidden = false; done.textContent = `${added} Added To Your Store ✓`; }
      } });
    });
    return b;
  }

  // A heading, its tiles, and "Show N more" when there are many (their pictures load only once shown).
  function group(tag, title, tiles, count, shown) {
    const s = make('section', 'sc-sec');
    const h = make(tag, tag === 'h2' ? 'h2' : null); h.append(title);
    if (count > 1) { h.append(' '); h.append(make('span', 'sc-n', String(count))); }
    const grid = make('div', 'sc-grid');
    tiles.forEach((t, i) => { t.hidden = i >= shown; grid.append(t); });
    s.append(h, grid);
    if (tiles.length > shown) {
      const more = make('button', 'btn ghost sc-more', `Show ${tiles.length - shown} More`); more.type = 'button';
      more.addEventListener('click', () => { tiles.forEach((t) => { t.hidden = false; }); more.remove(); });
      s.append(more);
    }
    return s;
  }

  function show(j) {
    const r = j.read || {};
    if (typeof j.left === 'number' && leftLine) leftLine.textContent = leftLine.textContent.replace(/^\d+/, String(j.left));
    added = 0;
    if (r.problem && r.problem !== 'none') { out.replaceChildren(card(null, PROBLEM[r.problem] || 'That photo couldn’t be read. Try again.')); return; }
    const parts = [card(r)];
    const name = r.name_en || r.name || 'this card';
    const exact = j.exact || [], versions = j.versions || [], family = j.family || [];
    if (!exact.length && !versions.length && !family.length) {
      parts.push(note(j.error ? 'The card list isn’t answering right now. Please try again in a moment.' : `We couldn’t find “${name}” in the card list. Try again, or use Search.`));
      out.replaceChildren(...parts);
      return;
    }
    if (exact.length) {
      const tiles = exact.flatMap((c) => {
        const fins = (c.finishes || []).filter((f) => f.key && f.key !== 'market');
        return fins.length ? fins.map((f) => tile(c, f)) : [tile(c, null)];
      });
      parts.push(group('h2', 'This Card', tiles, 0, tiles.length));
    } else {
      const first = r.number ? `We couldn’t match number ${r.number} exactly.` : 'We couldn’t read the card’s number.';
      parts.push(note(`${first} ${versions[0] && versions[0].guess ? 'Our best guess is first: check it against your card.' : 'Pick your card from these.'}`));
      if (!r.number) parts.push(make('p', 'sc-tip', 'Tip: hold the card flat and close, so the small number at the bottom is sharp.'));
    }
    if (versions.length) {
      parts.push(group('h2', exact.length ? 'Other Versions' : `Cards Called ${name}`, versions.map((c) => tile(c, null)), j.versions_total || versions.length, 24));
    }
    if (family.length) {
      const fam = make('section', 'sc-sec sc-fam');
      fam.append(make('h2', 'h2', 'Its Evolutions'));
      family.forEach((g) => fam.append(group('h3', g.name, (g.cards || []).map((c) => tile(c, null)), g.total || 0, 6)));
      parts.push(fam);
    }
    const again = make('button', 'btn sc-again', 'Scan Another Card'); again.type = 'button';
    again.addEventListener('click', () => file.click());
    parts.push(again);
    out.replaceChildren(...parts);
  }
})();
