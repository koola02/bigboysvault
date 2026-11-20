// Account Settings › Manage Binders (templates/me_collection.html): the binders of a person's own page and their cards.
// Each change saves at once and the page comes back where it was.
(() => {
  const root = document.getElementById('meCol'); if (!root) return;
  const say = (msg, bad) => {
    let t = document.querySelector('.me-toast');
    if (!t) { t = document.createElement('div'); t.className = 'me-toast'; t.setAttribute('role', 'status'); document.body.append(t); }
    t.textContent = msg; t.classList.toggle('bad', !!bad); t.classList.add('on');
    clearTimeout(say.t); say.t = setTimeout(() => t.classList.remove('on'), bad ? 5000 : 2400);
  };
  async function send(url, body) {
    let r;
    try { r = await fetch(url, { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify(body ?? {}) }); } catch (_) { throw new Error('No connection. Please try again.'); }
    const j = await r.json().catch(() => ({}));
    if (r.status === 401) { location.href = '/login?next=/me/collection'; throw new Error('Please log in again.'); }
    if (!r.ok) throw new Error(j.error || 'That didn’t save. Please try again.');
    return j;
  }
  const sure = (q, opt) => (window.bbvConfirm ? window.bbvConfirm(q, opt) : Promise.resolve(confirm(q)));
  // Back on the page at the binder that changed.
  const reload = (hash) => { if (hash) history.replaceState(null, '', `${location.pathname}#${hash}`); location.reload(); };

  // A new binder.
  const nf = root.querySelector('[data-new]');
  nf.addEventListener('submit', async (e) => {
    e.preventDefault();
    const btn = nf.querySelector('button'); btn.disabled = true;
    try { const j = await send('/me/collection', { title: nf.elements.title.value }); say('Binder Added ✓'); reload(`b${j.id}`); }
    catch (err) { say(err.message, true); btn.disabled = false; }
  });
  if (location.hash === '#new') setTimeout(() => nf.elements.title.focus(), 50);

  root.addEventListener('click', async (e) => {
    const t = e.target;
    const sec = t.closest('[data-b]'), bid = sec?.dataset.b;
    if (t.closest('[data-b-edit]')) { const f = sec.querySelector('[data-b-form]'); f.hidden = !f.hidden; if (!f.hidden) f.elements.title.focus(); return; }
    if (t.closest('[data-cancel]')) { t.closest('form').hidden = true; return; }
    const mv = t.closest('[data-b-move]');
    if (mv) { try { await send(`/me/collection/${bid}`, { action: 'move', by: Number(mv.dataset.bMove) }); reload(`b${bid}`); } catch (err) { say(err.message, true); } return; }
    const del = t.closest('[data-b-del]');
    if (del) {
      const n = Number(del.dataset.n) || 0, name = sec.querySelector('h2').firstChild.textContent.trim();
      const q = n ? `Delete “${name}” and the ${n} card${n === 1 ? '' : 's'} in it? This can’t be undone.` : `Delete “${name}”?`;
      if (!(await sure(q, { yes: 'Delete', no: 'Keep It' }))) return;
      try { await send(`/me/collection/${bid}`, { action: 'delete' }); say('Binder Deleted'); reload(); } catch (err) { say(err.message, true); }
      return;
    }
    const li = t.closest('[data-c]');
    if (li && t.closest('[data-c-edit]')) {
      const f = li.querySelector('[data-c-form]'); f.hidden = !f.hidden;
      if (!f.hidden) { if (bid) f.elements.binder.value = bid; f.elements.title.focus(); }
      return;
    }
    if (li && t.closest('[data-c-del]')) {
      const name = li.querySelector('.mcol-t').textContent.trim();
      if (!(await sure(`Take “${name}” out of your collection?`, { yes: 'Remove', no: 'Keep It' }))) return;
      try { await send(`/me/cards/${li.dataset.c}`, { action: 'remove' }); say('Card Removed'); li.remove(); } catch (err) { say(err.message, true); }
    }
  });

  root.addEventListener('submit', async (e) => {
    const bf = e.target.closest('[data-b-form]'), cf = e.target.closest('[data-c-form]');
    if (!bf && !cf) return;
    e.preventDefault();
    try {
      if (bf) {
        const bid = bf.closest('[data-b]').dataset.b;
        await send(`/me/collection/${bid}`, { action: 'edit', title: bf.elements.title.value, description: bf.elements.description.value });
        say('Saved ✓'); reload(`b${bid}`);
      } else {
        const li = cf.closest('[data-c]'), to = cf.elements.binder.value;
        if (!to) { say('Make a binder first, then put the card in it.', true); return; }
        await send(`/me/collection/cards/${li.dataset.c}`, { title: cf.elements.title.value, binder: Number(to) });
        say('Saved ✓'); reload(`b${to}`);
      }
    } catch (err) { say(err.message, true); }
  });
})();
