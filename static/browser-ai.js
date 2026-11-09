// AI that runs on the visitor's own computer, built into the browser. Nothing is sent to any server and there are no bills.
// Browsers without it (phones, Safari, Firefox) never see this button.
(() => {
  const make = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text) n.textContent = text; return n; };

  /* ---------- Ask for cards in a whole sentence ---------- */
  const finder = document.getElementById('finder');
  if (finder && 'LanguageModel' in self) setUpAsk().catch(() => {});

  async function setUpAsk() {
    const langs = { expectedInputs: [{ type: 'text', languages: ['en'] }], expectedOutputs: [{ type: 'text', languages: ['en'] }] };
    if ((await LanguageModel.availability(langs)) === 'unavailable') return;
    const input = document.querySelector('form.search input');
    if (!input) return;

    const row = make('div', 'ai-ask');
    const btn = make('button', 'ai-btn', '✨ Read my sentence'); btn.type = 'button';
    const note = make('span', 'ai-note');
    row.append(btn, note); row.hidden = true;
    finder.before(row);

    // Only worth offering for a sentence: the finder already handles names and "under 500" by itself.
    const refresh = () => { row.hidden = !document.documentElement.dataset.finder || input.value.trim().split(/\s+/).length < 3; };
    input.addEventListener('input', () => { note.textContent = ''; refresh(); });

    let session = null;
    btn.addEventListener('click', async () => {
      const sentence = input.value.trim();
      if (!sentence) return;
      btn.disabled = true;
      note.textContent = 'Thinking on your device…';
      try {
        const binders = (await (await fetch('/api/binders')).json()).binders || [];
        if (!session) {
          session = await LanguageModel.create({
            ...langs,
            initialPrompts: [{ role: 'system', content: 'You turn a sentence about Pokémon cards into search filters for a card collection. "words" holds only Pokémon names, card names or set names from the sentence. Prices are in Philippine pesos. Leave out anything the sentence does not say.' }],
            monitor(m) { m.addEventListener('downloadprogress', (e) => { note.textContent = `Getting ready ${Math.round(e.loaded * 100)}%`; }); },
          });
        }
        const titles = binders.map((b) => b.title);
        const reply = await session.prompt(sentence, {
          responseConstraint: {
            type: 'object',
            properties: {
              words: { type: 'string' },
              maxPrice: { type: 'number' },
              minPrice: { type: 'number' },
              graded: { type: 'boolean' },
              sort: { type: 'string', enum: ['cheapest', 'most expensive', 'newest', ''] },
              binder: { type: 'string', enum: [...titles, ''] },
            },
            required: ['words'],
          },
        });
        const a = JSON.parse(reply);
        const q = [String(a.words || '').slice(0, 80), a.graded === true ? 'graded' : '', a.maxPrice > 0 ? `under ${Math.round(a.maxPrice)}` : '', a.minPrice > 0 ? `over ${Math.round(a.minPrice)}` : '', a.sort || ''].filter(Boolean).join(' ');
        const binder = binders.find((b) => b.title === a.binder);
        dispatchEvent(new CustomEvent('vault:ask', { detail: { q, binder: binder ? binder.handle : undefined } }));
        note.textContent = '';
        refresh();
      } catch (e) {
        console.debug('On-device AI failed', e);
        note.textContent = 'Couldn’t work that out. Try just the card name.';
      }
      btn.disabled = false;
    });
    const seen = new MutationObserver(refresh);
    seen.observe(document.documentElement, { attributes: true, attributeFilter: ['data-finder'] });
    refresh();
  }
})();
