// Suggested prices by condition, for the owner, admins and sellers alike.
// The same rule as src/pricing.rs: a share of the Near Mint market price, rounded the way people price cards.
window.Price = (() => {
  const DEFAULTS = { 'Near Mint': 1, 'LP-NM': 0.9, 'Lightly Played': 0.8, 'MP-LP': 0.7, 'Moderately Played': 0.6, 'HP-MP': 0.5, 'Heavily Played': 0.4, 'Damaged-HP': 0.32, 'Damaged': 0.25 };
  const NAMES = Object.keys(DEFAULTS);
  let shares = { ...DEFAULTS };

  // The owner can change the shares. Pages that price cards fetch them once, then repaint on 'bbv:shares'.
  const ready = fetch('/api/condition-shares', { credentials: 'same-origin', headers: { Accept: 'application/json' } })
    .then((r) => (r.ok ? r.json() : null))
    .then((j) => {
      if (j && typeof j === 'object') {
        for (const k of NAMES) if (typeof j[k] === 'number' && isFinite(j[k])) shares[k] = j[k];
        document.dispatchEvent(new Event('bbv:shares'));
      }
    })
    .catch(() => {});

  // Every way a condition is written on the site: "Lightly Played", "LP", "near-mint / reverse-holofoil"...
  function read(text) {
    if (!text) return null;
    const first = String(text).split('/')[0].split('·')[0].trim().toLowerCase().replace(/[_.]/g, ' ');
    const words = first.replace(/-/g, ' ').split(/\s+/).filter(Boolean);
    const compact = words.join('');
    const direct = {
      nearmint: 'Near Mint', nm: 'Near Mint', mint: 'Near Mint', pristine: 'Near Mint',
      lpnm: 'LP-NM', nmlp: 'LP-NM', lightlyplayed: 'Lightly Played', lp: 'Lightly Played',
      mplp: 'MP-LP', lpmp: 'MP-LP', moderatelyplayed: 'Moderately Played', mp: 'Moderately Played', modplayed: 'Moderately Played',
      hpmp: 'HP-MP', mphp: 'HP-MP', heavilyplayed: 'Heavily Played', hp: 'Heavily Played', heavyplayed: 'Heavily Played',
      damagedhp: 'Damaged-HP', hpdamaged: 'Damaged-HP', dmghp: 'Damaged-HP', hpdmg: 'Damaged-HP', damaged: 'Damaged', dmg: 'Damaged',
    };
    if (direct[compact]) return direct[compact];
    const has = (w) => words.includes(w);
    if (has('mint') || has('pristine')) return 'Near Mint';
    if (has('lp') && has('nm')) return 'LP-NM';
    if (has('mp') && has('lp')) return 'MP-LP';
    if (has('hp') && has('mp')) return 'HP-MP';
    if ((has('damaged') || has('dmg')) && has('hp')) return 'Damaged-HP';
    if (has('lightly') || words[0] === 'lp') return 'Lightly Played';
    if (has('moderately') || has('mod') || words[0] === 'mp') return 'Moderately Played';
    if (has('heavily') || has('heavy') || words[0] === 'hp') return 'Heavily Played';
    if (has('damaged') || words[0] === 'dmg') return 'Damaged';
    return null;
  }
  function graded(c) {
    const t = String(c || '').trim().toLowerCase();
    const first = t.split(/\s+/)[0] || '';
    return ['psa', 'cgc', 'bgs', 'sgc', 'ace', 'tag'].some((g) => first === g || first.startsWith(g))
      || t.includes('gem mt') || t.includes('gem-mt') || t.includes('graded') || t.includes('slab');
  }
  const sealed = (c) => String(c || '').toLowerCase().includes('sealed');
  function share(c) {
    if (graded(c) || sealed(c)) return null;
    const std = read(c);
    return std ? shares[std] : null;
  }
  // Whole pesos: under ₱100 to the nearest ₱1, up to ₱1,000 to the nearest ₱5, above that to the nearest ₱10. Never below ₱1.
  function round(raw) {
    if (!(raw > 0)) return 1;
    const r = raw < 100 ? Math.round(raw) : raw <= 1000 ? Math.round(raw / 5) * 5 : Math.round(raw / 10) * 10;
    return Math.max(1, r);
  }
  function suggest(nearMint, cond) {
    const nm = Number(nearMint) || 0;
    if (nm <= 0) return null;
    const s = share(cond);
    return s == null ? null : round(nm * s);
  }
  const peso = (n) => '₱' + Math.round(Number(n) || 0).toLocaleString('en-US');
  const shortDate = (d) => {
    const t = Date.parse(d);
    return isNaN(t) ? String(d || '') : new Date(t).toLocaleDateString('en-PH', { day: 'numeric', month: 'short' });
  };

  /* One clear line under a price box, and a "Use ₱X" button.
     host: the element to fill. o: { nm, cond, label, source, updated, price, own, onUse } */
  function line(host, o) {
    if (!host) return null;
    host.classList.add('sug');
    host.replaceChildren();
    const txt = document.createElement('span'); txt.className = 'sug-t';
    host.append(txt);
    const nm = Math.round(Number(o.nm) || 0);
    if (o.ownPhoto) { txt.textContent = 'No market price for your own photo. Set your own value.'; return null; }
    if (graded(o.cond)) { txt.textContent = nm > 0 ? `Graded card: set your own price. Ungraded Near Mint is ${peso(nm)}.` : 'Graded card: set your own price.'; return null; }
    if (sealed(o.cond)) { txt.textContent = nm > 0 ? `Sealed: set your own price. Near Mint single is ${peso(nm)}.` : 'Sealed: set your own price.'; return null; }
    if (nm <= 0) { txt.textContent = 'No market price found for this card. Set your own price.'; return null; }
    const std = read(o.cond);
    const sug = suggest(nm, o.cond);
    if (sug == null) { txt.textContent = `Condition not known, so no suggestion. Near Mint is ${peso(nm)}.`; return null; }
    const where = [o.label, o.source, o.updated ? shortDate(o.updated) : ''].filter(Boolean).join(', ');
    const pct = Math.round(shares[std] * 100);
    const own = Number(o.own) || 0;
    if (own > 0) txt.textContent = `You set ${peso(own)}. Suggested for ${std}: ${peso(sug)}`;
    else { const b = document.createElement('b'); b.textContent = peso(sug); txt.append('Suggested ', b, ` · ${std} · ${pct}% of Near Mint ${peso(nm)}`); }
    if (own <= 0 && where) txt.append(` (${where})`);
    const cur = Number(String(o.price ?? '').replace(/[^\d.]/g, '')) || 0;
    if (o.onUse && cur !== sug) {
      const use = document.createElement('button'); use.type = 'button'; use.className = 'sug-use'; use.dataset.nodrag = '';
      use.textContent = `Use ${peso(sug)}`;
      use.addEventListener('click', (e) => { e.preventDefault(); e.stopPropagation(); o.onUse(sug); });
      host.append(use);
    }
    return sug;
  }

  return { NAMES, read, graded, sealed, share, round, suggest, line, ready, shares: () => ({ ...shares }) };
})();
