# Big Boys Vault: developer handoff

Live site: **https://www.bigboysvault.com**, also reachable at https://bigboysvault.fly.dev.
Check the live release with `fly releases -a bigboysvault`. This file was last brought up to date on 2026-11-14.

Read this first, then `docs/REDESIGN.md` (the look and where every page lives), `docs/SELLER_API.md` (stores, the
cart, orders, alerts) and `docs/OPERATIONS.md` (speed, backups, the VM's jobs).

## 1. What the site is

Big Boys Vault ("BBV") is a Pokémon card vault in the Philippines: the owner's cards and the stores of trainers the
owner has approved, shown and sold in pesos (₱). It is laid out the way a social app is, in the vault's dark and gold.

**Visitors:**
- read the **feed** at `/` (new cards, listings, vouches, price moves, trainers' posts), with the showcase's cards
  as the row at the top;
- browse **The Vault** (`/all`): every card for sale, the vault's and the stores', with search and filters;
- open a card in a **window over the page**; its own page (`/products/…`) is for shared links and search engines;
- visit **Stores** (`/stores`, `/store/{name}`, the vault's own at `/store/vault`) and their **binders**;
- meet **Trainers**, read vouches, use the vault chat and **Ask the Vault** (the shop's assistant);
- put cards in a **cart** and send an order. Sending needs an account. No money moves on the site: the buyer pays
  the vault or the seller directly (GCash, bank, meetup).

**Trainers** (accounts): a profile, a binder of cards (Highlight, For sale, Wishlist), vouches, posts, private
messages and calls, and, once the owner switches it on, a store with its own Store Manager.

**The owner and staff** (roles: admin, moderator): the Vault Manager under `/admin` (Overview, Cards, Add cards,
Add by set, Binders, Showcase, Orders, Trainers, Sellers). The Overview counts **visitors** beside orders.

## 2. Stack

| Part | Technology |
|---|---|
| Server | Rust 2021, **axum 0.8**, tokio |
| Templates | **askama 0.16** (compiled into the binary, so a template change needs a rebuild) |
| Database | **SQLite** through rusqlite 0.41 (bundled), one file: `/data/data.db` on the server, `data.db` locally |
| Cache | moka, in memory: kept pages and the catalogue (cleared on every write, see §5), rate limits, live chat and call state |
| Passwords | argon2 (trainers), plus a single owner `ADMIN_TOKEN` |
| Images | `image` crate (JPEG/PNG/WebP), uploads in `/data/uploads` |
| Card finder | `crates/finder`: Leptos 0.8 compiled to **WebAssembly** (wasm-bindgen **0.2.130**, pinned) |
| Shared search | `crates/vault-search`: catalogue format and ranking, used by the server and the finder |
| Front end | Hand-written CSS and plain JavaScript (no framework, no bundler); htmx for a few swaps |
| Hosting | **Fly.io**, app `bigboysvault`, region `sin`, 1 machine (shared-cpu-1x, 256 MB), volume `data` at `/data`; Cloudflare in front |
| Outside services | TCGdex and pokemontcg.io (cards, pictures, prices), Google Gemini (Ask the Vault), Cloudflare TURN (calls), Web Push |

The site runs as **one machine**: live chat, calls and "who is online" are kept in its memory, and the database is
one file on its volume. Don't add a second machine without moving those first.

## 3. File map (`marcuru-rs/`)

### Server (`src/`)
| File | What it does |
|---|---|
| `main.rs` | App state (`App`), every route, The Vault and binder pages, card pages, SEO, `robots.txt`, `sitemap.xml`, `with_db`, `cached`, the visitor counter's step (`count_visit`) |
| `feed.rs`, `social.rs` | The home feed: posts, reactions, comments, the automatic posts and their switches |
| `store.rs`, `sellers.rs` | Stores: the Store Manager, the public store pages, the cart and orders, alerts, mail (see `docs/SELLER_API.md`) |
| `community.rs`, `members.rs` | Trainers: join, log in, My trainer (`/me`), trainer pages, vouches, chat, moderation; accounts, sessions, roles |
| `messages.rs`, `dm.rs`, `push.rs` | Private messages and calls (long-poll), Web Push |
| `admin.rs` | The Vault Manager: access (`power()`, `authed()`), cards, Add cards, Add by set, binders, showcase, Overview |
| `db.rs` | Cards and binders, list queries, the catalogue files (`/cards.tsv`, `/vault.tsv`), migrations |
| `market.rs`, `pricing.rs`, `history.rs` | Card data and market prices, the suggested price by condition, price history |
| `visits.rs` | Visitors per day: who counts, the table, the totals the Overview shows |
| `ops.rs` | Speed and upkeep: picture formats, cache headers, `/healthz`, and the routes the VM's jobs call |
| `ask.rs`, `vault_ai.rs` | Ask the Vault: the built-in rules, and Gemini when its key is set |
| `blocks.rs` | Site settings (name, welcome line) and the showcase's cards |
| `look.rs` | The style guide page (`/admin/look`) with the feed's automatic-post switches |
| `stamp.rs`, `similar.rs`, `suggest.rs`, `translate.rs`, `insights.rs`, `wishes.rs`, `bulk.rs` | 1st Edition seal removal, "More like this", search suggestions, card languages, Overview graphs, wishlist alerts, bulk actions |
| `lib.rs` | Module list for the library half (`main.rs` is the binary) |

### Pages (`templates/`)
| Template | What it is |
|---|---|
| `base.html` | `<head>`, script and CSS includes **with version numbers**, the cart; the frame is `fb_top.html` + `fb_bottom.html` |
| `feed.html`, `feed_items.html` | Home |
| `collection.html`, `card.html`, `grid.html` | The Vault, a search, one of the vault's binders |
| `product.html`, `product_body.html`, `product_part.html` | A card: its page, and the part the card window loads |
| `store*.html`, `stores*.html`, `binder_tile.html` | Stores, a store, the vault's store, the Store Manager's pages (`store_tabs.html` is its frame) |
| `me.html` | My trainer, inside the Manager's frame |
| `admin_*.html` | The Vault Manager (`admin_tabs.html` is its frame; `mgr_head.html` loads `mgr.css`) |
| Others | `trainers.html`, `trainer_body.html`, `login.html`, `join.html`, `ask*.html`, `chat*.html`, `vouch*.html`, `messages.html`, `look_guide.html` |

### Browser code (`static/`)
| File | What it does |
|---|---|
| `fb.css` | **The site's stylesheet**: colours and sizes first, then every piece. `mgr.css` is the Manager's; `widgets.css` the messages, camera and staff buttons |
| `fb-shell.js` | The frame: who is signed in, tabs, menu, the **card window**, sliders |
| `feed.js`, `dm.js`, `alerts.js`, `cart.js`, `vouches.js`, `vault-talk.js` | Feed, messages and calls, alerts, cart, vouches, chat and Ask |
| `storefront.js`, `store.js`, `binder-tools.js`, `edit-listing.js` | A store's page, the Store Manager, arranging binders, editing a listing in place |
| `quick.js`, `my-cards.js`, `admin-float.js`, `admin-ui.js`, `mgr-table.js`, `showcase.js`, `binders.js`, `edit-card.js` | The Vault Manager |
| `me.js`, `pricing.js`, `dragsort.js`, `card-cam.js`, `holo.js`, `suggest.js`, `tiles.js` | My trainer, the suggested-price line, drag and drop, the card camera, a card's shine, search suggestions, card tiles |
| `sw.js`, `push-sw.js` | The service worker (alerts with the site closed) |
| `wasm/` | The built card finder (made by `build-wasm.sh`; don't edit) |

## 4. How it fits together

- **Catalogue:** `/cards.tsv` (the vault's cards) and `/vault.tsv` (those plus every store card that can be bought
  now) are whole lists in one compact text file, built by `db.rs` and kept in memory. The finder downloads the
  list and searches in the browser.
- **The card window:** a link to `/products/…` or `/trainers/{id}/cards/{id}` opens over the page
  (`fb-shell.js`, loading `?part=1`). The address changes, so links can be shared.
- **Binders belong to stores.** The vault's binders are its collections (`/collections/{handle}`); a seller's are at
  `/store/{name}/binders/{binder}`. Both open as a page under a bar with an X back to the store.
- **Admin access** (`admin.rs` `power()`): either the owner's `ADMIN_TOKEN` cookie, or a signed-in trainer whose
  role is admin (everything) or moderator (trainers and vouches only), checked on every request. Demoting or
  blocking a trainer ends *their session's* powers at once. It does **not** end access for anyone who knows
  `ADMIN_TOKEN` or still holds an owner cookie (30 days). **Rotating `ADMIN_TOKEN` is what ends that:**
  `fly secrets set ADMIN_TOKEN=$(openssl rand -hex 24) -a bigboysvault`, then put the same value in `ops/env` on
  the VM (its jobs send it). An empty `ADMIN_TOKEN` leaves the admin open to everyone: never run the live site
  without it.
- **Visitors** (`visits.rs`): one row per visitor per day, counted after a whole page opens. A visitor is a hash
  of address and browser. Search engines, scripts, the owner, staff and pages fetched ahead of time aren't counted.
- **Lite mode:** `<html data-lite>` is set in `base.html` for phones and low-power devices; `holo.js` leaves a
  card's shine off there. Add `?lite=1` to test it on a computer.
- **Vintage printings:** in Add by set and Add cards, cards from Base Set to Neo 4 come split into 1st Edition,
  Unlimited, Shadowless and the rarer prints. Unlimited pictures are the 1st Edition scan with the seal painted
  out (`stamp.rs`, kept in `/data/uploads/unl/`).

## 5. Conventions (please keep them)

- **Version every asset:** `<script src="/static/x.js?v=N">`. Bump N whenever that file changes, or browsers and
  Cloudflare keep the old one. After a change in `crates/finder` or `crates/vault-search`, run `build-wasm.sh` and
  bump only `<meta name="finder-v">` in `templates/base.html`.
- **Database migrations only add:** `CREATE TABLE IF NOT EXISTS`, or `ALTER TABLE … ADD COLUMN` behind a check.
  Never drop or rename on the live database.
- **Back up the live database before any schema change:** on the VM run `bash ~/bigboysvault/ops/backup.sh`
  (`ops/deploy.sh` does it for you), or from any machine:
  `curl -fsS -H "Cookie: marcuru_admin=$ADMIN_TOKEN" -o backup-YYYYMMDD.db https://bigboysvault.fly.dev/admin/backup/db`
  then `python3 -c "import sqlite3,sys;print(sqlite3.connect(sys.argv[1]).execute('pragma integrity_check').fetchone()[0])" backup-YYYYMMDD.db`,
  which must print `ok`. Never copy `/data/data.db` by itself while the app runs: the newest writes can still be in
  `data.db-wal`. To put a copy back, see `docs/OPERATIONS.md` › Restore.
- **Every write clears the kept pages:** call `app.forget()` (not `cache.invalidate_all()`), so a slow request
  can't put an old page back.
- **Words for people:** short, plain English a 10-year-old understands. No jargon on the site or in comments.
- **The owner's rules for anything people see:**
  - It works on every device. Check a small phone (360×640), a phone (390×844), a tablet (820×1180) and a
    computer (1440×900): nothing wider than the screen, nothing poking out of its own box.
  - One obvious way to do each thing; real pictures over icons; drag and drop wherever there is an order.
  - Editing happens where the thing is shown.
  - Nothing plays or opens by itself. A row that slides sideways has arrow buttons on a computer.
  - Whatever is done to "cards" is done on every surface that shows a card, all at once (condition colours, set
    symbols, prices).
  - The look is in `docs/REDESIGN.md` §1: flat, dark and gold, the system font for text and "Anybody" for the
    name, prices and titles. There is one look; the older one was removed on 2026-11-14.

## 6. Run, test, deploy

### Run locally (Windows)
```
cd marcuru-rs
cargo build --release
$env:ADMIN_TOKEN = "any-long-local-password"
target\release\marcuru-rs.exe           (serves http://localhost:3000, database ./data.db, uploads ./uploads)
```
Environment variables: `DATABASE`, `UPLOADS`, `PORT` (3000 locally), `HOST`, `ADMIN_TOKEN`, `PUBLIC_URL`.
For real data, point `DATABASE` at a copy of a backup (never at the only copy).

### Tests
- **Rust:** `cargo test` (116 in the library, 43 in the binary) and `cargo test -p vault-search` (21).
- **The phone, tablet and computer check:** `tests/browser/smoke.mjs` opens the site in Chrome at phone, tablet and
  computer size and walks a buyer's path: home, The Vault, a search, a card in its window and on its page, the cart, "Send
  order", then the other main pages. On each it checks that the page opened, nothing sticks out sideways, no
  script broke and nothing failed to load.
  ```
  cd tests/browser && npm install          (once)
  node tests/browser/smoke.mjs http://localhost:3000 --order     before a release, on a local copy
  node tests/browser/smoke.mjs https://www.bigboysvault.com      after it, on the live site (writes nothing)
  ```
  It must end with `READY`. Chrome is found by itself, or set `CHROME` to its path.

### Deploy
```
cargo test                                                  must pass
node tests/browser/smoke.mjs http://localhost:3000 --order  must say READY
(back up the live database, §5)
fly deploy                                                  from a clean checkout of master
fly releases -a bigboysvault                                the new release number
node tests/browser/smoke.mjs https://www.bigboysvault.com   must say READY
```
`ops/deploy.sh` does the same from the VM (tests where it can, backup, deploy, `/healthz`, then the check when Node
and Chrome are there). The build takes 3 to 5 minutes. People in a call or a chat are cut off for a moment when
the machine swaps, so deploy at a quiet time.

### The VM's nightly jobs
A small Google VM runs the backups, prices, picture formats and an uptime check from `ops/` (see
`docs/OPERATIONS.md` §4). Each job reports to the site, so the Overview shows how last night went. The VM pulls
`ops/` from GitHub (`master`) every night at 02:15 (`ops/update.sh`).

## 7. Things to know (gotchas)

- **Git:** the code is on GitHub (`koola02/bigboysvault`, branch `master`). Work on a branch, merge, push.
- **Templates are compiled in:** after editing anything in `templates/`, rebuild.
- **WAL files:** the newest writes sit in `data.db-wal`. Back up with `/admin/backup/db` or `ops/backup.sh`, and
  restore through `/data/restore.db` (`docs/OPERATIONS.md` › Restore).
- **Mail is off** until `RESEND_API_KEY` and `MAIL_FROM` are set as Fly secrets: until then messages meant for
  email are dropped (`store.rs`). There is no "forgot my password" page: an admin gives a locked-out trainer a
  new password from Trainers.
- **Rate limits:** log in, join, search, Ask and orders use `app.limits` (per address or account).
- **Old addresses:** `/admin/page` (the old home page editor) goes to `/admin/showcase`, where the site's name,
  the welcome line and the showcase's cards are set.

## 8. What's been built (newest first)

| When | What changed |
|---|---|
| 14 Nov 2026 | Visitors counted on the Overview; My trainer in the Manager's frame; the old look removed (one stylesheet, one frame); the phone, tablet and computer check (`tests/browser/smoke.mjs`) |
| 10–14 Nov 2026 | The Facebook-style redesign (`docs/REDESIGN.md`): feed, The Vault, stores and their binders, the Managers, private messages and calls, alerts with the site closed, suggested prices by condition |
| to 10 Nov 2026 | Card finder, binders, vintage printings, Japanese sets in English, lite mode, SEO, trainers, vouches, chat, Ask the Vault |
