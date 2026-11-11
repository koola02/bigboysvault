// For admins and moderators only: one floating button that edits whatever is in front of you.
// A card opens its edit form, a trainer opens their row in Admin › Trainers, a binder or the home page opens its editor.
// Moderators only see what's theirs to change. Nobody else ever sees the button.
(() => {
  // Hiding the admin buttons: remembered on this device, switched with any [data-staff-toggle] button (My profile,
  // the admin pages) or the small eye beside the admin button. The page's first script sets it before anything paints.
  const KEY = 'bbv:staff-hide';
  const hidden = () => document.documentElement.hasAttribute('data-staff-hide');
  const label = () => document.querySelectorAll('[data-staff-toggle]').forEach((b) => {
    b.setAttribute('aria-pressed', String(!hidden()));
    const t = b.querySelector('[data-staff-word]') || b; t.textContent = hidden() ? 'Show admin buttons' : 'Hide admin buttons';
  });
  const setHidden = (on) => {
    try { on ? localStorage.setItem(KEY, '1') : localStorage.removeItem(KEY); } catch (_) {}
    document.documentElement.toggleAttribute('data-staff-hide', on);
    label();
  };
  document.addEventListener('click', (e) => { if (e.target.closest('[data-staff-toggle]')) { e.preventDefault(); setHidden(!hidden()); } });
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', label); else label();
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
  const hide = document.createElement('button');
  hide.type = 'button'; hide.className = 'staff-hide'; hide.hidden = true;
  hide.title = 'Hide admin buttons (bring them back from My profile)'; hide.setAttribute('aria-label', 'Hide admin buttons');
  hide.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 3l18 18M10.6 6.1A10 10 0 0 1 12 6c5 0 9 6 9 6a17 17 0 0 1-3.2 3.7M6.6 6.6C4.2 8.2 3 12 3 12s4 6 9 6a9.3 9.3 0 0 0 4.4-1.1M9.9 9.9a3 3 0 0 0 4.2 4.2"/></svg>';
  hide.addEventListener('click', () => {
    setHidden(true);
    const t = document.createElement('div'); t.className = 'cart-toast on'; t.setAttribute('role', 'status');
    t.textContent = 'Admin buttons hidden. Show them again from My profile.';
    document.body.append(t); setTimeout(() => t.remove(), 3200);
  });

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
    btn.hidden = !c; hide.hidden = !c;
    if (!c) { off.hidden = true; return; }
    btn.href = c.href; word.textContent = c.text;
    off.hidden = !c.mcard; off.dataset.id = c.mcard || '';
    // Inside the window, the button lives in the window, or the window would cover it.
    const host = document.querySelector('dialog.float[open]') || document.body;
    if (btn.parentNode !== host) host.append(btn, hide, off);
  }
  off.addEventListener('click', async () => {
    if (!off.dataset.id || !confirm('Take this card off the trainer\u2019s page? This can\u2019t be undone.')) return;
    const r = await fetch(`/admin/member-cards/${off.dataset.id}/delete`, { method: 'POST', credentials: 'same-origin', headers: { Accept: 'application/json' } });
    if (r.ok) { off.hidden = true; history.back(); setTimeout(() => location.reload(), 350); }
    else alert((await r.json().catch(() => ({}))).error || 'That didn\u2019t work.');
  });
  // Who's here is asked once per page and shared (see vault-core.js).
  const bbvWho = () => (window.bbvWho ? window.bbvWho() : (window.bbvWhoAnswer ||= fetch('/me/who', { credentials: 'same-origin', headers: { Accept: 'application/json' } }).then((r) => (r.ok ? r.json() : {})).catch(() => ({}))));
  bbvWho().then((j) => {
    if (!j.admin && !j.full) return;
    who = j;
    paint();
    document.addEventListener('float:filled', paint);
    addEventListener('popstate', () => setTimeout(paint, 60));
    new MutationObserver(() => setTimeout(paint, 30)).observe(document.documentElement, { attributes: true, attributeFilter: ['data-float'] });
  }).catch(() => {});
})();
