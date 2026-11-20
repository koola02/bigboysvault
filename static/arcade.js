// Vault Arcade (templates/arcade.html; scores: src/arcade.rs). Four light games, a phone first: no downloads, no 3D,
// only small moves. Each opens from its tile (#who, #memory, #types, #price) with Play and Today's Top 10; Back returns.
// Anyone can play; a logged-in trainer's score is saved when a game ends (the server keeps their best of the day).
(() => {
  const root = document.getElementById('arcade'); if (!root || !window.ArcadeDex) return;
  const D = window.ArcadeDex;
  const home = document.getElementById('arcHome'), stage = document.getElementById('arcStage');
  const signed = root.dataset.signed === 'true';
  let boards = {}; try { boards = JSON.parse(document.getElementById('arcBoards').textContent || '{}'); } catch (_) {}
  const NAMES = { who: 'Who’s That Pokémon?', memory: 'Memory Match', types: 'Type Challenge', price: 'Price Check' };

  // ---------- little helpers ----------
  const el = (tag, cls, text) => { const n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; };
  const btn = (cls, text, on) => { const b = el('button', cls, text); b.type = 'button'; if (on) b.addEventListener('click', on); return b; };
  const rnd = (n, r = Math.random) => Math.floor(r() * n);
  const shuffle = (a, r = Math.random) => { for (let i = a.length - 1; i > 0; i--) { const j = rnd(i + 1, r); [a[i], a[j]] = [a[j], a[i]]; } return a; };
  const peso = (n) => '₱' + Math.round(n).toLocaleString('en-US');
  const clock = (ms) => { const s = Math.round(ms / 1000); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
  // Today in the Philippines ("2026-11-20"): the day Type Challenge's questions and every leaderboard belong to.
  const today = () => new Date(Date.now() + 8 * 3600e3).toISOString().slice(0, 10);
  // The same random numbers for everyone on the same day (Type Challenge).
  const seeded = (text) => { let h = 1779033703 ^ text.length; for (let i = 0; i < text.length; i++) { h = Math.imul(h ^ text.charCodeAt(i), 3432918353); h = (h << 13) | (h >>> 19); } return () => { h = Math.imul(h ^ (h >>> 16), 2246822507); h = Math.imul(h ^ (h >>> 13), 3266489909); h ^= h >>> 16; return (h >>> 0) / 4294967296; }; };
  // Words on a type's colour: white on the dark ones, dark on the rest (easy to read either way).
  const DARK_TYPES = ['fighting', 'poison', 'ghost', 'dragon', 'dark'];
  const typeChip = (t) => { const s = el('span', 'arc-type', D.cap(t)); s.style.setProperty('--c', D.COLOR[t]); s.style.setProperty('--fg', DARK_TYPES.includes(t) ? '#fff' : '#10131f'); return s; };
  const store = { get: (k) => { try { return localStorage.getItem(k); } catch (_) { return null; } }, set: (k, v) => { try { localStorage.setItem(k, v); } catch (_) {} } };

  // ---------- the tiles: today's top score on each ----------
  function tops() {
    root.querySelectorAll('[data-top]').forEach((n) => {
      const b = boards[n.dataset.top] || [];
      n.textContent = b.length ? `Top Today: ${b[0].name} · ${b[0].score}` : 'No Scores Yet Today. Be The First!';
    });
  }

  // ---------- leaderboard ----------
  function detail(game, r) {
    if (game === 'memory') return `Board ${r.level} · ${r.moves} Moves · ${clock(r.ms)}`;
    if (game === 'types') return `${r.score}/10 · ${clock(r.ms)}`;
    if (game === 'who') return `Level ${r.level}`;
    return `${r.score} In A Row`;
  }
  function boardView(game) {
    const box = el('div', 'arc-board');
    const rows = boards[game] || [];
    if (!rows.length) { box.append(el('p', 'arc-empty', 'No scores yet today. Play now and take the top spot!')); return box; }
    const ol = el('ol', 'arc-rows');
    rows.forEach((r, i) => {
      const li = el('li', r.me ? 'me' : null);
      li.append(el('b', 'arc-rank', String(i + 1)));
      const av = el('span', 'av sm'); if (r.avatar) { const im = el('img'); im.src = r.avatar; im.alt = ''; av.append(im); } else av.textContent = (r.name || '?')[0].toUpperCase();
      const who = el('span', 'arc-who'); who.append(el('b', null, r.name), el('small', null, detail(game, r)));
      li.append(av, who, el('b', 'arc-score', String(r.score)));
      ol.append(li);
    });
    box.append(ol);
    return box;
  }

  // ---------- saving a score ----------
  async function save(game, data, out) {
    const line = el('p', 'arc-saved');
    out.append(line);
    if (!signed) {
      line.append('Log in to save your score and join the leaderboard. ');
      const a = el('a', null, 'Log In'); a.href = '/login?next=/arcade%23' + game; line.append(a);
      return;
    }
    line.textContent = 'Saving…';
    try {
      const r = await fetch(`/arcade/${game}/score`, { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json', Accept: 'application/json' }, body: JSON.stringify(data) });
      const j = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(j.error || 'Your score didn’t save. Please try again.');
      boards[game] = j.board || boards[game]; tops();
      line.textContent = j.kept === 'best' ? `Saved! You’re #${j.rank} today.` : j.kept === 'already' ? `Today’s challenge was already counted. You’re #${j.rank} today.` : `Your best today is still higher. You’re #${j.rank}.`;
      line.classList.add('ok');
    } catch (e) { line.textContent = e.message; line.classList.add('bad'); }
  }

  // ---------- the game frame ----------
  let stop = () => {};
  function frame(game) {
    stop(); stop = () => {};
    stage.replaceChildren();
    const top = el('div', 'arc-bar');
    top.append(btn('btn ghost sm arc-out', '‹ Arcade', () => { location.hash = ''; }), el('h1', 'arc-gt', NAMES[game]));
    const tabs = el('div', 'arc-tabs'); tabs.setAttribute('role', 'tablist');
    const play = btn('on', 'Play'), lead = btn(null, 'Today’s Top 10');
    [play, lead].forEach((b) => b.setAttribute('role', 'tab'));
    tabs.append(play, lead);
    const body = el('div', 'arc-body t-' + game);
    stage.append(top, tabs, body);
    const show = (which) => {
      play.classList.toggle('on', which === 'play'); lead.classList.toggle('on', which === 'lead');
      play.setAttribute('aria-selected', String(which === 'play')); lead.setAttribute('aria-selected', String(which === 'lead'));
      body.hidden = which !== 'play';
      stage.querySelector('.arc-board')?.remove();
      if (which === 'lead') stage.append(boardView(game));
    };
    play.addEventListener('click', () => show('play')); lead.addEventListener('click', () => show('lead'));
    return { body, show };
  }

  // A screen at the end of a game: what you got, saving, and play again.
  function finish(body, game, title, lines, data, again, f) {
    body.replaceChildren();
    const card = el('div', 'arc-end');
    card.append(el('h2', 'arc-end-t', title));
    lines.forEach((l) => card.append(el('p', 'arc-end-l', l)));
    const acts = el('div', 'arc-acts');
    acts.append(btn('btn arc-go', 'Play Again', again), btn('btn ghost', 'Today’s Top 10', () => f.show('lead')));
    card.append(acts);
    body.append(card);
    save(game, data, card);
  }

  // ===================== 1. Who's That Pokémon? =====================
  // Name the silhouette. Five right answers make a level: level 1 is the best-known Pokémon with three answers to pick
  // from; then all 151 with four; from level 3 the wrong answers share a type; from level 4 there are 10 seconds.
  // Three misses end the run. Each right answer is worth 10 points times the level.
  function who() {
    const f = frame('who');
    let st;
    const start = () => { st = { right: 0, score: 0, lives: 3, seen: new Set(), t0: Date.now() }; ask(); };
    const level = () => 1 + Math.floor(st.right / 5);
    function ask() {
      const lv = level();
      const pool = lv === 1 ? D.EASY.map(D.byId) : D.MONS;
      let fresh = pool.filter((m) => !st.seen.has(m.id)); if (fresh.length < 4) { st.seen.clear(); fresh = pool; }
      const mon = fresh[rnd(fresh.length)]; st.seen.add(mon.id);
      const n = lv === 1 ? 3 : 4;
      let others = D.MONS.filter((m) => m.id !== mon.id && (lv === 1 ? D.EASY.includes(m.id) : true));
      if (lv >= 3) { const same = others.filter((m) => m.types[0] === mon.types[0]); if (same.length >= n - 1) others = same; }
      const choices = shuffle([mon, ...shuffle(others.slice()).slice(0, n - 1)]);
      f.body.replaceChildren();
      const hud = el('div', 'arc-hud');
      hud.append(el('span', null, `Level ${lv}`), el('span', null, `Score ${st.score}`), el('span', 'arc-lives', '♥'.repeat(st.lives) + '♡'.repeat(3 - st.lives)));
      const pic = el('div', 'arc-who-pic');
      const img = el('img', 'arc-shadow'); img.alt = 'A Pokémon In Shadow'; img.src = D.art(mon.id); img.width = 240; img.height = 240;
      pic.append(img);
      const say = el('p', 'arc-who-say', 'Who’s that Pokémon?');
      const opts = el('div', 'arc-opts');
      let timer = null, done = false;
      const bar = el('div', 'arc-time'); const fill = el('i'); bar.append(fill);
      const answer = (m, b) => {
        if (done) return; done = true; clearInterval(timer);
        const good = m && m.id === mon.id;
        img.classList.remove('arc-shadow'); img.classList.add('arc-reveal');
        say.textContent = good ? `Yes! It’s ${mon.name}!` : `It’s ${mon.name}!`; say.classList.add(good ? 'good' : 'bad');
        opts.querySelectorAll('button').forEach((x) => { x.disabled = true; if (x.dataset.id === String(mon.id)) x.classList.add('right'); });
        if (b && !good) b.classList.add('wrong');
        if (good) { st.right++; st.score += 10 * lv; } else st.lives--;
        setTimeout(() => (st.lives > 0 ? ask() : over()), good ? 1100 : 1700);
      };
      choices.forEach((m) => { const b = btn('arc-opt', m.name, () => answer(m, b)); b.dataset.id = m.id; opts.append(b); });
      f.body.append(hud, pic, say, opts);
      if (lv >= 4) {
        const t0 = Date.now(), total = 10000; f.body.insertBefore(bar, say);
        timer = setInterval(() => { const left = total - (Date.now() - t0); fill.style.width = `${Math.max(0, left / total) * 100}%`; if (left <= 0) answer(null, null); }, 100);
        stop = () => clearInterval(timer);
      }
    }
    function over() {
      finish(f.body, 'who', 'Game Over!', [`Score: ${st.score}`, `You reached level ${level()} with ${st.right} right.`], { score: st.score, level: level(), moves: st.right, ms: Date.now() - st.t0 }, start, f);
    }
    const intro = el('div', 'arc-intro');
    intro.append(el('p', null, 'A Pokémon hides in shadow. Pick its name! Every five right answers is a new level, and it gets harder. Three misses and the run ends.'), btn('btn arc-go', 'Start', start));
    f.body.append(intro);
  }

  // ===================== 2. Memory Match =====================
  // Find the pairs. Board 1 is 6×6, board 2 is 6×8, board 3 is 8×8. Each board is worth 100 points a pair, less 10 for
  // each extra move and 2 for each second (at least 50). The score after each board is saved.
  const BOARDS = [{ cols: 6, rows: 6 }, { cols: 6, rows: 8 }, { cols: 8, rows: 8 }];
  function memory() {
    const f = frame('memory');
    let run;
    const start = () => { run = { board: 0, total: 0, moves: 0, ms: 0 }; play(); };
    function play() {
      const { cols, rows } = BOARDS[run.board];
      const pairs = (cols * rows) / 2;
      const ids = shuffle(D.MONS.map((m) => m.id)).slice(0, pairs);
      const deck = shuffle([...ids, ...ids]);
      let first = null, lock = false, moves = 0, found = 0, t0 = 0, tick = null;
      f.body.replaceChildren();
      const hud = el('div', 'arc-hud');
      const mv = el('span', null, 'Moves 0'), tm = el('span', null, '0:00');
      hud.append(el('span', null, `Board ${run.board + 1} Of 3`), mv, tm, el('span', null, `Score ${run.total}`));
      const grid = el('div', 'arc-mem'); grid.style.setProperty('--cols', cols);
      // The whole board fits the screen: never wider than 560px, nor so tall it needs scrolling.
      grid.style.maxWidth = `min(100%, 560px, calc((100dvh - 240px) * ${cols} / ${rows}))`;
      deck.forEach((id) => {
        const c = btn('arc-cell'); c.dataset.id = id; c.setAttribute('aria-label', 'A Hidden Card');
        const face = el('img'); face.alt = ''; face.loading = 'lazy'; face.src = D.sprite(id); face.width = 96; face.height = 96;
        c.append(el('span', 'arc-back'), face);
        c.addEventListener('click', () => {
          if (lock || c.classList.contains('on') || c.classList.contains('done')) return;
          if (!t0) { t0 = Date.now(); tick = setInterval(() => { tm.textContent = clock(Date.now() - t0); }, 500); stop = () => clearInterval(tick); }
          c.classList.add('on'); c.setAttribute('aria-label', D.byId(id).name);
          if (!first) { first = c; return; }
          moves++; mv.textContent = `Moves ${moves}`;
          if (first.dataset.id === c.dataset.id) {
            first.classList.add('done'); c.classList.add('done'); first = null; found++;
            if (found === pairs) cleared();
          } else {
            lock = true; const a = first; first = null;
            setTimeout(() => { a.classList.remove('on'); c.classList.remove('on'); a.setAttribute('aria-label', 'A Hidden Card'); c.setAttribute('aria-label', 'A Hidden Card'); lock = false; }, 800);
          }
        });
        grid.append(c);
      });
      f.body.append(hud, grid);
      function cleared() {
        clearInterval(tick);
        const ms = Date.now() - t0, secs = Math.round(ms / 1000);
        const pts = Math.max(50, pairs * 100 - (moves - pairs) * 10 - secs * 2);
        run.total += pts; run.moves += moves; run.ms += ms;
        const last = run.board === BOARDS.length - 1;
        const data = { score: run.total, level: run.board + 1, moves: run.moves, ms: run.ms };
        if (last) {
          finish(f.body, 'memory', 'All Boards Cleared!', [`Score: ${run.total}`, `${run.moves} moves in ${clock(run.ms)}.`], data, start, f);
        } else {
          const card = el('div', 'arc-end');
          card.append(el('h2', 'arc-end-t', `Board ${run.board + 1} Cleared!`), el('p', 'arc-end-l', `+${pts} points · ${moves} moves · ${clock(ms)}`), el('p', 'arc-end-l', `Score So Far: ${run.total}`));
          const acts = el('div', 'arc-acts');
          acts.append(btn('btn arc-go', `Next Board (${BOARDS[run.board + 1].cols}×${BOARDS[run.board + 1].rows})`, () => { run.board++; play(); }));
          card.append(acts);
          f.body.replaceChildren(card);
          save('memory', data, card);
        }
      }
    }
    const intro = el('div', 'arc-intro');
    intro.append(el('p', null, 'Flip two cards at a time and find every pair. Three boards: 6×6, then 6×8, then 8×8. Fewer moves and a faster time score more.'), btn('btn arc-go', 'Start', start));
    f.body.append(intro);
  }

  // ===================== 3. Type Challenge =====================
  // The same 10 questions for everyone today: a Pokémon's type, or which type hits a type (or a Pokémon) hard. The first
  // try of the day goes on the leaderboard; another try is practice.
  function questions(day) {
    const r = seeded('types:' + day), qs = [];
    const mons = shuffle(D.MONS.slice(), r);
    for (let i = 0; i < 10; i++) {
      const mon = mons[i];
      if (i % 3 === 2) {
        // Which type is super effective against this type (or this one-type Pokémon)?
        const single = mons.slice(10).find((m) => m.types.length === 1 && D.beats(m.types[0]).length);
        const useMon = i === 5 && single;
        const hittable = D.TYPES.filter((x) => D.beats(x).length);
        const t = useMon ? single.types[0] : hittable[rnd(hittable.length, r)];
        const good = D.beats(t), right = good[rnd(good.length, r)];
        const wrong = shuffle(D.TYPES.filter((x) => !good.includes(x)), r).slice(0, 3);
        qs.push({ kind: 'beats', t, mon: useMon ? single : null, right, opts: shuffle([right, ...wrong], r).map((x) => [x]) });
      } else {
        // What type is this Pokémon?
        const right = mon.types.join('/');
        const seen = new Set([right]), opts = [mon.types];
        for (const m of shuffle(D.MONS.slice(), r)) { const k = m.types.join('/'); if (!seen.has(k) && m.types.length === mon.types.length) { seen.add(k); opts.push(m.types); } if (opts.length === 4) break; }
        qs.push({ kind: 'type', mon, right, opts: shuffle(opts, r) });
      }
    }
    return qs;
  }
  function types() {
    const f = frame('types');
    const day = today(), key = 'arc-types-' + day;
    let st;
    const start = () => { st = { i: 0, right: 0, t0: Date.now(), qs: questions(day), practice: store.get(key) === 'done' }; ask(); };
    function ask() {
      const q = st.qs[st.i];
      f.body.replaceChildren();
      const hud = el('div', 'arc-hud'); hud.append(el('span', null, `Question ${st.i + 1} Of 10`), el('span', null, `Right ${st.right}`), el('span', null, st.practice ? 'Practice' : 'Counts Today'));
      const card = el('div', 'arc-tq');
      if (q.kind === 'type') {
        const img = el('img', 'arc-reveal'); img.alt = q.mon.name; img.src = D.art(q.mon.id); img.width = 200; img.height = 200;
        card.append(img, el('p', 'arc-tq-q', `What type is ${q.mon.name}?`));
      } else if (q.mon) {
        const img = el('img', 'arc-reveal'); img.alt = q.mon.name; img.src = D.art(q.mon.id); img.width = 200; img.height = 200;
        card.append(img, el('p', 'arc-tq-q', `Which type is super effective against ${q.mon.name}?`));
      } else {
        const big = el('div', 'arc-bigtype'); big.append(typeChip(q.t));
        card.append(big, el('p', 'arc-tq-q', `Which type is super effective against ${D.cap(q.t)}?`));
      }
      const opts = el('div', 'arc-opts arc-topts');
      let done = false;
      q.opts.forEach((o) => {
        const k = o.join('/'), b = btn('arc-opt');
        o.forEach((t) => b.append(typeChip(t)));
        b.setAttribute('aria-label', o.map(D.cap).join(' And '));
        b.addEventListener('click', () => {
          if (done) return; done = true;
          const good = q.kind === 'type' ? k === q.right : o[0] === q.right;
          opts.querySelectorAll('button').forEach((x, j) => { x.disabled = true; const ok = q.kind === 'type' ? q.opts[j].join('/') === q.right : q.opts[j][0] === q.right; if (ok) x.classList.add('right'); });
          if (!good) b.classList.add('wrong'); else st.right++;
          setTimeout(() => { st.i++; if (st.i < 10) ask(); else end(); }, good ? 900 : 1500);
        });
        opts.append(b);
      });
      f.body.append(hud, card, opts);
    }
    function end() {
      const ms = Date.now() - st.t0, practice = st.practice;
      store.set(key, 'done');
      const lines = [`You got ${st.right} of 10 in ${clock(ms)}.`, practice ? 'This was practice: today’s first try is the one on the leaderboard.' : 'New questions tomorrow!'];
      if (practice) {
        f.body.replaceChildren();
        const card = el('div', 'arc-end'); card.append(el('h2', 'arc-end-t', 'Practice Done!'));
        lines.forEach((l) => card.append(el('p', 'arc-end-l', l)));
        const acts = el('div', 'arc-acts'); acts.append(btn('btn arc-go', 'Practice Again', start), btn('btn ghost', 'Today’s Top 10', () => f.show('lead'))); card.append(acts);
        f.body.append(card);
      } else finish(f.body, 'types', `${st.right}/10!`, lines, { score: st.right, level: 0, moves: 10, ms }, start, f);
    }
    const intro = el('div', 'arc-intro');
    const played = store.get(key) === 'done';
    intro.append(el('p', null, played ? 'You’ve played today’s challenge. Practice it again, or come back tomorrow for 10 new questions.' : 'Ten questions, the same for everyone today: a Pokémon’s type, or which type is super effective. Your first try today goes on the leaderboard.'), btn('btn arc-go', played ? 'Practice' : 'Start', start));
    f.body.append(intro);
  }

  // ===================== 4. Price Check =====================
  // Two real cards for sale on Big Boys Vault: tap the one that costs more. Each right pick adds one to the streak; a
  // wrong pick ends the run. The streak is the score.
  function price() {
    const f = frame('price');
    let cards = [], st;
    const load = async () => {
      const r = await fetch('/arcade/cards', { credentials: 'same-origin', headers: { Accept: 'application/json' } });
      const j = await r.json().catch(() => ({}));
      cards = shuffle((j.cards || []).filter((c) => c.price > 0 && c.img));
    };
    const start = async () => {
      st = { streak: 0, t0: Date.now() };
      f.body.replaceChildren(el('p', 'arc-empty', 'Shuffling the cards…'));
      try { await load(); } catch (_) {}
      if (cards.length < 2) { f.body.replaceChildren(el('p', 'arc-empty', 'Not enough cards for sale right now. Please try again later.')); return; }
      round();
    };
    // Two cards whose prices differ by at least 15%, so there is a real answer.
    const pair = async () => {
      for (let tries = 0; tries < 2; tries++) {
        for (let i = 0; i < cards.length; i++) for (let j = i + 1; j < cards.length; j++) {
          const a = cards[i], b = cards[j];
          if (a.title !== b.title && Math.max(a.price, b.price) >= Math.min(a.price, b.price) * 1.15) { cards.splice(j, 1); cards.splice(i, 1); return [a, b]; }
        }
        await load();
      }
      return null;
    };
    async function round() {
      const p = await pair();
      if (!p) { over(); return; }
      f.body.replaceChildren();
      const hud = el('div', 'arc-hud'); hud.append(el('span', null, `Streak ${st.streak}`), el('span', null, 'Tap The Pricier Card'));
      const duo = el('div', 'arc-duo');
      let done = false;
      const tiles = p.map((c) => {
        const b = btn('arc-pc');
        const img = el('img'); img.src = c.img; img.alt = ''; img.loading = 'eager'; img.onerror = () => { img.style.visibility = 'hidden'; };
        const info = el('span', 'arc-pc-i'); info.append(el('b', null, c.title), el('small', null, c.seller));
        const tag = el('span', 'arc-pc-p', '₱?');
        b.append(img, info, tag);
        b.addEventListener('click', () => {
          if (done) return; done = true;
          const other = p[0] === c ? p[1] : p[0], good = c.price > other.price;
          tiles.forEach((t, k) => { t.disabled = true; t.querySelector('.arc-pc-p').textContent = peso(p[k].price); if (p[k].price > (p[1 - k].price)) t.classList.add('right'); });
          if (!good) b.classList.add('wrong'); else st.streak++;
          setTimeout(() => (good ? round() : over()), good ? 1100 : 1800);
        });
        return b;
      });
      duo.append(tiles[0], el('span', 'arc-vs', 'VS'), tiles[1]);
      f.body.append(hud, duo);
    }
    function over() {
      finish(f.body, 'price', st.streak ? `${st.streak} In A Row!` : 'So Close!', [`Your Streak: ${st.streak}`, 'Every card here is really for sale on Big Boys Vault.'], { score: st.streak, level: 0, moves: st.streak, ms: Date.now() - st.t0 }, start, f);
    }
    const intro = el('div', 'arc-intro');
    intro.append(el('p', null, 'Two real cards for sale on Big Boys Vault. Tap the one that costs more! Keep your streak going: one wrong pick ends the run.'), btn('btn arc-go', 'Start', start));
    f.body.append(intro);
  }

  // ---------- which screen ----------
  const GAMES = { who, memory, types, price };
  function route() {
    const g = location.hash.slice(1);
    if (GAMES[g]) { home.hidden = true; stage.hidden = false; GAMES[g](); window.scrollTo(0, 0); }
    else { stop(); stop = () => {}; stage.hidden = true; stage.replaceChildren(); home.hidden = false; }
  }
  window.addEventListener('hashchange', route);
  tops(); route();
})();
