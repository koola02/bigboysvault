// Tags you type: each one becomes a highlighted chip. Enter or a comma adds it, ✕ or Backspace takes it away.
// A box with data-target keeps that (hidden) field filled in for a form; otherwise it announces 'tags:change' for the page to save.
(() => {
  const make = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const clean = (t) => t.replace(/,/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 30);
  function mount(box) {
    if (box.dataset.ready) return;
    box.dataset.ready = '1';
    const target = box.dataset.target && document.querySelector(box.dataset.target);
    const SYSTEM = /^(binder|finish|rarity|set)-/i;
    let kept = [];
    let tags = (target ? target.value.split(',') : (box.dataset.tags || '').split('|')).map((t) => t.trim()).filter(Boolean);
    if (target) { kept = tags.filter((t) => SYSTEM.test(t)); tags = tags.filter((t) => !SYSTEM.test(t)); }
    const input = make('input', 'tag-in'); input.type = 'text'; input.maxLength = 30; input.placeholder = '＋ Tag'; input.setAttribute('aria-label', 'Add a tag');
    input.setAttribute('enterkeyhint', 'done');
    const paint = () => {
      box.replaceChildren(...tags.map((t) => {
        const chip = make('span', 'tag-hl', t);
        const x = make('button', 'tag-x', '✕'); x.type = 'button'; x.setAttribute('aria-label', `Take off the tag ${t}`);
        x.addEventListener('click', () => { tags = tags.filter((y) => y !== t); changed(); });
        chip.append(x);
        return chip;
      }), input);
    };
    const changed = () => {
      paint();
      if (target) { target.value = [...kept, ...tags].join(', '); target.dispatchEvent(new Event('input', { bubbles: true })); }
      else box.dispatchEvent(new CustomEvent('tags:change', { bubbles: true, detail: { tags } }));
      input.focus({ preventScroll: true });
    };
    const add = () => {
      const t = clean(input.value);
      input.value = '';
      if (!t || tags.some((y) => y.toLowerCase() === t.toLowerCase()) || tags.length >= 12) return;
      tags.push(t); changed();
    };
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); add(); }
      else if (e.key === 'Backspace' && !input.value && tags.length) { tags.pop(); changed(); }
    });
    input.addEventListener('blur', () => { if (input.value.trim()) add(); });
    box.addEventListener('click', (e) => { if (e.target === box) input.focus(); });
    if (target) target.hidden = true;
    paint();
  }
  const scan = () => document.querySelectorAll('[data-tag-box]').forEach(mount);
  scan();
  document.addEventListener('htmx:afterSwap', scan);
  new MutationObserver(scan).observe(document.body, { childList: true, subtree: true });
})();
