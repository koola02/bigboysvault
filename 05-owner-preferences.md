# Owner preferences and standing rules (from the previous assistant's notes)


===== bbv-2526-theme.md =====
---
name: bbv-2526-theme
description: "BBV theme direction - the site is the vault 500 years in the future, emanating from the logo; applies to visuals, motion, logo behaviour and interactions"
metadata:
  type: feedback
---

Big Boys Vault's look is a Pokémon card vault in the year 2526, not brass-and-rivets 2026. The logo is the single source of everything: its gold light fills space, splits into the card-type colours, and a gold thread runs from it through the home page. Things float and glow rather than sitting in boxes and plates.

**Why:** the user rejected "plain" and then a riveted-vault concept mid-build: "something that doesn't exist in the current era", "500 years into the future", "not only the surface layout but the animations mechanisms logos actions and interactions". They gave full freedom over every pixel with no approval needed for theme work.

**How to apply:** new UI uses `static/vault.css` tokens and the shared interactions in `static/vault-core.js`; the home scene is raw WebGL2 in `static/vault-fx.js`. New features need future-feeling behaviour too, not just styling. Still bound by [[bbv-design-standards]] (a 10-year-old can use it, works on every device) and [[bbv-floating-and-consistency]] (condition colours and set symbols stay on every card surface).


===== bbv-binders-hero.md =====
---
name: bbv-binders-hero
description: Binders are the star of the BBV home page; real 3D binders that open only on tap; other home blocks stay small with arrow buttons
metadata:
  node_type: memory
  type: feedback
  originSessionId: 76f14029-25cc-495c-a0e7-45846eeb25e6
  modified: 2026-11-08T13:11:41.750Z
---

On the Big Boys Vault home page the binders section must stand out above everything else ("ultimate goal"). The ring is made of real 3D binders (leather cover, stitching, strap, spine, page edges, 9-pocket page). Every other home block is kept smaller and narrower. Stats were moved off the site to the admin Dashboard (2026-11-08).

**Why:** the user said the earlier binder look was "10% of what I desire" and asked that everything else shrink to highlight them. They then rejected the binder opening by itself: it must open only when clicked or tapped (first tap opens, second goes in).

**How to apply:**
- Never make home interactions auto-play or auto-open.
- Any sideways-scrolling row needs visible ‹ › buttons, because not everyone can scroll horizontally.
- Related: [[bbv-design-standards]], [[bbv-2526-theme]].


===== bbv-design-standards.md =====
---
name: bbv-design-standards
description: "Big Boys Vault UI rules the user insists on — every change must work on all devices, be child-simple, visual, on-brand, centered"
metadata:
  node_type: memory
  type: feedback
  originSessionId: 76f14029-25cc-495c-a0e7-45846eeb25e6
  modified: 2026-11-05T17:14:23.551Z
---

For the Big Boys Vault site (see [[marcuru-rs-big-boys-vault]]), every UI change must be verified on phone, tablet and desktop — never desktop only. Admin tools must be intuitive enough that "a 10-year-old understands it in under a minute": visual (real pictures over icons/jargon), one way to do each thing, drag-and-drop where ordering is involved, plain-word labels. Layouts must re-center odd item counts and scale to fit the viewport without cutting off any part of a section. Everything follows the brand (brass/steel/slate vault, slab display font).

**Why:** the user repeatedly flagged desktop-only fixes, empty/barren layouts, generic icons and duplicate controls as unacceptable.
**How to apply:** after any front-end change, run the multi-viewport QA (small phone 360×640, phone 390×844, tablet 820×1180, desktop 1440×900+) for overflow, tap targets ≥32px, tiny text, centering; look at the screenshots before reporting.

Checking "does it fit" means two things: nothing wider than the screen, and nothing poking out of its own card, on real data at 360px and 3x density. Screen-edge checks alone missed a card picture overflowing its frame. Always look at live screenshots as well as running measurements.


===== bbv-floating-and-consistency.md =====
---
name: bbv-floating-and-consistency
description: "BBV - everything opens in the floating window, never a new page; visual treatments apply to every card surface at once"
metadata:
  node_type: memory
  type: feedback
  originSessionId: 76f14029-25cc-495c-a0e7-45846eeb25e6
  modified: 2026-11-07T02:43:01.603Z
---

On Big Boys Vault, anything a visitor opens from a page (cards, trainers, binders, a binder's search and filters) happens inside the floating window, with Back returning to the same state. Never send them to a separate page.

A visual treatment applied to "cards" goes on every surface that shows a card, all at once: catalogue tiles, carousels, binder windows, the home showcase, highlighted trainer cards, trainer binders, and card window plates.

**Why:** the user had to ask three times in one session for this: "the search filter must happen in the floating window that should be a common sense", "cards showcased should also have the color-coding obviously", "set symbol must also be included in the highlighted trainer cards obviously".

**How to apply:** before finishing a card feature, grep every template that renders a card (`card.html`, `home.html` showcase and highlights, `trainer_body.html`, `member_card_part.html`, `product_body.html`, `crates/finder` Tile). Before adding any link in the floating window, ask whether it leaves the window. See [[bbv-design-standards]].


===== marcuru-rs-big-boys-vault.md =====
---
name: marcuru-rs-big-boys-vault
description: "marcuru-rs is the Big Boys Vault card shop site; deployed on Fly app \"bigboysvault\" (sin) with domain bigboysvault.com"
metadata:
  node_type: memory
  type: project
  originSessionId: 76f14029-25cc-495c-a0e7-45846eeb25e6
  modified: 2026-11-04T16:43:14.401Z
---

marcuru-rs (Rust/axum + askama, SQLite) is rebranded as **Big Boys Vault** (brass/steel/slate theme from the user's badge logo; heroes Slaking + Snorlax). Rebranded 2026-11-05.

Deployed 2026-11-05 to Fly.io app `bigboysvault` (region sin, 1 GB volume `data` at /data, ADMIN_TOKEN set as a Fly secret). Live at https://bigboysvault.fly.dev. Domain bigboysvault.com bought on Namecheap; Fly certs added for apex + www, DNS records (A 66.241.125.56, AAAA 2a09:8280:1::1e7:3322:0) entered by the user in Namecheap Advanced DNS.

**Why:** the user chose to upload the current catalogue (copied from marcurushop.com) despite the warning in DEPLOY.md.
**How to apply:** redeploy with `flyctl deploy --app bigboysvault --remote-only`; in Git Bash set `MSYS_NO_PATHCONV=1` before `fly ssh sftp put ... /data/...` or the remote path gets mangled. flyctl lives under %LOCALAPPDATA%\Microsoft\WinGet\Packages\Fly-io.flyctl_*\flyctl.exe (not on PATH).

Added 2026-11-06: a Cargo workspace with `crates/vault-search` (shared search code) and `crates/finder` (Leptos widget compiled to WebAssembly; the Docker build compiles it via `build-wasm.sh`), plus `static/modern.css`, `holo.js`, `webmcp.js` and `browser-ai.js`. hx-boost was removed on purpose so speculation rules and cross-document view transitions work; don't bring it back. WebMCP stays off for visitors until the user registers a Chrome origin-trial token and sets the Fly secret `ORIGIN_TRIALS` (steps in DEPLOY.md); that trial is due to end in early 2027.

Card page languages (2026-11-06): English/Japanese/Chinese use `src/translate.rs` (PokeAPI names plus hand tables), on the server, for all devices. The free MyMemory service was tried and rejected for bad results ("クレイバースト" became "Cancelled"). Full machine translation of free-form notes would need a paid key, which the user hasn't chosen. iOS Safari's WebGPU path could only be tested with an emulated tier in Chrome, because no real iPhone or iPad was available to Claude; ask the user to confirm on-device behaviour after 3D card changes.

Trainer accounts (2026-11-06): `src/members.rs`, `src/community.rs`, `/join`, `/me`, `/trainers`, `/admin/trainers`. Sign-in is name + password with no email, chosen by Claude as the default; only the owner can reset a password. Everything public is pre-moderated: nothing a trainer posts shows until the owner approves it, and edits to name, bio, picture or vouch go back for approval. Cards open in a floating window (`static/card-float.js`), closed by tapping outside, with no X button because the user asked for that. Backup of the live DB before this release is in the session scratchpad only.
