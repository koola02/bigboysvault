# Big Boys Vault: developer handoff

Live site: **https://www.bigboysvault.com**, also reachable at https://bigboysvault.fly.dev.
The live release is **v61** (2026-11-10).

## 1. What the site is

Big Boys Vault ("BBV") is the owner's Pokémon card collection, shown and sold from the Philippines. Prices are in pesos (₱).

**Visitors:**
- browse binders (collections), search every card, and open any card in a floating window;
- read trainers' (community members') pages and vouches;
- use a live chat, and ask "Ask the Vault", a built-in shop assistant.

**The owner and admins:**
- add cards by catalogue search ("Quick add") or a whole set at a time ("Add by set"), in English, Japanese and Chinese;
- edit cards, binders and the home page (with drag and drop);
- moderate trainers.

**Trainers** have their own accounts, profiles, card lists and vouch ratings. Roles are admin, moderator, or none.

## 2. Stack

| Part | Technology |
|---|---|
| Server | Rust 2021, **axum 0.8**, tokio |
| Templates | **askama 0.16** (Jinja-like; compiled into the binary, so a template change needs a rebuild) |
| Database | **SQLite** through rusqlite 0.41 (bundled), one file: `/data/data.db` on the server, `data.db` locally |
| Cache | moka (in-memory pages and catalogue; 5-minute TTL, cleared on every write; see §7) |
| Passwords | argon2 (trainers), plus a single owner `ADMIN_TOKEN` |
| Images | `image` crate (JPEG/PNG/WebP), uploads stored in `/data/uploads` |
| Card search widget | `crates/finder`: Leptos 0.8 compiled to **WebAssembly** (wasm-bindgen **0.2.130**, pinned) |
| Shared search | `crates/vault-search`: card catalogue format, BM25-style ranking, used by server and widget |
| Front end | Hand-written CSS and plain JavaScript (no framework, no bundler). htmx for a few admin swaps |
| Hosting | **Fly.io**, app `bigboysvault`, region `sin` (Singapore), 1 machine (shared-cpu-1x, 256 MB), 1 GB volume `data` mounted at `/data` |
| Domain | bigboysvault.com (Namecheap DNS pointing at Fly) |
| Card data | TCGdex API (sets, cards, pictures, printings), pokemontcg.io pictures, TCGplayer product pictures |

The Docker image builds the server and the WebAssembly widget itself (see `Dockerfile`, `build-wasm.sh`).

## 3. File map (`marcuru-rs/`)

### Server (`src/`)
| File | What it does |
|---|---|
| `main.rs` | App state (`App`), every route, home, browse and binder pages, card pages, SEO (titles, JSON-LD, canonical), `robots.txt`, `sitemap.xml`, caching helpers (`cached`, `App::forget`, `App::keep`) |
| `admin.rs` | Admin panel: access (`power()`, `authed()`), card save, My cards, binders, home page editor blocks, dashboard, Quick add, Add by set, Unlimited pictures (`unlimited_bytes`), "Have" counts (`mark_owned`) |
| `community.rs` | Trainers: join, sign-in, `/me` dashboard, trainer pages, vouches, chat, moderation actions |
| `members.rs` | Trainers' database tables and migrations, argon2 passwords, sessions, tags, highlights |
| `db.rs` | Products (cards), collections (binders), card list queries, catalogue file (`/cards.tsv`), "More like this", price standing, sitemap rows |
| `market.rs` | TCGdex and pokemontcg.io lookups, set search (with Japanese set English names, `JA_EN`), set cards, **printings** of vintage cards (`EDITION_SETS`, `printings_of`, `split_printings`), market prices |
| `stamp.rs` | Paints the "1st Edition" seal out of vintage scans to make Unlimited pictures (seal detection by its disc, stroke-mask inpainting) |
| `similar.rs` | "More like this" recommender (TF-IDF cosine plus Maximal Marginal Relevance) |
| `ask.rs` | Ask the Vault answers (intent rules, budget knapsack, follow-ups, Taglish) |
| `api.rs` | `/cards.tsv`, `/api/search`, `/api/ask`, card translations |
| `blocks.rs` | Home page sections (hero, binders, rows, showcase, highlights, vouches...) |
| `insights.rs`, `suggest.rs`, `translate.rs`, `bulk.rs` | Price charts, search suggestions, Japanese/Chinese ↔ English, bulk admin actions |
| `lib.rs` | Module list for the library half (main.rs is the binary) |

### Pages (`templates/`)
| Template | What it is |
|---|---|
| `base.html` | `<head>` (SEO, **lite-mode switch** script), top bar, footer, script and CSS includes **with version numbers** |
| `home.html` | Home page: every block type, including the **3D binder ring** (`.bx`, `.bk`), editor mode (`edit`) |
| `binder_part.html` | The **binder window**: search on top, left page (card details), rings, right page (3×3 pockets) |
| `product_body.html` | Card details (also loaded into the binder's left page) |
| `admin_*.html` | Admin pages. `admin_quick.html` is Quick add and Add by set |
| Others | `trainers.html`, `trainer_body.html`, `me.html`, `join.html`, `ask*.html`, `vouch*.html`, `member_card*.html` |

### Browser code (`static/`)
| File | What it does |
|---|---|
| `vault.css` | **Main stylesheet.** Later rules override earlier ones; newer sections are appended at the end. Also `app.css` (base) and `modern.css` |
| `card-float.js` | The **floating window** for cards, binders and trainers. Binder book: pages of 9, pager, page picker, left-page details, swipe |
| `vault-binders.js` | Home page 3D binder ring (turning, tap to open and enter, ✕ close) |
| `rail-arrows.js` | ‹ › buttons for sideways rows |
| `vault-core.js` | Text-fit (no "…"), pointer glow, top menu wheel scrolling |
| `vault-fx.js`, `vault-gl.js`, `holo.js` | WebGL hero (logo and orbiting cards), live sky and foil, WebGPU holo effect. Lighter or off in lite mode |
| `quick.js` | Quick add and Add by set (tray, finishes, prices, printings filter buttons) |
| `home-edit.js`, `dragsort.js` | Home page editor (drag and drop for cards, binders, trainers' picks) |
| `admin-float.js`, `my-cards.js`, `admin-ui.js` | Admin My cards (edit window, list) |
| `vault-talk.js` | Chat and Ask the Vault in the browser |
| `wasm/` | Built search widget (generated by `build-wasm.sh`; don't edit) |

## 4. How it fits together

- **Catalogue:**
  - `GET /cards.tsv` is the whole card list in one compact text file, with binder lines (`B\t…`) and card lines (`C\t…`).
  - It's built by `db::catalogue` and cached.
  - The search widget downloads it and searches in the browser.
- **Copies:** identical cards (same name, condition, finish and price) are grouped into one tile with "×N".
- **Binders:**
  - `/collections/{handle}`. In the floating window, the server sends `binder_part.html` with the first 48 cards.
  - The search widget then takes over and loads everything. `card-float.js` shows 9 per page.
- **Floating window:** any link to `/products/…`, `/collections/…` or `/trainers/…` opens in it (`card-float.js`). The address changes, so links can be shared, and opening the address directly shows a normal page.
- **Admin access** (`admin.rs` `power()`): either the owner's `ADMIN_TOKEN` cookie, or a signed-in trainer whose role is admin (full access) or moderator (trainers and vouches only).
- **Lite mode:**
  - `<html data-lite>` is set in `base.html` for phones, low-power devices and data saver.
  - CSS turns off blur and endless animations, and JS skips or lightens the WebGL.
  - Add `?lite=1` to any address to test it on a desktop.
- **Vintage printings:**
  - In Add by set and Quick add, cards from Base Set, Jungle, Fossil, Team Rocket, Gym Heroes, Gym Challenge and Neo 1–4 come split into 1st Edition, Unlimited, Shadowless and the rarer prints.
  - Unlimited pictures come from `/admin/cardimg/unlimited?src=…`: the 1st Edition scan with the seal painted out, cached in `/data/uploads/unl/`.
  - A saved card's name carries the printing, e.g. "Charizard (1st Edition) - 004/102 - Base Set".

## 5. Conventions (please keep them)

- **Version every asset:** `<script src="/static/x.js?v=N">`. Bump N whenever that file changes, or browsers keep the old one. The `?v=` lives in the template (mostly `base.html`, `home.html`, `admin_*.html`).
- **Database migrations only add:** `CREATE TABLE IF NOT EXISTS`, or `ALTER TABLE … ADD COLUMN` guarded by a check (see `members.rs`). Never drop or rename on the live database.
- **Back up the live database before any schema change:**
  `fly ssh sftp get -a bigboysvault /data/data.db ./backup-YYYYMMDD.db`
- **Every write clears the cache:** call `app.forget()` (not `cache.invalidate_all()`), so the change counter stops stale pages being stored (the bug fixed in v61).
- **Comments and words for people:** short, plain English, written for a 10-year-old. No jargon in the interface.
- **Design rules:** see `05-owner-preferences.md`. In short:
  - works on every device;
  - everything opens in the floating window;
  - one font (Anybody);
  - dark and gold "vault 500 years ahead" theme, emanating from the logo;
  - binders are the star of the home page.

## 6. Run, test, deploy

### Run locally (Windows)
```
cd marcuru-rs
cargo build --release
set ADMIN_TOKEN=local-test-token        (PowerShell: $env:ADMIN_TOKEN = "local-test-token")
target\release\marcuru-rs.exe           (serves http://localhost:3000, database ./data.db, uploads ./uploads)
```
- Environment variables: `DATABASE`, `UPLOADS`, `PORT` (default 3000 locally), `HOST`, `ADMIN_TOKEN`, `PUBLIC_URL` (defaults to the live domain).
- The search widget's WebAssembly is already built in `static/wasm/`. Rebuild it only if `crates/finder` changes; this needs wasm-bindgen 0.2.130 and binaryen; see `build-wasm.sh`.

### Tests
- **Rust tests:** run `cargo test --lib`, which should report 36 passing, and `cargo test -p vault-search` (14 passing).
- **Browser tests** (`tools-and-tests.zip` → `tests/`):
  - They use puppeteer-core and Chrome for Testing: run `npm i puppeteer-core` in that folder.
  - Each takes the Chrome path, an output folder and a base address, e.g. `node devices.mjs "C:\path\chrome.exe" out http://localhost:3000 run1`.

| Suite | What it covers |
|---|---|
| `devices.mjs` | 10 screen sizes, desktop to phone, landscape included: home, binder ring, binder window fit, page picker, details, search |
| `clear.mjs` | Search and filter regressions in All cards and binders |
| `editions.mjs` | Add by set printings, saving, "Have" counts (admin) |
| `stuck2.mjs` | Admin My cards: save then keep working (the stuck-screen bug) |
| `bf.mjs`, `left.mjs`, `loupe.mjs` | Binder window, left page, magnifier |
| `askapi.mjs` | Ask the Vault conversation through the API |
| `perf.mjs`, `perf2.mjs` | Scroll smoothness on a slowed-down phone |
| `seo_check.py` | SEO signals and sitemap |

- **Local helpers** (`local/`):
  - `up.ps1 fresh` restores a test database, seeds test data and starts the server; `up.ps1 build` rebuilds and restarts without touching the data.
  - `reset-local.ps1 stop` stops the server and restores the local database.
  - The paths inside them point at the previous assistant's scratch folder; adjust them.

### Deploy
```
cd marcuru-rs
fly deploy          (needs flyctl and either `fly auth login` or FLY_API_TOKEN from 02-CREDENTIALS-PRIVATE.md)
fly releases -a bigboysvault        (check the new release number)
```
The build takes about 3 to 5 minutes, and Fly swaps the machine with no downtime.

## 7. Things to know (gotchas)

- **Not a git repository.** Run `git init` first.
- **Templates are compiled in:** after editing anything in `templates/`, rebuild.
- **Edit the right CSS rule:** `vault.css` is long and layered; a rule near the end usually wins. Search for the selector first.
- **WAL files:** the live database has no WAL file, and the server has no sqlite3. Back up with `fly ssh sftp get`.
- **Seal-erasing:** `stamp.rs` was checked on 21 vintage scans and all came out clean. Cards whose seal can't be found keep their original picture.
- **Rate limits:** sign-in, join, search and Ask use `app.limits` (per IP).
- **Lite mode:** CSS and JS check `html[data-lite]`. Test phones with that on.

## 8. What's been built (recent history, newest first)

| Release | What changed |
|---|---|
| v61 | Binder card-count bug fixed (stale cache race) and the binder search counter removed |
| v60 | Trainers' picks drag and drop in the home page editor |
| v59 | Every printing of vintage cards in Add by set and Quick add, Unlimited pictures, printing filter buttons, per-printing "Have" counts |
| v58 | Japanese set English search (`JA_EN` table); magnifier stays inside the picture; text shrinks instead of "…" |
| v57 | Bigger binder window; translation only on Japanese and Chinese cards (opt-in); admin "stuck after save" fix; price under the card name |
| v56 | Lite mode for phones; binder window "More like this"; staff and Vouch buttons hidden while browsing; top menu scrollbar; tighter spacing; footer logo removed |
| v51 to v55 | Binder window as an open binder (9 pockets, pager, page picker, left-page details); 3D binders on the home page; stats moved to the admin dashboard; home sections narrower |
| earlier | SEO (JSON-LD, sitemap, canonical), BM25 search, "More like this", trainer tags, binder ring, drag and drop editing, chat, vouches, trainers |

**Not deployed (in the source, tests passing, not checked in a browser):** Ask the Vault improvements in `src/ask.rs`, `src/api.rs` and `static/vault-talk.js`:
- follow-ups (`ctx` sent back and forth);
- Taglish (magkano, meron ba, pinakamura, pinakamahal…);
- 1st Edition, Japanese and set-count questions.

Review and deploy these, or set them aside; see `03-PENDING-WORK.md` item 1.
