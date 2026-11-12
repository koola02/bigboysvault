# Running Big Boys Vault: speed, the CDN, and the VM's jobs

The site runs on one small Fly machine (256 MB, Singapore). It stays fast because pictures are small and cached,
the CDN answers most requests, and the heavy chores run elsewhere: on a Google VM, at night.

## 1. Pictures

- Card art from the catalogue (TCGdex, pokemontcg.io) is already WebP, sized by `db::cdn(src, width)`.
- The owner's and sellers' own photos are saved twice: `_l.jpg` (1600 px, the card page) and `_s.jpg` (480 px, grids).
- **WebP/AVIF**: the VM's `ops/images.sh` makes a `.webp` (and `.avif` for thumbnails) beside every JPEG and pushes
  them to `/data/uploads`. `/uploads/x.jpg` then answers with the AVIF or WebP when the browser's `Accept` says it
  takes them, and the JPEG otherwise. Nothing in the pages changes.
- Because one address can then send different formats, a picture that has WebP/AVIF copies is answered with
  `Vary: Accept` for browsers **and** `Cloudflare-CDN-Cache-Control: no-store`, so Cloudflare never keeps one format
  and hands it to everyone (the free plan ignores `Vary: Accept`). Browsers still keep each picture for a day.
  A picture without copies is always the same JPEG, so Cloudflare may keep it like any other file.
- A copy only counts while its original is there: a deleted photo stays deleted even if its `.webp` is left behind.
- Grids load pictures lazily (`loading="lazy"`); the first screen doesn't.

## 2. Cache headers (what the browser and the CDN keep)

| What | Header | Why |
|---|---|---|
| `/static/*.js?v=N`, `*.css?v=N` | `public, max-age=31536000, immutable` | Versioned: bump `?v=` when a file changes (docs/HANDOFF.md §5) |
| `/static/*` without `?v=` | `no-cache` | Revalidates each load (304) |
| `/uploads/*` | `public, max-age=86400, stale-while-revalidate=604800` + `Vary: Accept`; plus `CDN-Cache-Control` and `Cloudflare-CDN-Cache-Control: no-store` when the picture has WebP/AVIF copies | Browsers keep a picture a day; Cloudflare never keeps one that changes format by browser |
| `/cards.tsv` | `public, max-age=0, s-maxage=30, stale-while-revalidate=120` + ETag | Browsers revalidate (cheap 304); the CDN keeps it 30 s |
| Pages | as before (`cached()` keeps HTML 5 min in memory; `app.forget()` on every write) | |
| `/me/search` (catalogue) | kept 24 h in memory (`App.catalog`), warmed nightly | The card service is slow |

## 3. Cloudflare in front (free)

1. Add `bigboysvault.com` to Cloudflare; move the nameservers at Namecheap to the two Cloudflare gives you.
2. DNS: keep the records Fly gave you (`A`/`AAAA` for the apex, `CNAME www → bigboysvault.fly.dev`), **proxied**
   (orange cloud).
3. SSL/TLS: **Full (strict)**. Fly already has a certificate for the domain (`fly certs list -a bigboysvault`);
   if it ever fails to renew behind the proxy, switch the record to DNS-only for an hour, `fly certs check`, then
   proxy again.
4. Rules › Cache Rules: "Cache everything" for `/static/*` only, respecting origin headers. **No rule for
   `/uploads/*`**: it isn't needed (Cloudflare already caches `.jpg` files by default), and pictures with WebP/AVIF
   copies say `Cloudflare-CDN-Cache-Control: no-store` themselves (see §1). If an older `/uploads/*` rule exists,
   delete it, or at least make sure it does not set "Edge TTL: ignore cache-control header" (that would undo the
   `no-store`). Then Caching › Purge Everything once. Leave pages on the default (HTML isn't cached unless told).
5. Speed: Brotli on, Early Hints on, Rocket Loader **off** (it breaks the floating window), Auto Minify off (files
   are versioned and small). Don't turn on Polish or "Vary for Images" (paid, and not needed).
6. The site reads the visitor's address from `CF-Connecting-IP` only when the request really came through
   Cloudflare (Fly's `Fly-Client-IP` is one of Cloudflare's addresses); otherwise it uses `Fly-Client-IP`. Rate
   limits and view counting work the same either way, so nothing else changes.
7. The VM's jobs talk to the site's own address `https://bigboysvault.fly.dev` (`ORIGIN` in `ops/env`) for big
   downloads, so Cloudflare's 100-second limit never cuts a backup short.
8. Caching › Configuration › Browser Cache TTL: **Respect Existing Headers**, so Cloudflare doesn't turn the pages'
   `no-cache` into `max-age=14400`.
9. The bare domain: the app itself sends `https://bigboysvault.com/...` to `https://www.bigboysvault.com/...` with a
   301, keeping the path and query, so no Cloudflare redirect rule is needed (one with the same target is harmless).
10. HSTS: the app sends `Strict-Transport-Security: max-age=15552000` (6 months, no includeSubDomains, no preload).
    Don't also switch HSTS on in Cloudflare with other values. `/favicon.ico` is served by the app.

## 4. The VM (Google Compute Engine, the small one)

Jobs live in `ops/` of a checkout at `~/bigboysvault`. Once, in the VM's SSH window:
`cd ~/bigboysvault && git pull -q --ff-only origin master && bash ops/first-run.sh`. It asks for the admin password
(and, if you like, a Telegram bot), makes a backup bucket, installs the tools and cron, and runs the first backup.
`ops/env` holds the settings (copy of `ops/env.example`; never in the repo).

**Upgrading the VM that is already running (once).** A VM set up before `update.sh` existed has only the uptime,
backup, prices and images cron lines, so nothing pulls new scripts onto it. In the VM's SSH window, once:
`cd ~/bigboysvault && git config core.fileMode false && git pull -q --ff-only origin master && bash ops/install-cron.sh && crontab -l | grep bigboysvault/ops`
The last command should list five lines, including `15 2 * * *   bash …/update.sh`. From then on `update.sh`
pulls `ops/` every night and reinstalls the cron lines when `crontab.example` changes. Running it again is harmless
(`install-cron.sh` says `unchanged`). `bash ops/setup-vm.sh` does the same and more.

The VM's clock is set to Asia/Manila, so the times below are Philippine time.

| Job | When | What |
|---|---|---|
| `uptime.sh` | every 5 min | `GET /healthz`, looked at again after 20 seconds before a miss counts. A miss is then checked at the server's own Fly address and against an outside site: the server answering nowhere two runs in a row → "DOWN" (and what Fly says goes into the log); only the Cloudflare address failing three runs in a row → "can't be reached through Cloudflare, the server is fine"; the VM itself offline → logged, nobody is rung. One message when it's fine again (with how long it lasted), which also marks the warning in the bell as fixed |
| `update.sh` | 02:15 | `git pull --ff-only` of this checkout, so fixes to `ops/` reach the jobs by themselves (after the one-time upgrade above on an older VM); reinstalls the cron lines when `crontab.example` changed; keeps every log under 5 MB |
| `backup.sh` | 02:30 | `GET /admin/backup/db` (a `VACUUM INTO` snapshot, checked with `PRAGMA integrity_check`) → `~/bbv/backups/db/data-DATE.db(.gz)`; new pictures from `GET /admin/backup/uploads?since=` → `~/bbv/backups/uploads/` (three tries each; the "since" marker only moves when every picture arrived; every Sunday all pictures are checked again); then `gcloud storage` to the bucket. 30 days of `.db` and 90 days of `.db.gz` kept locally. Warns when the VM disk is 85% full |
| `prices.sh` | 03:00 | `POST /admin/prices/refresh?limit=50` in a loop until nothing is stale: today's Near Mint market price for every catalogue card (`market_php`, what the **SRP** on My cards is worked out from), the suggested price for cards whose price follows the market (graded, sealed and hand-set prices untouched), and the sellers' cards; sellers far off the market are told once a week per card. Then `POST /admin/catalog/warm`. The site also fills missing market prices by itself, 40 cards every ten minutes, and set release dates (`card_sets`, for "sort by set") soon after boot and every six hours (`POST /admin/sets/refresh` does it on demand) |
| `images.sh` | 03:30 | WebP/AVIF beside every backed-up JPEG (in `~/bbv/converted`). Everything not yet on the site's list (`~/bbv/pushed.txt`) is first asked for on the site (files it already has just go on the list), and the rest goes up in batches of 300: one tar file per batch (`fly ssh sftp put`), unpacked on the site, then every file is checked at `$ORIGIN/uploads/…`; only files that answer are added to the list. A failed night is simply tried again the next night. Before each batch it checks the site's disk, and stops (with an alert) when that batch would leave under 150 MB free; batches already pushed stay done |
| `checks.sh` | every 15 min | Opens the pages a buyer uses (home, All cards, Stores, log-in, the cart, `/healthz`, and one card and one store taken from the sitemap), each from the US. Each must answer 200 with the right words on it, in under 3 seconds (`CHECK_SLOW_S`); a miss is tried once more after 20 seconds before it alerts |
| `photos.sh` | every 30 min | Trainers' own card photos (`member_cards.photo_clean=0`, from `GET /admin/photos/pending`): `ops/cardcrop.py` (OpenCV) finds the card's four corners, straightens it and crops off the table, and the result goes back with `POST /admin/photos/{id}`. The original is kept (`photo_original`) and the trainer can put it back with "Use my own photo". A photo with no clear card, or already straight, is left as it is (`?skip=1`) |
| `drill.sh` | 04:15 | The restore drill: last night's backup is opened (must be under 30 hours old), checked (`integrity_check`, the main tables, counts close to `/healthz`), every picture the site lists is looked for in the backup, and, with Docker, the site's live image is started on that copy at 127.0.0.1:18080 and four pages must load. Proves the backup would actually bring the site back |
| `similar.sh` | 04:45 | "More like this" and Ask the Vault's "cards like …": `ops/similar.py` describes every card (name, type, evolution, attacks, rarity, artist, set, from TCGdex, cached in `~/bbv/tcgdex`), turns each description into numbers with a small language model on the VM's CPU (all-MiniLM-L6-v2), and sends each card's 24 closest available cards to `POST /admin/similar`. The site falls back to its own name/set/price match when a card has no picks yet |
| `deploy.sh` | by hand | fetch, test, back up, `fly deploy --remote-only`, check `/healthz` (builds from `~/bbv/deploy-src`, so the jobs' own scripts never change branch) |
| `staging.sh` | by hand | last night's copy in the site's Docker image, on the VM only (127.0.0.1:8080), with a random admin password, for 4 hours. See below |
| `restore-pictures.sh` | by hand | puts the backed-up pictures back on the site (see Restore) |
| `install-cron.sh` | by `setup-vm.sh` and `update.sh` | writes the cron lines from `crontab.example` |
| `setup-extras.sh` | once, by hand | the tools for `photos.sh`, `drill.sh`, `similar.sh` and `staging.sh`: Docker, and Python with OpenCV and the language model in `~/bbv/venv` (about 1.5 GB). Without them those jobs quietly do nothing (the drill still checks the backup file and pictures) |

### When something goes wrong

- **Every job tells the site how it went** (`POST /admin/ops/report`). The admin dashboard shows a "VM jobs" panel:
  when each job last ran, a tick or a warning, and a note. A failure also rings the admins' bell.
- The site itself notices a quiet VM: no good backup or drill report for 30 hours (pictures and
  similar cards: 50 hours; page checks: 1 hour) rings the bell too, so a stopped VM or a broken cron is still noticed.
- Any job that stops with an error raises an alert on the way out (`ops/lib.sh`), with the line it stopped at.
- Telegram (`TELEGRAM_BOT_TOKEN` + `TELEGRAM_CHAT_ID`) or a webhook (`ALERT_WEBHOOK`) in `ops/env` also sends each
  alert to a phone. Needed for "the site is DOWN", which can't reach a site that is down.
- Logs: `~/bbv/logs/<job>.log` on the VM (`tail -50 ~/bbv/logs/backup.log`). The push details are in
  `~/bbv/logs/images-push.log`.

### Staging (try a change on last night's data)

On the VM: `bash ~/bigboysvault/ops/staging.sh [branch]`. It builds the change on Fly's builders (the VM is too
small to compile it) and runs it on last night's copy. To look at it from a phone or any browser: open Google Cloud
Shell (the `>_` button in the Cloud console), run `gcloud compute ssh <vm-name> --zone <zone> -- -N -L 8080:localhost:8080`,
then Web Preview → port 8080. From your own computer with gcloud installed, the same command and http://localhost:8080. The admin password is in
`~/bbv/staging/admin-password.txt`. Never open port 8080 in the VM's firewall: the copy holds members' real data.
It stops by itself after 4 hours (`STAGING_HOURS`), or now with `docker rm -f bbv-staging`.

Secrets stay in `ops/env` on the VM (never in the repo). The admin routes answer only to the owner's token or a
signed-in full admin, and only over HTTPS.

### Restore

Never copy a file over `/data/data.db` while the site runs: the database uses WAL (`data.db-wal`), and an old
WAL file next to a new database breaks it. The site swaps a database in safely itself, when it starts.

**1. A whole bad day (first choice): Fly's own volume snapshots.** Fly keeps daily snapshots of the volume (5 days).
```
fly volumes list -a bigboysvault                       # the volume id (vol_…) and the machine using it
fly volumes snapshots list <vol-id>                    # pick a snapshot from before the problem
fly volumes create data --snapshot-id <snapshot-id> -r sin -a bigboysvault
fly machine clone <machine-id> --attach-volume <new-vol-id>:/data -a bigboysvault
# check https://bigboysvault.fly.dev/healthz and the site, then remove the old machine:
fly machine destroy <old-machine-id> --force -a bigboysvault
```

**2. The database from the VM's backups.** On the VM (flyctl is in `~/.fly/bin`):
```
ls ~/bbv/backups/db/                                   # pick data-DATE.db (or gunzip data-DATE.db.gz first)
fly ssh sftp put -a bigboysvault ~/bbv/backups/db/data-DATE.db /data/restore.db.part
fly ssh console -a bigboysvault -C "mv /data/restore.db.part /data/restore.db"   # only once the upload finished
fly apps restart bigboysvault
curl -s https://bigboysvault.fly.dev/healthz           # {"ok":true,…}
```
Upload to `restore.db.part` and rename it afterwards, so an upload that breaks off never leaves a half file called
`restore.db`. At start the site checks `restore.db`: it must not be empty, must pass `PRAGMA integrity_check`, and
must hold the site's tables (`products`, `members`). If it is good, the old database is kept as
`data.db.before-restore-<time>` (with its `-wal`/`-shm`) and `restore.db` becomes `data.db`. If it is bad, it is
renamed `restore.db.bad-<time>` and the site keeps its current database. `fly logs -a bigboysvault` says which.
Delete the `before-restore` files once all is well: `fly ssh console -a bigboysvault -C "ls -la /data"`, then
`fly ssh console -a bigboysvault -C "rm /data/data.db.before-restore-<time>"`.

**3. Pictures.** On the VM: `bash ~/bigboysvault/ops/restore-pictures.sh`. It sends the backed-up pictures in parts
of about 100 MB (a tar file each, unpacked on the site, then deleted), skips pictures the site still has, and stops
if the site's disk is too full. Tonight's `images.sh` then puts the WebP/AVIF copies back.

**4. A brand-new app.** Follow `DEPLOY.md` (Route B), set `FLY_APP`/`SITE` in `ops/env` if the name changed, then
steps 2 and 3.

Try a restore now and then on staging (`ops/staging.sh`) so it's familiar before it's needed.

## 4b. Ask the Vault with Google Gemini

`src/vault_ai.rs`. When the Fly secret `GEMINI_API_KEY` is set, Gemini answers Ask questions: it searches the vault
(`search_cards`), opens a card (`card_details`: price, stock, market price, trend), finds look-alikes
(`similar_cards`) and searches the trainers' stores (`store_listings`), all read-only. It may only quote what those
return; the cards it recommends come back as tiles built from the real products. Without a key, or when anything goes
wrong (timeout, Google's free quota used up: a 429 pauses it for a minute), Ask answers the old way (`src/ask.rs`).

- Key: aistudio.google.com/apikey → Create API key, then `fly secrets set GEMINI_API_KEY=... -a bigboysvault`.
- `GEMINI_MODEL` (default `gemini-3.6-flash`), `GEMINI_DAILY_MAX` (answers per Philippine day, default 900; each
  visitor at most 30 per 15 minutes).
- Status: `GET /admin/ask/status` (owner): on/off, model, answers today, last success, last problem.
- The free tier lets Google use the questions to improve its products; the Ask screen says that questions go to Google.

## 4c. Private messages (Messenger style)

`src/dm.rs` (database), `src/messages.rs` (web), `static/dm.js` (the chat window). One conversation per pair of
people; "the vault" is one side too (id 0), answered by every full admin. A "Message" button sits beside every
add-to-cart button; it opens the chat with that card attached to the first message. Text, photos (re-made from the
pixels, like every upload), voice notes (WebM/Ogg/MP4/MP3, checked by their first bytes, 3 MB) and voice calls.

- Live: each open page keeps one waiting request (`GET /dm/poll`, 25 s), woken the moment a message, a "Seen" or a
  call signal arrives. Calls are WebRTC, browser to browser; `GET /dm/ice` hands out Cloudflare's relay (TURN) when the
  Fly secrets `CF_TURN_KEY_ID` and `CF_TURN_API_TOKEN` are set (credentials for a day, made again every 12 hours),
  otherwise public STUN only (some mobile networks then can't connect).
- Notifications: the other side gets one line per conversation that keeps count ("Juan · 3 new messages: …").
  Every message in a conversation between two trainers also notifies the admins who aren't in it (`dm_watch`).
- Admin › Messages (`/admin/messages`): every conversation. Reading there never marks anything seen. The vault's own
  conversations open normally, so an admin can answer and call as the vault.
- Media counts toward the photo space cap (`PHOTO_SPACE_MB`) and lives in `uploads/` under unguessable names
  (`dm<thread>-…`), so the nightly backup keeps it.

## 4d. The new look (Facebook-style redesign, in progress)

The plan is in `docs/REDESIGN.md`. While it is built the site has two looks. `src/look.rs` decides per request:
the new look is on when the visitor chose it (cookie `bbv_look=new`, set from `/admin/look` by staff) or the owner
switched everyone (`site_look` setting, `POST /admin/look/all`), **and** the page is in `look::READY`. Templates ask
`crate::look::is_new()`; `base.html` then loads `fb.css` + `fb_top.html`/`fb_bottom.html` + `fb-shell.js` instead of
`app.css`/`modern.css`/`vault.css` + today's header + `vault-core.js`. `widgets.css` (messages, card camera, the
admin-buttons switch) is loaded by both. `cached()` keeps the looks apart. To convert a page: restyle it in `fb.css`,
add its route to `READY`. `/admin/look` is the style guide: every component in the frame, plus the switches.

## 5. Catalogue importer (later)

Adding stock already pulls names, sets and art from TCGdex on demand, and the nightly warm-up keeps the most
searched names instant. A full local copy of the catalogue (every set, searchable offline) is the next step if the
service's slowness or limits ever get in the way: a nightly `ops/catalog.sh` filling a `catalog_cards` table.
