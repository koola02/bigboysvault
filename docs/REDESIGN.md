# Big Boys Vault: Facebook-style redesign

## Context

Russell wants the whole site rebuilt to look and work like the Facebook app, on desktop, tablet and phone, while keeping Big Boys Vault's own brand. Today the site is a cinematic showcase (logo gate, starfield, card-fan hero, glow effects) with features bolted on over time: card finder, stores, vault chat, Ask the Vault, private messages. Facebook's strength is a calm, flat frame where everything has one obvious place: a feed in the middle, navigation at the sides, Marketplace, profiles, Messenger, notifications.

Decisions already made by Russell:

- **Colours:** dark and gold only (Facebook's dark-mode layout in our colours).
- **Home:** the feed is home. The showcase becomes the **Stories row** at the top. Seller stores appear as a **carousel unit** in the feed (like Facebook's carousel ads).
- **Social features:** posts with likes and comments, follow trainers, stories, share to feed.
- **Rollout:** built in phases behind a "new look" switch that admins see first; then one switch moves everyone, and it can be switched back.

"Inspired by Facebook" means its layout patterns. We do not use Facebook's name, logo, blue, icons or wording.

---

## 1. The look (design system)

One list of colours and sizes ("tokens") drives everything, in a new stylesheet `static/fb.css` that replaces `app.css` + `modern.css` + `vault.css` (4,600 lines) for converted pages.

| Facebook dark mode | Big Boys Vault |
|---|---|
| Page background | near-black navy `#0b0d14` |
| Card surface | `#141826`, 8px corners, hairline border, almost no shadow |
| Hover / input fill | `#1d2233` |
| Blue (active tab, links, primary button) | **gold `#efc873`** with dark ink text |
| Grey secondary button | flat `#262b3d` |
| Text | primary `#f6f2e8`, secondary `#aeb0c0` |
| Red badge | unchanged red count badges |

- **Type:** system font for all body text (fast, feels like Facebook). The brand font "Anybody" stays for the wordmark, prices and page titles.
- **Kept from the brand:** logo, gold, peso prices in the display font, condition colours (as small chips), card-shaped tiles, the holo/inspect effect on a card's own page.
- **Dropped in the new look:** intro logo gate, starfield/WebGL sky, card-fan hero, heavy glows and squircles. Flat and fast.
- **Sizes:** header 56px; feed column 1088px max (Russell, 12 Nov 2026: every content column 60% wider than first drawn, on every page; phones are unchanged and tablets simply fill their width); side columns 300px; touch targets 44px; Marketplace filter column 360px.

## 2. The frame on each device

**Desktop (1536 and wider): three columns** (the feed is 1088 wide; the side columns take what is left, 208 to 300 each)

```
[logo][ search……… ]   [Home][Marketplace][Stores][Trainers][Chat]   [cart][msgs][bell][me]
┌ left 300 ────────┐ ┌ feed 680 ──────────────┐ ┌ right 300 ───────┐
│ Me               │ │ Stories row            │ │ Featured card    │
│ Marketplace      │ │ "What's on your mind?" │ │ Price movers     │
│ Stores, Trainers │ │ post · post            │ │ Contacts (online)│
│ Ask the Vault    │ │ Stores carousel        │ │ Vault chat       │
│ My cards, Orders │ │ post · post …          │ │                  │
│ Shortcuts=Binders│ │                        │ │                  │
└──────────────────┘ └────────────────────────┘ └──────────────────┘
```

- 1100–1535 wide: left column hides (its items live in the Menu); feed + right column (232 to 300), so the feed reaches its full width from about 1366.
- The card section is called **The Vault** (Russell found "Marketplace" too generic); its sign is a vault door.
- Header icons open **popovers** like Facebook: Messenger list, Notifications, Account menu. Cart opens a right-side drawer.
- Chats dock bottom-right (already built).

**Tablet (700–1099): one centred column**

- Same header with the centre tabs as icons; no side columns; feed/pages centred at 680px.
- Filters and menus slide in as sheets. There is no bottom bar on tablets, so chats dock at the bottom-right corner. Landscape tablets 1100+ get the desktop frame.

**Phone (under 700): the Facebook app**

```
[logo] Big Boys Vault            [search][cart][msgs]
──────────────────────────────────────────────────────
 Stories row
 composer
 feed…
──────────────────────────────────────────────────────
 [Home] [Marketplace] [ASK logo] [Alerts] [Menu]
```

- Bottom tab bar (thumb reach): Home, Marketplace, Ask (centre, the logo), Notifications, Menu.
- **Menu tab** = Facebook's menu page: profile row, grid of shortcut tiles (My cards, My store, Orders, Wishlist, Messages, Vault chat, Stores, Trainers, Binders), settings list, Admin section for staff, Log out.
- Full-screen chat, cart, filters, share sheet, story viewer. Sideways phones use the phone frame.

## 3. Page by page (Facebook → Big Boys Vault)

| Facebook | Our page | What changes |
|---|---|---|
| **Feed** | `/` | New. See section 4. |
| **Stories** | top of feed | The showcase. See section 5. |
| **Marketplace** | `/all`, `/search`, `/collections/*` | Desktop: left column with search, categories (binders) and the finder's filters; grid of tiles with **price first**, then name, set · condition. Tablet: filters in a sheet. Phone: chip row (Categories, Filters, Sort) + 2-column grid. Two tabs: "The Vault" and "Trainers' listings". |
| **Marketplace item** | `/products/*`, trainer card pages | Desktop: big image left, 360px panel right: name, price, condition, market trend, **Add to cart** (gold), **Message**, **Share**, Save; details list; seller card with Follow; "More like this" below. Opens as an overlay from grids (existing floating window). Phone: image on top, sticky action bar at the bottom. |
| **Profile** | `/trainers/{id}` | Cover photo, round avatar, name and badges, buttons (Follow, Message, Visit store), tabs: Posts · Cards · For sale · Wishlist · Vouches · About. "Intro" card at the side on desktop. |
| **Page / Shop** | `/store/{slug}` | Same header as the profile with the **Shop** tab open; binders shown like shop collections; reviews tab. URLs unchanged. |
| **Friends** | `/trainers` | Grid of trainer cards with Follow buttons; "Trainers you may know". |
| **Pages** | `/stores` | Grid of store cards (cover, avatar, on sale, Follow / Visit). |
| **Messenger** | chat window, `/me/messages` | Header popover with recent chats; full page becomes two panes (list + chat) on desktop/tablet. Already themed. |
| **Notifications** | bell | Popover on desktop/tablet (avatar, text, time, unread dot, All/Unread); its own tab on phones. |
| **Search** | header | Pill search with a dropdown of cards, sets, trainers, stores, and "Ask the Vault: …". Results page with tabs: All, Cards, Listings, Trainers, Stores, Posts. |
| **Meta AI** | `/ask` | Ask the Vault, restyled as an assistant chat; centre tab on phones. |
| **Group chat** | `/chat` | Vault chat, styled like a Messenger group. |
| **Settings** | `/me`, my cards, listings, orders, notification settings | Left settings list + content cards on desktop; list → detail on phones. |
| **Business Suite** | `/admin/*` | Same frame with a left list of admin sections. Last phase. |
| **Log in** | `/login`, `/join` | Logo and tagline left, form card right; stacked on phones. |

Cart, card camera, private messages, wishlist alerts, price history, Ask with Gemini, VM jobs: all kept, restyled.

## 4. The feed (home)

Order: welcome banner (signed-out only: logo, one line, Join / Log in) → **Stories row** → composer → items, 8 at a time.

**Endless scrolling.** More items load by themselves as you scroll, and the feed never ends: when the posts run out it carries on with sets of cards "From the vault", binder after binder, round and round.

**Automatic posts have switches.** Full admins can turn each kind on or off (new cards, sellers' listings, vouches, price moves, new trainers, the vault sets) at `/admin/look`, or from a post's ⋯ menu. Off: those posts leave the feed, with their likes and comments kept for when they're switched back on.

**What appears in the feed**

- **Trainers' posts:** words, up to 4 photos, or an attached card.
- **Vouches:** shown as posts ("vouched for Big Boys Vault").
- **New in the vault:** one post per day from Big Boys Vault with that day's new cards as a photo grid.
- **New store listings:** one post per seller per day.
- **Price moves:** "Charizard ex is up 18% this month" with the small chart.
- **Announcements** from admins (can be pinned to the top).
- **Shares** of a card, listing or store with the sharer's words.

**Units placed between posts**

- **Stores carousel** after the 2nd post and about every 12 after: a sideways-scrolling row of store cards (cover, avatar, name, one hero listing with price, **Visit store** / **Follow**), labelled "Trainers' stores" where Facebook says "Sponsored".
- **Trainers you may know** carousel (Follow buttons).
- **Ask the Vault** prompt card.

**Under each post:** Like (tap) with reactions on long-press/hover (Like, Love, Fire, Wow, Haha, Sad), Comment (one level of replies), Share (to feed, in a message, copy link, phone share sheet). Counts shown like Facebook.

**Following:** follow any trainer or store; posts from people you follow rank first; you're notified of their new posts and listings (can be muted). Today's store follows carry over.

**Moderation:** only approved trainers can post; hourly limits; admins and moderators can hide or delete any post, comment or story (these buttons obey the "hide admin buttons" switch); a Report link on every post notifies admins.

**Editing, like Facebook.** Whatever is yours has its Edit control right where it appears, in the ⋯ menu or beside it, and opens in place:

- Your own post: edit the words, take photos off, add photos; delete. Your own comment: edit, delete. Both show "Edited".
- Full admins post as **Big Boys Vault** (the logo and the shop's name), and any full admin can edit, pin or delete what the vault posted.
- The posts the site writes can carry your own words: a seller on the post about their listings, a full admin on the vault's posts. The site's refresh never touches those words.
- As pages are converted, the same rule applies there: a seller's listing, store header and binders, a trainer's profile, and (for admins) any card in the vault get their Edit control on the page itself.

**Right column (desktop):** featured card (owner's pick), price movers, contacts online (opens a chat), vault chat preview.

## 5. Stories row (the showcase)

- **First tile:** "Add story" (signed-in trainers): a card photo taken with the card camera, optional words and attached card. Lasts 24 hours.
- **Vault stories (gold ring, never expire):** each showcase section the owner has today (Showcase, spotlight rows, binders, trainers' picks) becomes one story tile, with the first card as its cover. Tapping opens the **full-screen story viewer**: one card per step, progress bars on top, tap or arrow for next, with **View card / Add to cart / Message** at the bottom. Desktop shows the viewer centred with a list of stories at the left, like Facebook.
- **Trainers' stories:** gold ring until you've seen them.
- **Home editor** (`/admin/page`) becomes the **feed editor**: order of vault story tiles, pinned posts, the featured card, the welcome banner text.

## 6. How it's built (technical)

**The "new look" switch**

- `src/look.rs` (new): a per-request flag. A small middleware reads the cookie `bbv_look` and the setting `site_look` (`blocks::setting`), plus a list of routes already converted, and runs the request with a task-local "new look" flag. Templates ask `crate::look::is_new()`.
- `templates/base.html`: `{% if look::is_new() %}` loads `fb.css` + `shell_fb.html`; otherwise today's sheets and header. A page that isn't converted yet always renders in today's look, so nothing is ever half-styled.
- `cached()` in `src/main.rs` adds the look to its cache key (one line, covers every cached page).
- Switch control: "Try the new look" on My profile (admins first, via `admin::power`); at switch-over the setting flips for everyone.

**New data** (`src/social.rs` in the library, tables created in `db::open` like `dm::migrate`)

- `posts(id, member_id [0 = the vault], kind, text, payload JSON, like_n, comment_n, share_n, pinned, hidden, created_at)`: one table for every feed item, so the feed is one indexed query (the server is small).
- `post_images`, `reactions(post_id, member_id, kind)`, `comments(id, post_id, member_id, parent_id, text, hidden, created_at)`.
- `follows(follower_id, followee_id)`; `store_follows` copied in once (settings-flag guard, like `price_src_v`).
- `stories(id, member_id, image, thumb, card JSON, text, created_at, expires_at)`, `story_views`.
- Backfill once: existing vouches, the last 30 days of vault arrivals and store listings become `posts` rows.
- Feed rows are written where things already happen: `members::post_vouch`, product create in `admin.rs`, listing create in `sellers.rs`, `ops::save_market` (price moves), announcements in `store.rs`.

**Reused, not rebuilt**

- Photos: `community::store_photo`, `gated`, `photo_cap_mb` (posts, comments, stories count toward the photo space cap).
- Card attachments: `card_copy` from `src/messages.rs` (moved to a shared place).
- Notifications: `sellers::notify` + `EVENTS` (new: like, comment, follow, followed-post), grouped per post the way `dm::notify` groups per chat.
- Text and limits: `members::clean_text`, the vouch hourly-limit pattern, `community::acting` / `same_site`.
- Staff tools: `admin::power`, `.staff-only` + `html[data-staff-hide]`.
- Live badge counts: the existing `/dm/poll` long-poll.
- Scripts keep their class names, so `cart.js`, `dm.js`, `card-cam.js`, `card-float.js`, `vouches.js`, `vault-talk.js`, `suggest.js` work under the new styles with small changes.

**New files**

- `static/fb.css` (tokens, frame, every component), `static/fb-shell.js` (header popovers, menu, tabs), `static/feed.js` (composer, reactions, comments, share, infinite scroll via the already-loaded htmx), `static/stories.js` (row + viewer).
- `templates/shell_fb.html`, `feed.html`, `feed_item.html`, `story_row.html`, `stores_carousel.html`, `profile.html`, plus restructured `product_body.html`, `store.html`, `trainers.html`, `stores.html`, `messages.html`, `login.html`.
- `src/social.rs` (lib), `src/feed.rs` (handlers), `src/look.rs`.
- `crates/finder/src/lib.rs`: the finder's view split into a filter column and a grid so CSS can place them per device (finder version bump in `base.html`).

**Routes:** `GET /` (feed under the new look), `GET /feed?before=`, `POST /posts`, `POST /posts/{id}/react`, `GET|POST /posts/{id}/comments`, `POST /posts/{id}/hide|delete|report`, `POST /follow/{id}`, `GET|POST /stories`, `POST /stories/{id}/seen`, `POST /share`, `GET /admin/look` (style guide).

**Kept for search engines:** every URL, titles, descriptions, structured data, sitemap; the signed-out feed is real server-made HTML.

## 7. Phases

Each phase ends the same way: all tests pass, a screenshot sweep at 360, 390, 820, 844×390 (sideways), 1100 and 1440, admins try it on the live site with the switch, Russell approves.

| # | Phase | Size | You'll see |
|---|---|---|---|
| 0 | **Foundations**: the switch, tokens, base components, the frame on an empty page, a style-guide page | small | The frame and every button, card and form on all devices, for sign-off on the look |
| 1 | **Frame + buying**: header, side columns, bottom tabs, Menu; Marketplace and finder; item page and overlay; cart drawer; search dropdown; notifications and Messenger popovers; two-pane Messages; login/join | large | Browsing and buying fully in the new look |
| 2 | **Feed**: posts, reactions, comments, share, stores carousel, right column, welcome banner, feed editor | large | Home is the feed |
| 3 | **Stories**: vault story tiles from the showcase, story viewer, trainers' 24-hour stories | medium | The showcase as stories |
| 4 | **People**: follow, new profiles, stores as pages, trainers and stores directories, suggestions | medium | Facebook-style profiles and following |
| 5 | **Account pages**: My trainer, my cards, listings, orders, settings; Ask; Vault chat | medium | Everything a signed-in trainer uses |
| 6 | **Admin pages** in the new frame | medium | Admin matches the site |
| 7 | **Switch-over**: new look for everyone (switch-back kept for 2 weeks), then the old styles and effects are deleted, docs updated | small | One look, lighter site |

Each phase is deployed with the usual steps: live database backup, `fly deploy`, graph update, patch notes.

## 8. Checks (how each phase is verified)

- `cargo test`: new tests for feed order (followed first, pinned on top, hidden never shown), reaction and comment counts, follows carry-over, story expiry, the look switch and cache key.
- End-to-end scripts like the messaging one (`dm-test.sh` pattern with real sign-ins): post, react, comment, share, follow, story, moderation, limits, signed-out and cross-site refusals.
- Screens: the `shots.sh` + `cdp.js` harness at the six sizes for every converted page, checking no sideways scroll, nothing overlapping, bottom bar and chat not colliding.
- Speed: page weight and first paint compared with today on a throttled phone profile; target a lighter page than today.
- Live: the VM's 15-minute `checks.sh` markers updated for the new pages; restore drill unchanged.

## 9. Things to know

- **Moderation work goes up.** Posts, comments and stories from any approved trainer mean more to watch. The plan includes limits, hide/delete, reports and admin alerts.
- **The home page changes for Google too.** Titles and structured data stay; the feed is readable by search engines; card pages don't change address.
- **Photo space.** Post and story photos use the same capped space as other uploads; expired stories are deleted hourly.
- **Voice calls, Gemini, the VM jobs** are untouched by this work.
- **Not copied:** Facebook's logo, name, blue, icon set or text.

## Status

As of 12 November 2026, behind the new-look switch:

- **Live:** the frame and style guide; the feed as home (endless, reactions, comments, automatic posts with admin
  switches, editing of posts and comments, the showcase's cards as the stories row); The Vault (the card section,
  first called Marketplace) and each card's page, which opens as a floating window with More like this; the admin's
  editor on a card's page; Trainers and Stores, a trainer's profile, a store, a listing's page, the vouches wall;
  the seller's editor on their own store; Ask the Vault and the vault chat as chat windows; log in and join;
  two-pane Messages with New message (anyone can message anyone); Notifications; every content column 60% wider
  than first drawn.
- **Live, the Manager:** the admin's Vault Manager and a seller's Store Manager, laid out the way an ads manager is:
  a tools rail at the left, a title bar, Overview with the numbers first, Cards and Listings as tables you edit in
  place (a switch on every listing, tools for many rows at once), Add cards, Add by set, Binders, Orders, Trainers,
  Showcase, Sellers, Store settings, and the card form both as a page and as a window over the table. A buyer's My orders and a
  moderator's Trainers page use the same title bar without the rail. The styles are `static/mgr.css`, loaded only
  on those pages (each template asks for it through `{% block head %}` and `mgr_head.html`).
- **Live, the showcase tool:** `/admin/showcase` in the Manager's rail picks the showcase's cards, takes them out and
  puts them in order (drag or arrows), saving by itself (`POST /admin/showcase {ids}`, `blocks::set_showcase`). The
  page shows the strip as the home page draws it. Admins also get an "Edit showcase" tile at the end of the row on the
  home page. The stories row reads one section only: `blocks::showcase` (the shown one, else the first).
- **Live, the tables:** in Cards and Listings a column's name sorts by it, and the column tapped before settles the
  ties (two columns at most). The server sorts, so it holds across pages: the address carries it as
  `sort=price:desc,card:asc` (`db::column_sort`, with the columns each table allows). The line at the right of a
  column's name drags to change its width (double-click sets it back); widths are kept on the device
  (`localStorage` `bbv:cols:<table id>`) and written into the table's `--cols`. All of it is `static/mgr-table.js`.
  The SRP line under a price wraps instead of being cut.
- **The new look is on for everyone** (the owner switched `site_look` to `new` on 12 November 2026).
- **Next:** stories viewer and trainers' stories, follow, share to feed, the feed editor (until then "Home page" in
  the Manager opens today's home page editor, in today's look, because that is the page visitors still see);
  My trainer (`/me`); then the switch-over.

Owner's rules added along the way: editing happens where the thing is shown and must feel natural; Ask the Vault
opens as a chat, not a page; pages are 60% wider (phones unchanged, tablets fill their width); the card section is
called The Vault; the stories row shows only the showcase's cards; a card tapped in The Vault opens in a floating
window with More like this; everyone can message everyone (30 new conversations a day for trainers who aren't
staff); the admin's and the seller's dashboards should feel like home, the way an ads manager does; the header's
five places carry their words and are as wide as the feed (on a three-column screen they sit exactly over the feed's
column; on smaller screens they fill what is left between the search and the buttons, with the word under the icon
below 1280px; phones keep the worded tabs at the bottom); the showcase is edited from the Manager's sidebar; the
new look has no "Show admin buttons" switch (admin buttons are simply there for admins, and a device that hid them in
today's look shows them in the new one).

The search suggestions (suggest.js) open in the browser's top layer where they can, so no page box places them:
`fb.css` attaches the list to its search box with anchor positioning, as `modern.css` does for today's look. Without
those rules the list falls to the bottom of the screen.

One thing to remember when a script in today's look meets the new one: a class name can mean something else there.
`quick.js` marks a tapped card `pop`, which in the new look is the header's drop-down; `mgr.css` takes those styles
back off (`.qa-card.pop`). Check for such names before bringing another old page across.
