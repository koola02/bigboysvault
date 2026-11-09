// Page editor: sections you can add, edit, reorder, hide and delete, with a live preview.
(() => {
  const A = window.Admin;
  const root = document.getElementById('pageEditor'); if (!root) return;
  const list = document.getElementById('blocks');
  const pv = document.getElementById('pv');
  const collections = JSON.parse(root.dataset.collections || '[]');
  const reloadPreview = () => { try { pv.contentWindow.location.reload(); } catch (_) { pv.src = '/'; } };

  /* ---------- list refresh with a smooth FLIP reorder ---------- */
  async function refresh() {
    const before = new Map([...list.children].map((li) => [li.dataset.id, li.getBoundingClientRect().top]));
    const html = await (await fetch('/admin/page?frag=1', { credentials: 'same-origin' })).text();
    list.innerHTML = html;
    for (const li of list.children) {
      const was = before.get(li.dataset.id); if (was == null) { li.classList.add('enter'); continue; }
      const dy = was - li.getBoundingClientRect().top; if (!dy) continue;
      li.style.transition = 'none'; li.style.transform = `translateY(${dy}px)`;
      requestAnimationFrame(() => requestAnimationFrame(() => { li.style.transition = ''; li.style.transform = ''; }));
    }
    reloadPreview();
  }

  /* ---------- row actions ---------- */
  list.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-act]'); if (!b || b.disabled) return;
    const li = b.closest('.blk'); const id = li.dataset.id; const act = b.dataset.act;
    try {
      if (act === 'up' || act === 'down') { await A.post(`/admin/blocks/${id}/move`, { dir: act }); await refresh(); }
      else if (act === 'vis') { await A.post(`/admin/blocks/${id}/visible`, { on: li.classList.contains('off') ? '1' : '0' }); await refresh(); A.toast(li.classList.contains('off') ? 'Hidden from your page' : 'Showing on your page', { kind: 'ok' }); }
      else if (act === 'edit') openEditor(li);
      else if (act === 'del') {
        A.undoable({
          label: `Deleted “${li.querySelector('.blk-kind').textContent}”`,
          hide: () => { li.style.height = li.offsetHeight + 'px'; requestAnimationFrame(() => li.classList.add('gone')); },
          show: () => { li.classList.remove('gone'); li.style.height = ''; },
          commit: async () => { await A.post(`/admin/blocks/${id}/delete`); await refresh(); },
        });
      }
    } catch (err) { A.toast(err.message, { kind: 'err' }); }
  });

  /* ---------- drag and drop ---------- */
  if (window.dragSort) window.dragSort(list, { item: '.blk', handle: '[data-drag]', key: (li) => li.dataset.id, onDrop: async ([ids]) => {
    try {
      const r = await fetch('/admin/blocks/order', { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json', Accept: 'application/json' }, body: JSON.stringify({ ids: ids.map(Number) }) });
      const j = await r.json().catch(() => ({})); if (!r.ok || !j.ok) throw new Error(j.error || 'Not saved. Try again.');
      await refresh(); A.toast('Order saved ✓', { kind: 'ok' });
    } catch (err) { A.toast(err.message, { kind: 'err' }); await refresh(); }
  } });

  /* ---------- site name ---------- */
  const brand = document.getElementById('brand'); let brandVal = brand.value;
  brand.addEventListener('change', async () => {
    try { await A.post('/admin/settings', { brand: brand.value }); brandVal = brand.value; A.toast('Site name saved ✓', { kind: 'ok' }); reloadPreview(); }
    catch (err) { brand.value = brandVal; A.toast(err.message, { kind: 'err' }); }
  });

  /* ---------- editor form, generated from a tiny schema ---------- */
  const SORTS = [['new', 'Newest first'], ['price-desc', 'Highest value first'], ['price-asc', 'Lowest value first'], ['title', 'A to Z']];
  const SCHEMA = {
    hero: [['text', 'eyebrow', 'Small label'], ['text', 'title', 'Headline'], ['area', 'subtitle', 'Subtitle', 'Tip: type {count} to show your live card total.'],
      ['text', 'button_label', 'Button text'], ['text', 'button_link', 'Button goes to', 'For example /all or /collections/favorites'], ['text', 'sticker', 'Sticker (optional)', 'A small badge on the hero, for example “New arrivals”. Leave empty for none.'], ['img', 'img_a', 'Left picture'], ['img', 'img_b', 'Right picture']],
    ticker: [['area', 'phrases', 'Scrolling phrases', 'One per line. They scroll across the page.']],
    binders: [['text', 'title', 'Section title']],
    row: [['select', 'handle', 'Show cards from', collections.map((c) => [c.handle, c.title])], ['text', 'title', 'Section title', 'Leave empty to use the collection’s name.'],
      ['text', 'eyebrow', 'Small label (optional)'], ['select', 'sort', 'Order', SORTS], ['switch', 'spotlight', 'Highlight this section', 'Dark spotlight style with gold accents.']],
    feature: [['text', 'eyebrow', 'Small label'], ['text', 'title', 'Headline'], ['area', 'subtitle', 'Subtitle', 'Tip: type {count} to show your live card total.'],
      ['text', 'button_label', 'Button text'], ['text', 'button_link', 'Button goes to'], ['img', 'img', 'Picture']],
    text: [['text', 'title', 'Title'], ['area', 'body', 'Text']],
  };
  const el = (tag, cls, txt) => { const n = document.createElement(tag); if (cls) n.className = cls; if (txt != null) n.textContent = txt; return n; };

  function openEditor(li) {
    const id = li.dataset.id, kind = li.dataset.kind, data = JSON.parse(li.dataset.json || '{}'), fields = SCHEMA[kind] || [];
    const form = el('form', 'sheet-form'); form.onsubmit = (e) => { e.preventDefault(); A.sheet.save(); };
    const getters = {}; const files = {}; const removed = new Set();
    for (const [type, key, label, hint] of fields) {
      const wrap = el('div', 'sf');
      if (type === 'switch') {
        const row = el('label', 'sf-switch'); const t = el('span', null, label); const inp = el('input'); inp.type = 'checkbox'; inp.checked = !!data[key]; inp.className = 'sr';
        const knob = el('span', 'knob'); row.append(t, inp, knob); wrap.append(row); getters[key] = () => (inp.checked ? '1' : '0');
      } else if (type === 'img') {
        wrap.append(el('span', 'sf-l', label)); const box = el('div', 'sf-img'); const img = el('img'); img.alt = ''; const ph = el('span', 'sf-ph', 'No picture');
        const setImg = (src) => { if (src) { img.src = src; img.hidden = false; ph.hidden = true; } else { img.hidden = true; ph.hidden = false; } }; setImg(data[key]);
        const pick = el('input'); pick.type = 'file'; pick.accept = 'image/*'; pick.className = 'sr';
        const change = el('button', 'pbtn', 'Change picture'); change.type = 'button'; change.onclick = () => pick.click();
        const rm = el('button', 'pbtn danger', 'Remove'); rm.type = 'button'; rm.onclick = () => { removed.add(key); delete files[key]; setImg(''); };
        pick.onchange = async () => { const f = pick.files[0]; if (!f) return; removed.delete(key); files[key] = f.type === 'image/png' ? f : await A.resize(f); setImg(URL.createObjectURL(files[key])); };
        box.append(img, ph); const acts = el('div', 'sf-img-acts'); acts.append(change, rm, pick); wrap.append(box, acts);
      } else {
        const lab = el('label', 'sf-label'); lab.append(el('span', 'sf-l', label));
        let inp; if (type === 'area') { inp = el('textarea'); inp.rows = 3; } else if (type === 'select') {
          inp = el('select'); for (const [v, t] of hint) { const o = el('option', null, t); o.value = v; inp.append(o); } }
        else { inp = el('input'); inp.type = 'text'; }
        if (type !== 'select') inp.value = data[key] || ''; else inp.value = data[key] || (key === 'sort' ? 'new' : 'all');
        inp.autocomplete = 'off'; lab.append(inp); wrap.append(lab); getters[key] = () => inp.value;
        if (hint && type !== 'select') wrap.append(el('p', 'hint', hint));
      }
      form.append(wrap);
    }
    A.sheet.open(`Edit ${li.querySelector('.blk-kind').textContent.toLowerCase()}`, form, async () => {
      const fd = new FormData();
      for (const k in getters) fd.set(k, getters[k]());
      for (const k of removed) fd.set('rm_' + k, '1');
      for (const k in files) fd.set(k, files[k], files[k].name || 'picture.jpg');
      await A.post(`/admin/blocks/${id}/save`, fd);
      A.sheet.close(); await refresh(); A.toast('Saved ✓', { kind: 'ok' });
    });
  }

  /* ---------- add a section ---------- */
  const KINDS = [['hero', 'Big banner at the top', 'Big title, two pictures and a button.'], ['ticker', 'Scrolling words', 'A gold strip with words that slide across.'], ['row', 'Row of cards', 'A row of cards from one binder.'],
    ['feature', 'Picture banner', 'A box with words, a button and a picture.'], ['binders', 'Binder pictures', 'A picture for each binder.'],
    ['stats', 'Number boxes', 'How many cards, graded cards, binders and total value.'], ['text', 'Text', 'A title and a few lines of words.']];
  document.getElementById('addBlock').addEventListener('click', () => {
    const box = el('div', 'picker');
    for (const [kind, name, desc] of KINDS) {
      const b = el('button', 'pick'); b.type = 'button'; b.append(el('strong', null, name), el('small', null, desc));
      b.onclick = async () => {
        try {
          const j = await A.post('/admin/blocks/add', { kind }); A.sheet.close(); await refresh();
          const li = list.querySelector(`[data-id="${j.id}"]`); li?.scrollIntoView({ behavior: 'smooth', block: 'center' });
          if (li && kind !== 'stats') setTimeout(() => openEditor(li), 320); else A.toast('Section added ✓', { kind: 'ok' });
        } catch (err) { A.toast(err.message, { kind: 'err' }); }
      };
      box.append(b);
    }
    A.sheet.open('Add a section', box, null);
  });
})();
