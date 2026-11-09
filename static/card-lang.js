// Japanese and Chinese cards can be read in English, when asked. The text comes from the server, so it works on every device.
export function mountLang(root = document) {
  const sw = root.querySelector('[data-lang-switch]');
  if (!sw || sw.dataset.ready) return;
  sw.dataset.ready = '1';
  const buttons = [...sw.querySelectorAll('button')];
  const original = sw.dataset.original;
  const handle = sw.dataset.handle;
  const fields = [...root.querySelectorAll('[data-tr]')];
  const saved = new Map(fields.map((n) => [n, { text: n.textContent, lang: n.getAttribute('lang') }]));
  const versionsSaved = new Map([...root.querySelectorAll('[data-version]')].map((n) => [n, [...n.children].map((c) => c.textContent)]));
  const relatedSaved = new Map([...root.querySelectorAll('.rail a.card .card-t')].map((n) => [n, n.textContent]));
  const cache = new Map();
  let wanted = original;

  const mark = (lang) => buttons.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.to === lang)));

  function showOriginal() {
    saved.forEach((o, n) => { n.textContent = o.text; if (o.lang) n.setAttribute('lang', o.lang); else n.removeAttribute('lang'); });
    relatedSaved.forEach((t, n) => { n.textContent = t; });
    versionsSaved.forEach((words, n) => n.replaceChildren(...words.map((w) => { const t = document.createElement('span'); t.textContent = w; return t; })));
  }

  function show(data, lang) {
    for (const n of fields) {
      const key = n.dataset.tr;
      let text = data[key];
      if (key === 'copies') text = data.labels.copies.replace('{n}', n.dataset.n);
      if (key === 'market') text = `${data.labels.market} ${n.dataset.date}`;
      if (key === 'more') text = data.labels.more;
      if (key.startsWith('lab_')) text = data.labels[key.slice(4)];
      if (key === 'versions') text = data.labels.versions;
      if (key === 'card') text = data.labels.card;
      if (key === 'rarity') text = data.rarity;
      if (key === 'tag') text = data.tags[n.dataset.i];
      if (text == null) continue;
      n.textContent = text;
      n.setAttribute('lang', lang === 'zh' ? 'zh-Hant' : lang);
    }
    for (const box of root.querySelectorAll('[data-version]')) {
      const words = data.versions[box.dataset.version];
      if (words) box.replaceChildren(...words.map((w) => { const t = document.createElement('span'); t.textContent = w; return t; }));
    }
    for (const a of root.querySelectorAll('.rail a.card')) {
      const t = data.related[a.getAttribute('href')];
      if (t) a.querySelector('.card-t').textContent = t;
    }
  }

  async function choose(lang) {
    wanted = lang;
    mark(lang);
    sw.classList.add('busy');
    try {
      if (!cache.has(lang)) {
        const r = await fetch(`/api/card/${encodeURIComponent(handle)}/text?to=${lang}`);
        if (!r.ok) throw new Error('status ' + r.status);
        cache.set(lang, await r.json());
      }
      if (wanted === lang) show(cache.get(lang), lang);
    } catch (e) {
      console.debug('Translate failed', e);
      if (wanted === lang) { wanted = original; mark(original); showOriginal(); }
    }
    sw.classList.remove('busy');
  }

  // A card always opens as it was printed. English is there for anyone who taps it, and only for that card.
  buttons.forEach((b) => b.addEventListener('click', () => {
    if (b.dataset.to === original) { wanted = original; mark(original); showOriginal(); return; }
    choose(b.dataset.to);
  }));
}

mountLang(document.querySelector('main') || document);
