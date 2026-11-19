# Big Boys Vault: developer handoff

Live site: **https://www.bigboysvault.com**, also reachable at https://bigboysvault.fly.dev.
Check the live release with `fly releases -a bigboysvault`. This file was last brought up to date on 2026-11-14.

Read this first, then `docs/REDESIGN.md` (the look and where every page lives), `docs/SELLER_API.md` (stores, the
cart, orders, alerts) and `docs/OPERATIONS.md` (speed, backups, the VM's jobs).

## 1. What the site is

Big Boys Vault ("BBV") is a Pokémon card vault in the Philippines: the owner's cards and the stores of trainers the
owner has approved, shown and sold in pesos (₱). It is laid out the way a social app is, in the vault's dark and gold.

**Visitors:**
- read the **feed** at `/` (new cards, listings, vouches, trainers' posts; no prices: those are where cards are
  shopped), with the showcase's cards
  as the row at the top;
- search **every card for sale** (`/all`, the page once called The Vault; since 17 Nov 2026 it has no tab and is
  reached through the search and Stores), the vault's and the stores', with filters;
- open a card in a **window over the page**; its own page (`/products/…`) is for shared links and search engines;
- visit **Stores** (`/stores`, `/store/{name}`, the vault's own at `/store/vault`) and their **binders**;
- meet **Trainers**, read vouches, use the vault chat and **Ask the Vault** (the shop's assistant);
- put cards in a **cart** and send an order. Sending needs an account. No money moves on the site: the buyer pays
  the vault or the seller directly (GCash, bank, meetup).

**Trainers** (accounts): a profile, a binder of cards (Highlight, For sale, Wishlist), vouches, posts, private
messages and calls, and, once the owner switches it on, a store with its own Store Manager.

**The owner and staff** (roles: admin, moderator): the Vault Manager under `/admin` (Overview, Cards, Add cards,
Binders, Showcase, Orders, Trainers, Sellers; Add cards has Search, Scan a card, By set and Not in the list). The Overview
counts **visitors** beside orders.

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
| `feed.rs`, `social.rs` | The home feed: posts, reactions, comments (with a like and one photo each), the automatic posts and their switches; who hears about a like or a comment (`post_owners`: the vault's posts tell its admins) and followers told when a store puts up cards (`tell_followers`) |
| `store.rs`, `sellers.rs` | Stores: the Store Manager, the public store pages, the cart and orders, alerts, mail (see `docs/SELLER_API.md`) |
| `community.rs`, `members.rs` | Trainers: join, log in, Account Settings (`/me`), your own page (`/me/page` forwards to `/trainers/{id}`), trainer pages, vouches, chat, moderation; accounts, sessions, roles |
| `messages.rs`, `dm.rs`, `push.rs` | Private messages and calls (long-poll), Web Push. The same live connection carries the bell's number (`notes`): `sellers::notify_from` wakes whoever it told, and the 3-second push loop in `main.rs` passes them on |
| `admin.rs` | The Vault Manager: access (`power()`, `authed()`), cards, Add cards (`/admin/quick?by=set\|scan&binder=`; `/admin/sets` forwards there), binders, showcase, Overview |
| `db.rs` | Cards and binders, list queries, the catalogue files (`/cards.tsv`, `/vault.tsv`), migrations |
| `market.rs`, `pricing.rs`, `history.rs` | Card data and market prices, the suggested price by condition, price history |
| `visits.rs` | Visitors per day: who counts, the table, the totals the Overview shows |
| `ops.rs` | Speed and upkeep: picture formats, cache headers, `/healthz`, and the routes the VM's jobs call |
| `ask.rs`, `vault_ai.rs` | Ask the Vault: the built-in rules, and Gemini when its key is set |
| `scan.rs` | Scan a card (one of the ways on Add cards: a seller's `/me/add#scan`, the vault's `/admin/quick?by=scan`; the old `/me/scan` forwards): Claude reads a card photo; the card, its versions, other printings and evolution family from the card list (see `docs/OPERATIONS.md` 4e) |
| `blocks.rs` | Site settings (name, welcome line) and the showcase's cards |
| `binder_sort.rs` | A binder's one-tap Sort (A to Z, Z to A, by set, by number, by evolution) and the TCGdex details it keeps in `card_dex` |
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
| `me.html` | Account Settings (picture, name, cards, password), inside the Manager's frame |
| `store_add.html` | Add cards (`/me/add`): the one place a seller or a collector adds a card; four ways at the top (Search, Scan a card for sellers, By set, Not in the list); `?binder=id` adds into one of a seller's binders, its own cards first |
| `admin_*.html` | The Vault Manager (`admin_tabs.html` is its frame; `mgr_head.html` loads `mgr.css`) |
| Others | `trainers.html`, `trainer_body.html`, `login.html`, `join.html`, `ask*.html`, `chat*.html`, `vouch*.html`, `messages.html`, `look_guide.html` |

### Browser code (`static/`)
| File | What it does |
|---|---|
| `fb.css` | **The site's stylesheet**: colours and sizes first, then every piece. `mgr.css` is the Manager's; `widgets.css` the messages, camera and staff buttons |
| `fb-shell.js` | The frame: who is signed in, tabs, menu, the **card window**, sliders |
| `feed.js`, `dm.js`, `alerts.js`, `cart.js`, `vouches.js`, `vault-talk.js` | Feed, messages and calls, alerts, cart, vouches, chat and Ask |
| `storefront.js`, `store.js`, `binder-tools.js`, `edit-listing.js` | A store's page, the Store Manager, arranging binders, editing a listing in place |
| `store-head.js`, `bookmarks.js`, `apply.js` | Every store page's top (Follow, the Vouches window, the border colour picker), Bookmark buttons and the Bookmarks page, Apply as a seller |
| `quick.js`, `my-cards.js`, `admin-float.js`, `admin-ui.js`, `mgr-table.js`, `showcase.js`, `edit-card.js` | The Vault Manager |
| `share.js` | Share, everywhere: a binder, a store, a card or a post to a message here, Messenger, Facebook, or its link |
| `me.js`, `pricing.js`, `dragsort.js`, `card-cam.js`, `holo.js`, `suggest.js`, `tiles.js` | Account Settings, the suggested-price line, drag and drop, the card camera (`data-card-cam="scan"`: Scan a card's sharper photo, sent at the shutter), a card's shine, search suggestions, card tiles |
| `add-page.js`, `add-card.js`, `scan.js` | The Add cards page (its four ways, "Added just now"); the "Add to your store" window every seller's new card goes through (from Scan a card it also offers the version: Normal, Holo, Reverse Holo...; it announces each card with a `bbv:added` event); and Scan a card |
| `sw.js`, `push-sw.js` | The service worker (alerts with the site closed) |
| `wasm/` | The built card finder (made by `build-wasm.sh`; don't edit) |

## 4. How it fits together

- **Catalogue:** `/cards.tsv` (the vault's cards) and `/vault.tsv` (those plus every store card that can be bought
  now) are whole lists in one compact text file, built by `db.rs` and kept in memory. The finder downloads the
  list and searches in the browser. `/vault.tsv` also gives each card's date on a `T` line after its card line (a
  finder from before dates skips those lines), so The Vault shows everyone's cards newest first; `/cards.tsv` has no
  dates and keeps the owner's order.
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
- **Vintage printings:** in Add cards (Search and By set, the vault's and the sellers'), cards from Base Set to Neo 4 come split into 1st Edition,
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
  - Easy to read (the owner, 18 Nov 2026): no text under 12px; the grey (`--muted`) is for labels and hints, and
    what a person wrote (a description, a note, a post) is in `--text`; details go one per line, a label then its
    value (`.sf-rows`), never strung together with dots. `tests/browser/smoke.mjs` measures every word it sees
    (12px or more, 4.5 to 1 against what is behind it, 3 to 1 for big words) and fails the release otherwise.
  - A tap opens the thing itself, not its settings: your name in the menu opens your page, where "Account
    Settings" is a button. Prices show only where cards are shopped: stores, their binders, the Stores page, the
    search and a card's window (not the feed, trainers' pages or the search suggestions).

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
- **Rust:** `cargo test` (116 in the library, 43 in the binary) and `cargo test -p vault-search` (24).
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
- **Confirm boxes:** ask with `await window.bbvConfirm(question, { yes, no })` (a Promise of true/false), not
  `confirm()`. Only a check that must answer at once (leaving a page with unsaved changes) keeps `confirm()`. The
  smoke check answers the box when it cancels its test order.
- **Why you were sent to log in:** set `sessionStorage['bbv:login-why']` to `order`, `message`, `react`, `post`, `bookmark`, `follow`, `vouch` or
  `offer` before sending someone to `/login`; the page says why, and Register (`/join`) carries it on.
- **Old addresses:** `/admin/page` (the old home page editor) goes to `/admin/showcase`, where the site's name,
  the welcome line and the showcase's cards are set.

## 8. What's been built (newest first)

| When | What changed |
|---|---|
| 19 Nov 2026 (open, release 8b) | The owner: "When creating a post, the space for typing is too small and the font is too big, fix that, take inspo from how Facebook does it. Also give the option to bold, change font size in 3 levels of any text within a post." **The post box** (feed.js `compose`, fb.css 15f): who is posting, a tall box to type in, one row of styles, Add To Your Post (Photos, Camera), one wide Post button. The typing space was 120px tall in 18px letters; it is 220px on a computer or tablet and the whole screen on a phone (433px on a 360×640 one), in 16px (15px is how a post reads; 16 stops a phone zooming in). **On a phone** it is laid out as Facebook's: Close, the title and Post along the top, and the styles and the photo buttons in one row at the bottom that stays just above the keyboard (`sheet.fit` follows `visualViewport`). **Styles in a post:** Bold, Italic, Underline and three sizes (Small, Normal, Large) on any part of the words, also by Ctrl+B/I/U; the buttons light up for what the cursor is in. Pasted or dropped words come in plain. **How it is kept** (`src/rich.rs`, new): the words stay plain in `posts.text` (or `note`) for everything that reads them, and when any are styled the same words go into `posts.rich` as pieces with their styles (JSON runs `{t, b, i, u, s}`). `rich::clean` tidies the pieces exactly as `members::clean_text` tidies plain words (a test holds the two together); `rich::html_for` draws them with `<b>`, `<i>`, `<u>` and `span.rt-s` / `span.rt-l`, every piece escaped, and only when they spell the words shown. Nothing a person typed is ever stored or sent as HTML. `POST /posts` and `POST /posts/{id}/edit` take a `rich` field beside `text`; editing to plain words clears the styles (`social::post_edit`, `post_note`). The words a seller or an admin adds to an automatic post can be styled the same way. Comments, vouches and messages stay plain. |
| 19 Nov 2026 (open, release 8a) | The owner: "Approval for Store changes or customizations etc. should not be required. The only approval needed is when a trainer wants to be a seller. Anyone can be a trainer no approval for that is needed when registering." **Nothing waits for approval any more**, except the application to sell. A new trainer's page is live the moment they register (`members::create` sets `profile_ok` 1) and they can post, add photos and be found straight away; changing a page (`members::save_page`) or a store (`sellers::save_store`, its links included) no longer takes it off the site. Pages that were waiting went live once (`open_pages_v` in settings). **`profile_ok` now means one thing: 0 is a page the vault's team hid** (Trainers › Hide Page, Show Page; the action names are still `hide` and `approve`). A hidden page shows visitors the name and vouches only, its store is closed, and its trainer cannot post or add photos; they are told when it is hidden and when it shows again (`community::show_page`), and their own pages say so. **Admins hear of a new trainer for knowing, not for doing:** one bell line while unread, "Gary joined. 5 new trainers in the last day.", opening the newest one's page (`tell_staff_joined`, ref `staff:joined`). The Trainers tab has no waiting number; the Overview shows Joined This Week in its place. **What stops a bot** now that nobody is looked at first: the limit for each address, plus `members::MAX_JOINS_PER_DAY` (200 new trainers in 24 hours, then sign-ups pause). `/me/who` says `page_hidden` in place of `approved`; `/me/profile` no longer answers `approved`. |
| 19 Nov 2026 (social, release 7e) | The owner: "check any other problems like this across the site and revise and perfect as needed". A sweep of every page on a phone for the same kinds of fault. **Finger-sized:** on touch screens (`@media (pointer: coarse)`, fb.css 15e and mgr.css) the things a finger aims at that were under 32px are bigger: who reacted, a row's See all, a trainer's followers, the finder's tips and Show more cards, Listings' kind buttons, stock steppers and a tile's own links, the star in Account Settings and its Password fold. A mouse keeps the tighter rows. **Your own vouch can be edited** (and a reply under one): Edit beside Delete, a box with Save and Cancel (`POST /vouches/{id}/act { action: "edit", text }`, `members::edit_vouch`; only its writer). **Taken down is never silent:** a post or a vouch the vault's team takes down or hides tells its writer (`feed::take_post`, `community::tell_vouch_writer`), as a comment already did. **An admin's own changes stay approved:** an admin who changed their own page or their store's links was un-approved like anyone, and their store dropped off the site until they approved themselves; now it stays live (they are who would approve it). Everyone else's changes still wait for approval. |
| 19 Nov 2026 (social, release 7d) | The owner: "an option for everyone to improve further the design of their stores to make their stores standout according to their own tastes and effort". **Store design** in Store settings, and the same in Vault Manager › Store settings (`templates/store_look_fields.html`, each choice a small picture of itself): **Background** (Plain, Glow, Dots, Lines, all drawn in the store's colour, or Your photo, shown blurred under a dark veil), **Store name** (Plain, Glow, Underline), **Card size** (Standard, Large: one to a row on a phone, Small) and a one-line **Welcome note** at the top of the store. Kept as JSON (`sellers::StoreLook`: `members.store_look`, the vault's in the `vault_look` setting; `LookForm` reads the form fields `look_sent`, `look_bg`, `look_name`, `look_tile`, `look_note`, `bg_photo`). The store's page (and its binder pages) carries classes `lk-bg-*`, `lk-name-*`, `lk-tile-*` and the variables `--sacc` (the store's colour, what was the border colour, now called Store colour) and `--sbg` (its photo); the page behind is one fixed layer under everything (`.storefront::before`, with the body made see-through on that page), so words stay on the site's solid panels and the readability check still passes. Only the choices there are can be saved, and only a photo the site stored. |
| 19 Nov 2026 (social, release 7c) | The owner: "an option to share a binder within messages and even outside the site like facebook messenger". **Share** (`static/share.js`, on every page; any button with `data-share-ref`, `data-share-url`, `data-share-title`) opens one small window for a binder (a seller's or the vault's), a store, a card or a post: **Send in a message** (the Messages people list as "Send to", then that chat opens with it attached, ready to send), **Messenger** (a phone's own share menu, "Messenger and other apps"; on a computer the link is copied and Messenger opens), **Facebook**, **Copy link**. The buttons: a binder's bar (not on a binder only its owner may see), every store's header (owner and visitors), a card's page, and a post's Share (which used to open the phone's menu or copy the link). In Messages it rides as a card message whose copy says what it is (`messages::share_copy`: `src: "share"`, `ref`, `icon`, `go`); unlike a card of the conversation it can go to anyone, so only what anyone with the link may open is copied (never a private binder, a hidden store or its cards). `POST /dm/open { to, share }`, `POST /dm/{thread}/send { card: { share } }`. A shared binder's link shows the binder's own cover and words (the store page's `og:image` and description), and a binder not everyone can see is not indexed. |
| 19 Nov 2026 (social, release 7b) | The owner: "personal page and store should have different settings ... when i add a link in store, it shows up in my personal page too, there should be an option to change the profile pic of the store alone too". **A trainer's own page** (`/trainers/{id}`) shows only what **Account Settings** (`/me`) holds: picture, name, about, and now its own cover photo and links (`members.page_cover`, `page_fb_url`, `page_ig_url`, `page_tiktok_url`, `page_links`; `members::save_page`). **A store** shows only what **Store settings** (`/me/settings`) holds: the links, cover, place and contact that were there (they were typed there), plus its own picture (`store_avatar`; the trainer's shows until one is picked: `Member::store_pic()`) and its own words (`store_bio`, copied once from the trainer's about so stores read as before). The store's picture shows wherever the store's name does: its page, Stores, the feed's store posts and rows, "Sold by" on a card, bookmarks, the Store Manager's corner, and a seller in Messages. Each page has its own settings button: Account Settings on the page, **Store settings** on the store (the small Edit store window is gone; the border colour moved into Store settings with an "Other" colour). Fixed on the way: a store's extra link written with a name (`Carousell \| https://…`) never showed (`Member::social_links` checked the whole line as an address). Left as it is: any change to the page or to a store's links goes back for approval, which also hides the store until then. |
| 19 Nov 2026 (social, release 7a) | The owner: photos in posts should open "similar to facebook", comments inside comments, your own comment to edit and delete, and "the pictures are not perfectly showing for desktop and tablet". **The photo window** (`static/feed.js`, `.pv`): a post's photo opens big and always fits the screen; ✕ top left (also Esc and the phone's Back), arrows / swipe / arrow keys through the post's photos with "2 of 3"; the post sits beside it from 1000px wide, under it on a phone or an upright tablet. It is the real post, moved into the window while it is open (a holder keeps its place in the feed), so its likes, comments and the comment box (photo and camera) work there and stay on the post; every feed listener is on `#feed` and on the window (`listen()`). **Any photo opened big** (`.dm-lightbox`: comment, chat and vouch photos, a card up close) now fits the screen too (a tall one ran off the bottom on tablets and computers) and its ✕ is plain to see. **Replies to replies:** a reply keeps the comment it answers (`comments.parent_id`, no longer flattened); replies step in twice, deeper ones stay there and say "to Name"; the comment box shows "Replying to Name ✕". Deleting a comment takes its whole thread (`social::THREAD`). **A comment's own buttons** are finger-sized words on one row (Like, Reply, Edit, Delete; the time sits beside the name); Edit is a box with Save and Cancel. **A deleted comment tells someone** (`feed::take_comment`): deleting your own tells whoever wrote the post (the vault's posts: its admins); the vault's team taking one down tells its writer. |
| 19 Nov 2026 (flow, release 6b) | **One binder editor** for every store, on the store page (`static/binder-tools.js`): Edit binders (or the Managers' Binders lines, which open `{store}#edit-binders` and `/store/vault#edit-binders` with editing on) gives each binder one **Edit** window: the cover (Pick a card, the automatic one, or Upload a photo), Name, What's in it, Who can see it (Everyone, Only people with the link, Only me / Only the vault's staff; Copy its link for link-only), Add cards, Arrange or remove (the binder's own page), Delete. Nothing changes until Save. **Sellers get photo covers** (`member_binders.photo`, `POST /me/binders/{id}/photo`; a card picked for the cover lets the photo go and its file is deleted; a deleted binder or trainer takes its photos with it). **Vault binders get words and who can see them** (`collections.description`, `collections.visibility`; `POST /admin/binders/{handle}` takes `description`, `visibility`): a link-only or private vault binder is left off its store, Stores, the feed, the menu, the sitemap, `/api/binders`, a card's binders in the API, search suggestions and the visitors' search file; its page opens by link (Unlisted, not indexed) or for staff only (Private, 404 for anyone else). A binder's page says "Link only" or "Private" in plain words, and a vault binder's page shows its words. The Store Manager's and the Vault Manager's own Binders pages are gone (`/me/binders` and `/admin/binders` redirect to the store; their JSON stays), with `store_binders.html`, `admin_binders.html`, `binders.js` and the vault store's Manage binders link. |
| 19 Nov 2026 (flow, release 6a) | The owner said yes to Release 6 after a page of pictures. **One menu:** from 700px the menu under your picture (`#meMenu`, fb_bottom.html) holds everything once (your page, My store or Apply as a seller, Orders, Cart, Bookmarks, Notifications, Vouches, Ask the Vault, Admin, Log out); the ☰ button and the wide screen's left column went; phones keep the Menu tab and its sheet (`#fbMenu`). The cart has a header button from 700px while it holds cards (`.hd-cart`, `[data-cart-open]`, cart.js), so a visitor can open it. **One place for a seller's cards:** Account Settings › My cards shows a seller's cards as pictures (★ and drag order stay here) with one button to Listings; collectors keep the full editor. **One tile:** a seller's store draws its cards with the vault's tile (card.html's parts: price with Message and +, name, set, condition and finish, one line of the note); the owner gets a small Edit (its window sets stock). **The vault's Store settings** (`/admin/store`, Vault Manager rail): cover photo, about, links, how buyers pay and the border colour (`sellers::VaultStore`, settings `vault_cover`, `vault_bio`, `vault_*_url`, `vault_links`); the vault's store page and its card on Stores show them; Orders lost its pay box and says when buyers would see no way to pay. |
| 19 Nov 2026 (flow, release 5) | **The loops finished.** Follow people, not only stores: Follow on every trainer's page (`/trainers/{id}/follow`, the same `store_follows` row as their store's Follow; someone without a store hears "X now follows you."); followers hear when a trainer posts (`feed::tell_post_followers`, one line per author) or puts up cards. Every follower count lists the people in a window (`static/people.js`, `window.bbvPeople`): on a trainer's page, a store's header and the Overview. A post's faces list who reacted (`/posts/{id}/reactions`). Report on every post (its menu) and comment (by its Reply), once each (`reports`), to the admins and moderators (kind `staff`, now offered to moderators too). "I've paid" on an accepted order, for the buyer (`buyer_paid_at` on `reservations`, for the whole order, and on `vault_orders`): the seller or the vault's admins hear, and their order shows "The buyer says they paid". |
| 19 Nov 2026 (flow, release 4) | **One way to do each thing.** Ask became Message: "Ask in the store" on a card, "Ask the seller" and "Ask the vault" on orders, and the store page's Ask sheet (gone with its code in `storefront.js`) are Message buttons with the card attached; a trainer without a store messages the vault instead of the public chat. Every order has Message, both sides: `/dm/open` takes `{ card, to }` when the card is the writer's own (a seller writing to a buyer); the vault's Orders link to `/admin/orders/{id}/message` (the vault's conversation with that buyer). The Questions tab and the `inquiry` choice show only while old questions exist (`/store/{slug}/ask` stays for old callers). Notifications sits in the Store Manager's frame. One name per thing: "order" in every sentence people read (the card status Reserved stays), "Overview" (also the Vault Manager's), "Store Manager", "My cards". A tap goes to the thing itself: "My store" opens the store (`/me/store/page`, `who` gives `store`, `fb-shell.js` points `[data-my-store]` at it), Log in returns to the page you were on or Home (`landing()` falls back to `/`; a plain tap on Log in or Register remembers the page), Register lands on the new trainer's page unless it was for something, a follow opens the follower's page, a card for sale says "Sold by" the store and opens it, the Overview's numbers open Listings, Sales and Binders. Twins: a store's card shows its finish, the seller's note and copies left (`left` = qty minus open orders); one cart is one order per store (`reservations.batch`, `sellers::order_groups`; the buyer hears answers to one order as one line, `answer:{batch}:{status}`); the vault's store has the Condition filter (`db::Query.cond`, `db::conditions_for_sale`); the Vault Manager's Binders is one list (the old home page's groups and switch went; `on_home` is shown nowhere). Each fact on its own line on orders (both sides, and the vault's) and as separate pieces on Listings. |
| 19 Nov 2026 (flow, release 3) | **Nobody misses anything** (the owner: liking a post or following someone should reliably notify the recipient; comments should take a like and a photo). The bell is live on every page and size: `sellers::notify_from(c, member, actor, kind, group, message, link)` (`notify` is the same with no one and no group) writes the line, wakes the member (`sellers::wake`/`take_woken`), the 3-second loop bumps their live connection, `/dm/poll` answers `notes`, `dm.js` fires `bbv:notes`, `fb-shell.js` moves the number and redraws an open list. `notifications.actor_id` (who did it: their picture or first letter beside the line; 0 is the site's logo) and `notifications.ref` (a group: while a line with the same group is unread it is brought up to date and the phone isn't buzzed again: `react:{post}`, `creact:{comment}`, `cart:{buyer}` for one cart per store, `following:{store}:{day}`, `staff:waiting`). A tap on a line reads it (`POST /api/notifications/read {id}`); the drop-down has See all; on a phone the bell opens the Notifications page, which shows the pictures too. Defaults in one place (`sellers::default_pref`): phone and bell for everything people wait on, bell only for views, price hints, end dates and status. Settings: one choice per kind (Phone and bell, Bell only, Off; `{event, how}`), Email only once the site sends mail; `sale` went (never sent), `review`, `following` and `staff` came; the views box left Store settings (folded into the `view` choice once, `views_switch_v`). Alerts are offered as a card after Send order, on Orders and on the Overview. New lines: a like or comment on the vault's post (its admins), a vouch (its store; a reply, its writer), a page approved or a store opened (the trainer), a trainer waiting (the admins), a store you follow putting up cards (once a day, never for old days), paid orders inviting a review; follows, offers, order answers, reviews and messages say who. Comments: like with the six reactions (`comment_reactions`, `comments.like_n`, `POST /comments/{id}/react`, the writer hears), and one photo, uploaded or taken (`comments.image`, `thumb`, `bytes`, counted in the photo space; `POST /posts/{id}/comments` is a form now: `text`, `parent`, `photo`). |
| 19 Nov 2026 (flow, release 2) | **One Add cards page** (the owner: "keep the add a card tool in a dedicated single page and not in multiple tabs corners and pages"; "keep listings for just managing listings"). Sellers and collectors: `/me/add` (`store_add.html`, `add-page.js`), four ways at the top: Search, Scan a card (sellers, while scanning is on), By set (new for them: `GET /me/sets/search`, `/me/sets/cards`, on `market::set_search` / `set_cards`, the same limit as `/me/search`), Not in the list; the card language for Search and By set; "Added just now" (every card the add window adds, through its `bbv:added` event). A seller's card goes through "Add to your store"; a collector's joins their binder. `?binder=id` (a seller's binder's Add cards): the store's own cards first (`/me/binders/{id}/choices`, each opens the window filled in), new cards go into it, Done goes back to the binder's editor (`#edit`). Every Add cards door goes there (the Manager's bar and rail, the store page, Account Settings, Stores, the Overview, a binder's editor: its own add window is gone); Listings lost its add panel (`/me/listings#add` and `#scan` forward). Sellers may load the vintage Unlimited pictures (`/admin/cardimg/unlimited`, within a limit), which Search and By set show them. The vault: Add cards, Add by set and Add by hand are one page with the same four ways (`/admin/quick`, `?by=set`, `?by=scan` with the vault's own scans going into the tray, Not in the list is `/admin/new` with the ways on top; `/admin/sets` forwards); `?binder=handle` ticks that binder in the tray and lists the vault's other cards first to add in one go, Done goes back. |
| 19 Nov 2026 (flow, release 1) | From the owner's review of how the site flows (18 Nov 2026), the first of five releases. **Your name opens your page**: the menu's first line and "See your profile" go to `/me/page` (forwards to `/trainers/{id}`; `fb-shell.js` points them there once it knows who you are); `/me` is now called **Account Settings** everywhere, and your own page's "Edit profile" button reads Account Settings. **Scan a card** lost its menu line and page: it is the Scan tab of Add cards (`/me/listings#scan` opens it; `/me/scan` forwards). **Easy to read**: `--muted` `#c9cbd8` and `--faint`/`--steel` `#a3a7b9` (were `#aeb0c0`, `#7d8194`), no font under 12px in any stylesheet, people's own words in `--text`; a store's details one per line (`.sf-rows`: Location, Open since, Find me on, Pay with, How to pay, Contact), the vault's store and a trainer's "Find me on" alike; a store's card on Stores and a binder's bar without dot-strings. The smoke check measures contrast and size on every page and in the card window, and now also opens a trainer's store and a trainer's page. **Prices only where cards are shopped**: none in the feed (posts, the stores row, the sets of cards), price moves gone (the weekly post and the right column; `social::AUTO` lost "prices", old ones hidden), none on trainers' pages (a highlight card's window shows no value; a trainer's cards for sale are one line to the store), none in the search box's suggestions (the set instead). |
| 18 Nov 2026 (sort) | **Sort and Hide in a binder's editor** (sellers' binders and the vault's): "Sort:" A to Z · Z to A · By set · By number · By evolution, worked out on the site (`src/binder_sort.rs`, `POST /me/binders/{id}/sort`, `POST /admin/binders/{handle}/sort`) and saved like a drag, with Undo. By evolution asks TCGdex once per card (six at a time) and keeps the answer in the new `card_dex` table. Each card has Hide / Show: a seller's card becomes Hidden (the listing's own status); a vault card is hidden in that binder only (new column `collection_products.hidden`, `POST /admin/binders/{handle}/hide`), so the binder's page, count, tile, cover, the finder and the card's "binders" list leave it out. Saving, copying or bulk-moving a card keeps it hidden there. |
| 18 Nov 2026 | **Every store alike** (the owner: "all stores are official"): the vault's store is listed and drawn like trainers' stores on /stores (one card, `stores_card.html`; no "Official" badge anywhere; the store that last put up or changed a card comes first, `sellers::last_active`). Every store page has Follow, Message and Bookmark and the same four numbers (cards for sale, binders, followers, Vouches); **Vouches** opens a window (`store-head.js`, `GET /store/{slug}/vouches`): vouches now belong to a store (`vouches.store_id`, 0 the vault's, the older ones the vault's) and a seller's reviews from finished sales are listed there too (the Reviews section moved into it). A **border colour** per store (Edit store; `members.store_border`, setting `vault_store_border`, gold by default; `sellers::border_ok` refuses colours too dark to see). The store header is compact and side by side. **Apply as a seller** (`/me/sell`, menu): an optional note and photos go into the trainer's chat with the vault, where the whole application goes on; admins answer under Admin › Sellers › Applications (a number on the tab and a banner on the Overview), and the answer is written into that chat. **Bookmarks** (`/me/bookmarks`, menu, with a number): stores and vault cards (`bookmarks`) and sellers' cards (the old ♥ Save, `listing_saves`), each card under its store, with Add to cart and "Add all". The **cart** is a row in the menu under Orders (its number on the Menu buttons); the top bar has no cart. "Top of the vault" is off the home page. A seller's binder **Add cards** also finds new cards from the card list; the Store Manager's binders page can **Delete** picked cards (with Undo). The VM: flyctl never holds a job's lock (it kept the picture job stopped 14-17 Nov), "More like this" asks the site again when it is slow |
| 17 Nov 2026 (later) | The Vault's tab is gone on every screen and Stores is the highlighted place (the logo in a gold ring, in the header and the phone's bar; lit wherever cards are shopped); "Browse stores" on the home banner and the 404 page, "‹ Back" on a card's page; Register beside Log in at the top from 700px; one word per action: Register (Trainers, Vouches, Vault chat, Ask's answers and its AI's instructions) and Log in (Stores); the name boxes on log in and register match the password box |
| 17 Nov 2026 | "Register" replaces "Join free" everywhere a visitor reads it; the phone's bottom bar is four equal tabs (Home, Stores, Ask, Menu) with a gold pill for where you are; Log out at the foot of the Menu on every screen (phones had no way to log out); the release check fails if the phone's tabs leave a gap. Lesson: the 16 Nov bar went live with `fb.css` still at `?v=22`, so Cloudflare kept serving the old five-slot stylesheet and the bar showed an empty slot. Raise the number every time |
| 16 Nov 2026 | From a click-through review (logged out, buyer, seller, admin; four screen sizes): the cart's + on a tile takes the card out on a second tap, the cart's + stops at the copies there are, a tap outside the cart closes it; log in and join say why you were sent there; the site's own "are you sure?" box (`window.bbvConfirm` in `fb-shell.js`) replaces `confirm()` everywhere a choice can wait; a trainer not yet approved is told so in the post box; the finder's counts in the site's words and a "Show N cards" button in the phone filters (both from `fb-shell.js`, since the finder is not rebuilt); a card's language and how meetup/shipping works on its page; one name (the store name) per seller in orders, offers and alerts; messages survive an unreadable poll reply |
| 14 Nov 2026 | On a phone, Add to cart stays in a bar above the bottom tabs; visitors counted on the Overview; My trainer in the Manager's frame; the old look removed (one stylesheet, one frame); the phone, tablet and computer check (`tests/browser/smoke.mjs`) |
| 10–14 Nov 2026 | The Facebook-style redesign (`docs/REDESIGN.md`): feed, The Vault, stores and their binders, the Managers, private messages and calls, alerts with the site closed, suggested prices by condition |
| to 10 Nov 2026 | Card finder, binders, vintage printings, Japanese sets in English, lite mode, SEO, trainers, vouches, chat, Ask the Vault |
