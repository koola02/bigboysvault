# Sellers' stores: how it works and every route

Every trainer can run a store. A store is the trainer's cards (`member_cards`) with stock, asking prices and
statuses, grouped into binders, sold through an informal flow: the buyer reserves, the seller accepts and shows how
to pay (GCash, Maya, bank), the buyer pays in their own app, the seller marks it paid. The site never handles money.

Code: `src/sellers.rs` (database side, in the library, with tests) and `src/store.rs` (pages and JSON routes, in the
binary). Pages: `templates/store_*.html` (the seller panel), `store.html` (public storefront), `stores.html`
(directory), `admin_sellers.html`. Browser: `static/store.js`, `static/storefront.js`.

All JSON routes answer `{ "ok": true, ... }` or an error `{ "error": "plain words" }` with a 4xx status. Signed-out
callers get 401. Every `/me/…` route only touches the signed-in trainer's own data. Requests must come from the site
(same-origin check), with the `bbv_trainer` cookie.

## Who can sell

Trainers are collectors by default: a profile and a binder. An admin switches selling on per trainer (Admin › Trainers
› Store: **Seller** / **Collector**, `POST /admin/trainers/{id}` with `{ "action": "seller", "seller": true }`).
Only sellers get the store panel, a public `/store/<name>`, Reserve/Offer buttons and a place in `/stores`; a trainer
whose store is off sees `/me/store` explain that, and still buys from stores like anyone else. Everyone signs in at `/login`: a trainer
with their name and password, admins included: an admin trainer's admin powers come from their role, checked on every
request through their own session, so demoting or blocking them ends the admin powers of that session at once. It
does not end access for someone who knows `ADMIN_TOKEN` or still holds an owner cookie from before (those are the
owner's key, checked before any role or block); every admin who used the old `name/<ADMIN_TOKEN>` way knows the key.
Rotating `ADMIN_TOKEN` (`fly secrets set`, plus the same value in `ops/env` on the VM) is what ends that. (The old
`name/<ADMIN_TOKEN>` way, say `selhio/…`, still opens only that admin's own trainer session, never the owner's
cookie; it goes away once every admin has their own password.) An admin runs their own trainer like any other, except lock, block, removal and role:
those are for another admin.

## The SRP

Every card a seller or the owner prices shows an **SRP**: the suggested retail price in pesos for its condition,
a share of today's Near Mint market price (`pricing.rs`, the owner's shares from Admin › Dashboard › Condition
prices). It is only a guide: the price is always the seller's (or the owner's), and nothing changes it by itself.
Graded and sealed cards get no SRP, only the raw Near Mint price for reference. Sellers' market prices refresh
nightly; the owner's cards remember theirs in `products.market_php`.

## What a card is: Highlight, For sale, Wishlist

Every card in a trainer's binder has a `kind`: `own` (a Highlight, shown off on the trainer page), `sale` (in the
store, with stock and a price; only a seller can pick this) or `wish` (a card the trainer is after). A seller's
`Add cards` puts cards straight on sale; `/me` adds Highlights. Change it per card with `POST /me/cards/{id}`
`{ action: "kind", kind }` or `POST /me/listings/{id}` `{ kind }`, or many at once through the listings bulk
sheet (`{ action: "edit", kind }`). A seller's listings page has an **In stock / Out of stock** switch: it flips the
status between `Active` and `Out of stock` without touching the copy count.

## Listing statuses

`Active` (on sale) → `Reserved` (every copy spoken for by open reservations) → `Sold` (stock is 0). Also
`Out of stock` (the seller's own switch), `Hidden` (the seller took it off the shelf) and `Expired` (its end date
passed; the owner is told and can renew).

- `Out of stock`, `Hidden` and `Expired` belong to the seller or the clock: changing the copy count never moves a
  listing out of them.
- Otherwise the status follows stock (`sellers::refresh_reserved`): 0 copies is `Sold`, every copy held by open
  reservations is `Reserved`, anything else is `Active`.
- Asking for `Active` with 0 copies sets 1 copy first. Asking for `Sold` sets 0 copies. `Reserved` can't be picked by
  hand; it is always worked out from real reservations.
- Renew and "Change end date" only move the date, except that an `Expired` listing comes back to life.
- Switching a card into For sale brings `Expired` and `Sold` back with at least 1 copy and leaves `Out of stock` and
  `Hidden` alone. A card already for sale is untouched.
- Removing a listing, or moving it out of For sale, cancels its open reservations and declines its open offers
  (`sellers::close_open_orders`), and each buyer is told once. A member who deletes their account has their own
  orders cancelled the same way (`sellers::close_member_orders`).
- Nobody can buy from a store that is blocked or whose profile is waiting for approval: "This store is closed."
  Accepting an offer re-checks stock, so a card can't be sold twice.

The same card can be listed more than once (say a Near Mint and a Lightly Played copy): the second listing's
`card_id` is `tcgdex:base1-58#2`. `sellers::catalogue_key` strips the suffix wherever the catalogue id matters.

## The seller panel (signed in)

| Route | What |
|---|---|
| `GET /me/store` | Dashboard: on sale, stock value, sold, views (7/30 days, chart by day), followers, rating, to-do (open orders, questions, expiring), sales this week/month/ever, most looked at, popular binders, recent activity |
| `GET /me/listings?q=&status=&binder=&sort=&id=` | Listings, filtered. `sort`: `newest`, `price-desc`, `price-asc`, `views`, `stock`, `title`, or the seller's order. `id` scrolls to a card |
| `POST /me/listings/{id}` | Change one listing. JSON, any of: `price` (₱, 0 = follow the market), `cond`, `qty`, `status`, `finish`, `notes`, `binder_id` (null = none), `expires_days` (0 = no end), `title`/`set`/`number` (own-photo cards only), `flag` (ask the vault to check a custom card). Returns `card` |
| `POST /me/listings/bulk` | `{ ids, action }`: `edit` (same fields as above), `reprice` (`percent`, −90…500), `delete`. Returns `n` |
| `POST /me/listings/{id}/renew` | `{ days }` (default 30, 0 = no end date). Puts an expired card back on sale |
| `POST /me/listings/move` | `{ ids, binder_id }` moves cards into a binder (null = out) |
| `POST /me/cards` | Add a catalogue card (`{ id, image, image_large, en }`), as before; now allows a second listing of the same card |
| `POST /me/cards/upload` | Add a card from the seller's own photo (multipart: `photo`, `name`, `set`, `cond`, `value`) |
| `GET /me/search?q=&lang=` | The card catalogue, as before |
| `GET /me/binders` | Binders page |
| `POST /me/binders` | `{ title }` makes one |
| `POST /me/binders/{id}/edit` | `{ title?, description?, visibility? }` (`Public`, `Unlisted`, `Private`) |
| `POST /me/binders/{id}/order` | `{ ids }`: the order of cards inside the binder |
| `POST /me/binders/{id}/cover`, `/rename`, `/delete` | As before |
| `GET /me/orders?tab=selling\|questions\|sales\|buying` | Orders page |
| `POST /me/orders/reservations/{id}` | Seller: `{ status: "Confirmed" \| "Paid" \| "Declined", paid_via }`. Buyer: `{ status: "Cancelled" }`. `Paid` creates the sale, lowers stock |
| `POST /me/orders/offers/{id}` | Seller: `{ status: "Accepted" \| "Declined" \| "Countered", counter }`. Buyer: `{ status: "accept" \| "withdraw" }` on a counter |
| `POST /me/orders/questions/{id}/reply` | `{ text }` |
| `POST /me/reviews` | Buyer: `{ transaction_id, rating (1–5), text }`. Only the buyer of a finished sale, once; never the seller |
| `POST /me/reviews/{id}/reply` | Seller: `{ text }` |
| `GET /me/settings` | Store settings page |
| `POST /me/settings` | Multipart: `store_name`, `cover` (image), `drop_cover`, `status` (`Active`/`Away`/`Vacation`), `location`, `contact_info`, `contact_public`, `fb_url`, `fb_group_url`, `ig_url`, `tiktok_url`, `shopee_url`, `links` (lines of `Label \| https://…`), `pay_method` (repeatable; send `pay_methods_sent=1` to clear), `pay_info`, `email`, `notify_views`. Links must be `https://` |
| `POST /me/settings/slug` | `{ slug }`: 3–30 of `a-z 0-9 -`, unique whatever the case, not a name the site uses |
| `GET /me/notifications?page=` | Notifications (30 a page) and preferences. Opening it marks the page read |
| `POST /me/notifications/prefs` | `{ event, site, email, push }`. Events: `my_order` (the buyer's news: accepted, declined, countered, replied, paid, ran out, off sale), `follow`, `view`, `inquiry`, `offer`, `reservation`, `order`, `sale`, `price`, `expiry`, `status`, `announcement`. `inquiry`, `offer` and `reservation` are the seller's side only; a reservation that runs out tells both the buyer and the seller. Also `{ address }` saves the email address and returns `{ ok, email }` |
| `GET /api/notifications`, `POST /api/notifications/read` | The bell, as before |
| `POST /api/notifications/push` | Saves a Web Push subscription (`{ endpoint, keys: { p256dh, auth } }`, or `remove: true`) |
| `POST /api/cards/{id}/save` | Save a card for later, or unsave. Returns `saved` |

## The public store

| Route | What |
|---|---|
| `GET /stores` (also `/sellers`) | Stores with cards on sale, busiest first |
| `GET /store/{slug}` | A store: cover, name, status banner, stats from real data, links, ways to pay, binders, cards on sale, reviews. Blocked and not-yet-approved stores are seen only by their owner and the vault's staff |
| `GET /store/{slug}/binders/{binder}` | One binder (`Private` only for the owner) |
| `POST /store/{slug}/follow` | Follow or unfollow. Returns `following`, `followers` |
| `POST /store/{slug}/reserve` | `{ card_id, qty, message }`. Refused for the seller's own cards, on vacation, when not enough copies are free, or with an open reservation already |
| `POST /store/{slug}/offer` | `{ card_id, qty, amount, message }` |
| `POST /store/{slug}/ask` | `{ card_id?, message }` |
| `GET /trainers/{id}/cards/{card}` | A card (the floating window). Counts one look per viewer per day, never the owner's |

Reservations nobody answers, and accepted ones nobody pays, expire after 3 days (`sellers::RESERVATION_DAYS`)
and free the copies again.

## The cart

Anyone can add a card to their cart from the vault (product page, browse tiles, home rails) or from a seller's
store or trainer page. The cart lives in the browser (`localStorage` `bbv:cart`, `static/cart.js`) until it is sent.
There is no checkout and no payment: sending the cart places orders, and the seller or the vault answers them.

| Route | What |
|---|---|
| `GET /cart/lines?v=1,2&c=3` | Fresh title, price, picture and availability for vault products (`v`) and sellers' cards (`c`) |
| `POST /cart/checkout` | `{ items: [{ kind: "vault" \| "card", id, qty }], message }` → `{ placed, failed }`. The vault's lines become one `vault_orders` row; each seller's card becomes a reservation (`sellers::reserve`). 401 when signed out. |
| `GET /me/orders?tab=buying` | The buyer's reservations, offers and vault orders, with how to pay once accepted |
| `POST /me/orders/vault/{id}` | `{ status: "Cancelled" }` while the order is still open |

Open vault orders (`Pending`, `Confirmed`) hold the vault's copies (`db::vault_left`), oldest first, so two buyers
can't both get the last copy; Accept and Mark paid re-check stock. One order holds at most 30 lines and 60 copies, a
buyer has at most 3 open, and a `Pending` order the vault doesn't answer in 7 days lapses (`expire_due`) and the
buyer is told. Deleting a vault copy moves open orders onto another copy of the same card (`db::repoint_open_orders`).

Vault orders go `Pending` → `Confirmed` (the buyer now sees how to pay the vault) → `Paid` (the copies come off the
shelf through `db::mark_sold`), or `Declined` / `Cancelled`. Admin trainers get a notification (event `order`).

## Admin

| Route | What |
|---|---|
| `GET /admin/orders` | Orders for the vault's own cards, pending first; the "How buyers pay the vault" form |
| `POST /admin/orders/{id}` | `{ status: "Confirmed" \| "Paid" \| "Declined", paid_via }` |
| `POST /admin/orders/pay` | `{ methods: [], info }` → settings `vault_pay_methods`, `vault_pay_info` |
| `GET /admin/sellers` | Announcement form, custom cards flagged for checking, latest reviews, stores on sale |
| `POST /admin/sellers/announce` | `{ message, link }` to every trainer (owner and full admins) |
| `POST /admin/sellers/reviews/{id}` | `{ hidden }` |
| `POST /admin/sellers/flag/{id}/clear` | Marks a flagged custom card checked |

Settings the owner can add in the admin settings table: `seller_max_cards` (how many cards a store may hold; default 500).

## Email, push, housekeeping

- A background task (`store::start_housekeeping`) runs every minute: listings past their end date come off sale,
  stale reservations expire, and mail in the outbox goes out.
- **Email** needs `RESEND_API_KEY` and `MAIL_FROM` (like `Big Boys Vault <no-reply@bigboysvault.com>`, a domain
  verified at resend.com). Without a key, mail is logged once and dropped. Only events the trainer switched on, and
  only to a trainer who typed an email address in Store settings.
- **Push** (call and message alerts on phones and computers, even with the site closed) is `src/push.rs`: RFC 8291
  encryption and RFC 8292 (VAPID) signing, posted straight to the browser makers' push services (an allow-list:
  Google FCM, Mozilla, Apple, Microsoft WNS; nothing else is ever called). The site's key pair is made on the first
  start and kept in `settings` (`vapid_private`); `VAPID_PUBLIC_KEY` + `VAPID_PRIVATE_KEY` override it. A device turns
  alerts on from a tap (`static/alerts.js`: the cards marked `data-alerts` on the Messages page, the inbox drop-down
  and notification settings), which registers the service worker at `/sw.js` (scope `/`, no fetch handler) and saves
  the subscription (`POST /api/notifications/push`; one browser belongs to one member, the last to sign up on it;
  turning alerts on also turns on the "message" push preference unless it was set before). Signing out, or another
  person signing in on that browser, forgets it (`POST /api/notifications/push/forget {endpoint}`, no sign-in needed:
  only the browser knows its address). What gets pushed: a call rings the callee's devices every 7 seconds while it
  rings, then one last push says how it ended (missed, answered, declined), with the Topic `call<thread>` so a phone
  that was out of reach gets only the last; private messages for members with message alerts on; and every other
  notification event whose Push box is ticked, through the `push_outbox` queue (sent every 3 seconds). The service
  worker always shows an alert (Safari takes alerts away from a site that skips one; Chrome allows skipping only while
  a page of the site is on screen), asks `GET /dm/{thread}/call-state?call=` before ringing so a late push never rings
  for a call that is over, tells the caller "it's ringing here" (`type: "ringing"`), and opens or brings forward the
  chat for Answer (`?call=…&answer=1`, or a message to an open page). 404 and 410 from a push service delete the
  subscription. Limits worth knowing: iPhone and iPad need the site added to the Home Screen first; Android may hold
  alerts while the phone sleeps unless Chrome's battery use is set to Unrestricted; a ring is the phone's normal
  notification sound, repeated, not a ringtone.
- **Scan a card** uses the browser's on-device model (`LanguageModel` with image input, Chrome desktop) to read the
  name and number off a photo, then searches the catalogue and lets the seller pick. Browsers without it ask the
  seller to type the name under the photo. Nothing is ever added without a tap.

## Tables added (all `CREATE … IF NOT EXISTS` / `ADD COLUMN`, never dropped)

`members`: `slug`, `cover_photo`, `location`, `contact_info`, `contact_public`, `status`, `email`, `store_name`,
`fb_url`, `fb_group_url`, `ig_url`, `tiktok_url`, `shopee_url`, `links`, `pay_info`, `pay_methods`, `notify_views`.
`member_cards`: `binder_id`, `status`, `expires_at`, `is_custom`, `qty`, `finish`, `notes`, `sold`, `view_n`,
`save_n`, `updated_at`, `review_flag`. `member_binders`: `description`, `visibility`.
New: `reservations`, `offers`, `inquiries`, `transactions`, `reviews`, `store_follows`, `listing_views`,
`listing_saves`, `notifications`, `notify_prefs`, `push_subscriptions`, `card_events`, `outbox`, `condition_shares`.

## Testing by hand

1. `cargo run`, open http://localhost:3000/join and make two trainers (approve them on Admin › Trainers, or they
   see only their own store).
2. As the seller: My trainer › **Open my store** › Listings › **+ Add cards**; set stock, prices, a binder; Store
   settings: name, address, ways to pay, how to pay.
3. As the buyer: `/stores` › the store › **Reserve** (or **Offer**, **Ask a question**, **Follow**, ♥).
4. As the seller: Orders › **Accept** › the buyer's Orders › My buying now shows how to pay › seller **Mark paid**.
5. As the buyer: Orders › My buying › Cards I bought › **Leave a review**. It shows on the store.
