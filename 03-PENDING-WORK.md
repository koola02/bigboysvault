# Pending work, in order

Live: release v61. Each item lists what's done and what's left. The full task specs for items 2 and 3 are at the end.

## 1. Ask the Vault improvements: built, not deployed
- **In the source (not live):** `src/ask.rs`, `src/api.rs`, `static/vault-talk.js` (and `vault-talk.js?v=5` in `templates/base.html`).
- **What it adds:**
  - Follow-up questions: the server returns `ctx`, and the browser sends it with the next question.
  - Taglish: magkano, meron ba / meron kayo, mura / pinakamura, mahal / pinakamahal, lang, pa.
  - "1st edition", "unlimited", "shadowless", "japanese".
  - Set questions: "how many base set cards do you have".
- **Status:** `cargo test --lib` gives 36 passing. Checked through the API, before the last fixes:
  - "magkano pikachu" gives 49 Pikachu.
  - Then "near mint lang" gives 34 Near Mint Pikachu.
  - "meron ba kayong charizard" and "pinakamura na holo" work.
- **To do:**
  1. Read the diff carefully. This code was written by another AI and needed several repairs: garbled duplicated sections were removed by hand.
  2. Run `tests/askapi.mjs` against a local server.
  3. Try the chat window on a phone, then deploy.

## 2. Logins, email confirmation, roles, and removing the Admin button: not started
The owner's request, parts 2 and 3:
- **Logins:** unified `/login` and `/register` with email and password, and an email confirmation link.
- **Roles:** an admin goes to the admin dashboard after logging in; a seller goes to their own dashboard.
- **Admin button:** remove the public Admin button from the footer. Admins get in only by logging in with an admin account.
- **Seller accounts and profiles:**
  - Seller accounts have email, password hash, email verified and role.
  - Profiles add a name, a slug (the trainer web address), a rating, Facebook and TikTok links, and extra vouch links.
- **Seller dashboard:** sellers create, edit and delete only their own binders and cards, enforced on the server. Each seller has a public page at `/trainer/{slug}`.

Key facts for the design:
- A seller should be a trainer (the `members` table): trainers already have argon2 passwords, sessions, `/me`, cards, roles (admin/moderator) and stars.
- The owner's `ADMIN_TOKEN` sign-in at `/admin/login` must keep working as an emergency way in, but not be linked publicly.
- **Email needs a sending service.** The plan is Resend (free tier): the owner creates an account and verifies the domain, then the key is set with `fly secrets set RESEND_API_KEY=… -a bigboysvault`. Until then, write confirmation links to the server log.
- **Back up the live database before deploying** (migrations add columns to `members`).

Full spec: "Spec A" below.

## 3. Seller binders and a public Sellers page: not started
The owner's request, parts 3 and 4:
- Sellers get their own binders, using the same 3D binder look as the home page (reuse the markup as an include).
- `/sellers` lists every seller. Above each seller's binders is a vouch header: name linking to `/trainer/{slug}`, rating, and Facebook, TikTok and vouch links.

Full spec: "Spec B" below.

## 4. Smaller follow-ups
- **Google Search Console:** the owner was adding the domain (TXT record at Namecheap), then submitting `https://www.bigboysvault.com/sitemap.xml`.
- **Newest Japanese sets:** a few have no English name yet in `JA_EN` (`src/market.rs`): M3 to M6, MC, MF and starter decks. Add them when their official English names are known.
- **Vault AI, paid option:** offered but not approved. A Claude model would answer anything, using the site's own search as tools, with a spending cap. It needs the owner's OK and an API key.
- **Nightly database backups off the server:** not set up. Tigris storage on Fly was suggested and needs the owner's OK, because it's paid.
- **Unlimited pictures for saved cards:** if an Unlimited card's photo file is ever lost, `heal_photos` (admin.rs) restores the 1st Edition picture. Make it rebuild the Unlimited picture instead when the title says "(Unlimited" or "(Shadowless".

---

## Spec A: logins, email, roles, seller profile fields

Extend the accounts of a Pokémon card shop website (Rust, axum 0.8, askama templates, rusqlite/SQLite, plain JS). The site is live, with real accounts in the database: every change must keep existing data working, with migrations that only add columns or tables (see how src/members.rs already adds columns with `ALTER TABLE ... ADD COLUMN` guarded by checks).

What exists today (read the files):
- **Trainers** (`members` table, src/members.rs):
  - They have accounts with a name and an argon2 password, and sessions in `member_sessions`.
  - Their dashboard is /me (templates/me.html), they have their own cards (`member_cards`) and public pages at /trainers/{id}.
  - A role field holds 'admin', 'moderator' or none. Stars are their vouch rating.
  - Sign-up is /join; sign-in is POST /signin (src/community.rs).
- **Admin access** (src/admin.rs `power()`): the owner's ADMIN_TOKEN cookie, OR a signed-in trainer whose role is admin (full) or moderator (limited).
- **The footer** (templates/base.html) has a public "Admin" button linking to /admin.

Build this, as ADDITIONS to the trainer accounts (a seller IS a trainer, so don't create a second kind of account):

1. Email and confirmation:
   - Add columns to members: `email` (unique when not empty, stored lowercase), `email_verified` (0/1), `verify_token_hash`, `verify_sent_at`.
   - Registering or adding an email sends a confirmation link /verify?token=… (one use, valid 48 hours). Store only a SHA-256 hash of the token, using the sha2 crate already in Cargo.toml.
   - Sending email goes through a small `mailer` module (new file src/mailer.rs, registered in src/lib.rs). If the environment variable RESEND_API_KEY is set, send with Resend's HTTP API: POST https://api.resend.com/emails with JSON {from, to, subject, html}, using the reqwest client (MAIL_FROM env, default "Big Boys Vault <no-reply@bigboysvault.com>"). If it isn't set, write the link to the server log with eprintln! and still succeed.
   - Unverified accounts can still sign in. /me shows a clear notice with a "Send the link again" button (rate-limited: once a minute).

2. One place to sign in: /login and /register pages (new templates, extending base.html like templates/join.html does):
   - /register: email, trainer name, password. It creates the trainer exactly the way /join does today, plus the email, and sends the link. Keep /join working: redirect it to /register.
   - /login: email OR trainer name, plus password. Use the same session cookie and rate limits as the existing /signin.
   - After signing in: role admin goes to /admin/dashboard, anyone else to /me.
   - Add "Log in" to the site's top bar for visitors, and "My dashboard" for signed-in trainers (templates/base.html).

3. Remove the public Admin button from the footer in templates/base.html. Admins reach the admin panel by logging in at /login with an admin trainer account. The owner's existing ADMIN_TOKEN sign-in must keep working at its current address (it's the owner's emergency way in), but it must not be linked from any public page.

4. Seller profile fields: add columns to members: `slug` (unique, lowercase letters, digits and hyphens, made from the trainer name and kept unique with -2, -3 …), `fb_url`, `tiktok_url`, `links` (newline-separated extra vouch links).
   - Fill `slug` for existing trainers in the migration.
   - The /me profile form edits fb_url, tiktok_url and links. Accept only https links.
   - Public trainer pages also answer at /trainer/{slug} (the same page as /trainers/{id}; /trainers/{id} keeps working and its canonical becomes the slug address).

Write complete, compiling code in the existing style, with the existing rate limiting and HTML escaping (askama escapes by default). Add unit tests in src/members.rs for:
- slug making and uniqueness;
- the verify token: hash, expiry and single use;
- the email uniqueness check.

---

## Spec B: seller binders and the Sellers page

Give sellers their own binders, and add a public Sellers page, on a live Pokémon card shop website (Rust, axum 0.8, askama, rusqlite, plain JS). A seller is a trainer (the `members` table, src/members.rs). Trainers already have their own cards (`member_cards`), a dashboard at /me and public pages (/trainers/{id}, and /trainer/{slug}). Migrations may only add tables or columns, and existing data must keep working.

1. Seller binders:
   - New table `member_binders`: id, member_id, title, slug (unique per member), cover_card (a member_cards id, nullable), position, created_at.
   - New column `member_cards.binder_id` (nullable).
   - A trainer's existing cards stay where they are, with binder_id NULL, shown as "All my cards".
   - /me gets a "My binders" section to:
     - create, rename, reorder and delete binders (deleting a binder keeps its cards, which go back to no binder);
     - pick a binder's cover;
     - move cards into a binder.
   - Every write checks the signed-in trainer owns the binder and every card it touches, in SQL (WHERE member_id = ?).
   - A trainer can never see, change or delete another trainer's binders, cards or anything under /admin. Return 404, not 403, for someone else's ids.

2. Public binders on the trainer page: /trainer/{slug} (and /trainers/{id}) shows, under the trainer's header, their binders with the SAME binder look as the home page.
   - The home page's binders are the `.bk` 3D binder markup in templates/home.html (the `{% for (c, pk) in b.shelf %}` loop) with static/vault-binders.js and the `.bk`/`.bx` styles in static/vault.css.
   - Reuse that markup: move it into an include template, e.g. templates/binder_ring.html, used by both pages, so it stays identical.
   - Tapping a seller's binder opens a page or float listing that binder's cards: /trainer/{slug}/binders/{binder-slug}.

3. A new public page /sellers, linked in the main navigation:
   - It shows every approved, not-blocked trainer who has at least one binder.
   - Above each seller's binders, a Seller Vouch Header shows:
     - the trainer name, linking to /trainer/{slug};
     - their avatar, stars or rating, and vouch count if available;
     - Facebook and TikTok buttons and their extra vouch links (fb_url, tiktok_url, links), shown only when set.
   - Below the header, the same binder ring as the home page.
   - The page must look right on phones (390px wide) as well as desktop, in the site's existing dark and gold style.

Write complete, compiling code in the existing style. Add the routes in src/main.rs. Add SEO for /sellers the way other public pages set `nav.seo`. Add unit tests in src/members.rs for the ownership checks: one trainer cannot rename, delete or fill another trainer's binder.
