// Scan a card (sellers, /me/scan; src/scan.rs). The card camera (card-cam.js, data-card-cam="scan") hands back a photo of
// the card; the site reads it (POST /me/scan) and this page shows what the card list has for it: the card itself, one
// tile per version (Normal, Holo, Reverse Holo...), the same card in other sets, and its evolution family. A tile opens
// "Add to your store" (add-card.js) with that version chosen; the seller still picks the condition, the price and how
// many. Everything comes from the full online card list Add cards searches, not only the vault's cards. Nothing is
// added by itself.
(() => {
  const file = document.getElementById('scanFile');
  if (!file) return;
  const out = document.getElementById('scanOut'), word = document.getElementById('scanWord'), leftLine = document.getElementById('scanLeft');
  const shoot = file.closest('label');
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
      const r = await fetch('/me/scan', { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' }, body: fd });
      const j = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(j.error || 'That didn’t work. Please try again.');
      show(j);
    } catch (e) {
      out.replaceChildren(card(null, e.message || 'That didn’t work. Please try again.'));
    } finally {
      busy = false; shoot.classList.remove('off'); shoot.removeAttribute('aria-disabled');
      file.value = '';
      word.textContent = 'Scan another card';
    }
  });

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
    const pic = make('img', 'sc-shot'); pic.src = photo; pic.alt = 'Your photo';
    const t = make('div', 'sc-what');
    if (trouble) t.append(make('b', 'sc-bad', trouble));
    else {
      t.append(make('small', null, 'We read'));
      const name = r.name || r.name_en || 'This card';
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
    const none = make('span', 'sc-nopic', 'No picture yet');
    if (c.image) { const i = make('img'); i.src = c.image; i.alt = ''; i.loading = 'lazy'; i.decoding = 'async'; i.width = 245; i.height = 342; i.onerror = () => i.replaceWith(none); pic.append(i); } else pic.append(none);
    const plus = make('span', 'sc-plus', '+'); plus.setAttribute('aria-hidden', 'true');
    b.append(pic, plus, make('b', 'sc-name', c.name || 'Card'));
    if (c.guess) { b.classList.add('guess'); b.append(make('span', 'sc-guess', 'Best guess')); }
    if (fin) b.append(make('span', 'sc-fin', fin.label));
    b.append(make('small', null, [c.en && c.en !== c.name ? c.en : '', c.set, numberOf(c)].filter(Boolean).join(' · ')));
    if (fin && fin.php > 0) b.append(make('small', 'sc-mk', `Market about ${peso(fin.php)}`));
    b.setAttribute('aria-label', `Add ${c.name || 'this card'}${fin ? `, ${fin.label}` : ''}, ${[c.set, numberOf(c)].filter(Boolean).join(' ')}, to your store`);
    b.addEventListener('click', () => {
      if (!window.bbvAddCard) { out.prepend(note('Please reload the page and try again.')); return; }
      const pick = { ...c };
      if (fin) { pick.finish = fin.key; pick.finishLabel = fin.label; }
      window.bbvAddCard.open({ card: pick, versions: true, onAdded: () => {
        b.classList.add('in'); plus.textContent = '✓';
        added++;
        const done = out.querySelector('.sc-added');
        if (done) { done.hidden = false; done.textContent = `${added} added to your store ✓`; }
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
      const more = make('button', 'btn ghost sc-more', `Show ${tiles.length - shown} more`); more.type = 'button';
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
      parts.push(note(j.error ? 'The card list isn’t answering right now. Please try again in a moment.' : `We couldn’t find “${name}” in the card list. Try again, or use Add cards in My store.`));
      out.replaceChildren(...parts);
      return;
    }
    parts.push(make('p', 'sc-src', 'From the full Pokémon card list, the same one Add cards searches: every card printed, not only the vault’s.'));
    if (exact.length) {
      const tiles = exact.flatMap((c) => {
        const fins = (c.finishes || []).filter((f) => f.key && f.key !== 'market');
        return fins.length ? fins.map((f) => tile(c, f)) : [tile(c, null)];
      });
      parts.push(group('h2', 'This card', tiles, 0, tiles.length));
    } else {
      const first = r.number ? `We couldn’t match number ${r.number} exactly.` : 'We couldn’t read the card’s number.';
      parts.push(note(`${first} ${versions[0] && versions[0].guess ? 'Our best guess is first: check it against your card.' : 'Pick your card from these.'}`));
      if (!r.number) parts.push(make('p', 'sc-tip', 'Tip: hold the card flat and close, so the small number at the bottom is sharp.'));
    }
    if (versions.length) {
      parts.push(group('h2', exact.length ? 'Other versions' : `Cards called ${name}`, versions.map((c) => tile(c, null)), j.versions_total || versions.length, 24));
    }
    if (family.length) {
      const fam = make('section', 'sc-sec sc-fam');
      fam.append(make('h2', 'h2', 'Its evolutions'));
      family.forEach((g) => fam.append(group('h3', g.name, (g.cards || []).map((c) => tile(c, null)), g.total || 0, 6)));
      parts.push(fam);
    }
    const again = make('button', 'btn sc-again', 'Scan another card'); again.type = 'button';
    again.addEventListener('click', () => file.click());
    parts.push(again);
    out.replaceChildren(...parts);
  }
})();
