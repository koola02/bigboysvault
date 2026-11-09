// For admins and moderators only: one floating button that edits whatever is in front of you.
// A card opens its edit form, a trainer opens their row in Admin › Trainers, a binder or the home page opens its editor.
// Moderators only see what's theirs to change. Nobody else ever sees the button.
(() => {
  if (document.querySelector('.admin, #edList')) return;
  let who = null;
  const btn = document.createElement('a');
  btn.className = 'staff-fab';
  btn.hidden = true;
  btn.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 20h4L19 9l-4-4L4 16z"/><path d="M14 6l4 4"/></svg><span></span>';
  const word = btn.querySelector('span');
  // A trainer's own card can be taken down by staff too.
  const off = document.createElement('button');
  off.type = 'button'; off.className = 'staff-off'; off.hidden = true;
  off.innerHTML = '<span>Take this card down</span>';

  function context() {
    const scope = document.querySelector('dialog.float[open] .float-panel') || document.querySelector('main');
    if (!scope) return null;
    const full = !!who?.full, staff = !!who?.admin;
    // Browsing cards and binders stays clear: no edit button on them, open or on their own pages.
    const open = !!document.querySelector('dialog.float[open]');
    if (open ? !!scope.querySelector('.bf, [data-hit]') : /^\/(all|search|collections|products)(\/|$)/.test(location.pathname)) return null;
    const card = scope.querySelector('[data-hit]');
    if (card && full) return { href: `/admin/edit/${card.dataset.hit}`, text: 'Edit this card' };
    const mcard = scope.querySelector('[data-mcard]');
    const trainer = scope.querySelector('[data-trainer]') || mcard;
    const theirs = trainer && (full || !trainer.dataset.ownerRole);
    if (trainer && staff && theirs) return { href: `/admin/trainers#trainer-${trainer.dataset.trainer || trainer.dataset.owner}`, text: 'Manage this trainer', mcard: mcard?.dataset.mcard };
    if (scope.querySelector('.bf') && full) return { href: '/admin/binders', text: 'Edit binders' };
    if (document.getElementById('hxHero') && full && !document.querySelector('dialog.float[open]')) return { href: '/admin/page', text: 'Edit the home page' };
    if (full) return { href: '/admin', text: 'Admin' };
    if (staff) return { href: '/admin/trainers', text: 'Admin' };
    return null;
  }
  function paint() {
    if (!who) return;
    const c = context();
    btn.hidden = !c;
    if (!c) { off.hidden = true; return; }
    btn.href = c.href; word.textContent = c.text;
    off.hidden = !c.mcard; off.dataset.id = c.mcard || '';
    // Inside the window, the button lives in the window, or the window would cover it.
    const host = document.querySelector('dialog.float[open]') || document.body;
    if (btn.parentNode !== host) host.append(btn, off);
  }
  off.addEventListener('click', async () => {
    if (!off.dataset.id || !confirm('Take this card off the trainer\u2019s page? This can\u2019t be undone.')) return;
    const r = await fetch(`/admin/member-cards/${off.dataset.id}/delete`, { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' } });
    if (r.ok) { off.hidden = true; history.back(); setTimeout(() => location.reload(), 350); }
    else alert((await r.json().catch(() => ({}))).error || 'That didn\u2019t work.');
  });
  fetch('/me/who', { credentials: 'same-origin', headers: { Accept: 'application/json' } }).then((r) => r.json()).then((j) => {
    if (!j.admin && !j.full) return;
    who = j;
    paint();
    document.addEventListener('float:filled', paint);
    addEventListener('popstate', () => setTimeout(paint, 60));
    new MutationObserver(() => setTimeout(paint, 30)).observe(document.documentElement, { attributes: true, attributeFilter: ['data-float'] });
  }).catch(() => {});
})();
