# Running Big Boys Vault: speed, the CDN, and the VM's jobs

The site runs on one small Fly machine (256 MB, Singapore). It stays fast because pictures are small and cached,
the CDN answers most requests, and the heavy chores run elsewhere: on a Google VM, at night.

## 1. Pictures

- Card art from the catalogue (TCGdex, pokemontcg.io) is already WebP, sized by `db::cdn(src, width)`.
- The owner's and sellers' own photos are saved twice: `_l.jpg` (1600 px, the card page) and `_s.jpg` (480 px, grids).
- **WebP/AVIF**: the VM's `ops/images.sh` makes a `.webp` (and `.avif` for thumbnails) beside every JPEG and pushes
  them to `/data/uploads`. `/uploads/x.jpg` then answers with the AVIF or WebP when the browser's `Accept` says it
  takes them (`Vary: Accept`), and the JPEG otherwise. Nothing in the pages changes.
- Grids load pictures lazily (`loading="lazy"`); the first screen doesn't.

## 2. Cache headers (what the browser and the CDN keep)

| What | Header | Why |
|---|---|---|
| `/static/*.js?v=N`, `*.css?v=N` | `public, max-age=31536000, immutable` | Versioned: bump `?v=` when a file changes (GEMINI.md §5) |
| `/static/*` without `?v=` | `no-cache` | Revalidates each load (304) |
| `/uploads/*` | `public, max-age=86400, stale-while-revalidate=604800` + `Vary: Accept` | A day, then refreshed in the background |
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
4. Rules › Cache Rules: "Cache everything" for `/static/*` and `/uploads/*`, respecting origin headers (the headers
   above do the rest). Leave pages on the default (HTML isn't cached unless told).
5. Speed: Brotli on, Early Hints on, Rocket Loader **off** (it breaks the floating window), Auto Minify off (files
   are versioned and small).
6. The site reads the visitor's address from `CF-Connecting-IP` when it's there (rate limits, view counting), so
   nothing else changes.

## 4. The VM (Google Compute Engine, the small one)

Jobs live in `ops/`. Once: `git clone … ~/bigboysvault && ~/bigboysvault/ops/setup-vm.sh`, fill in `ops/env`.

| Job | When | What |
|---|---|---|
| `uptime.sh` | every 5 min | `GET /healthz`; two misses in a row → Telegram or webhook; one message when it's back |
| `backup.sh` | 02:30 | `GET /admin/backup/db` (a `VACUUM INTO` snapshot, checked with `PRAGMA integrity_check`) → `~/bbv/backups/db/data-DATE.db(.gz)`; new pictures from `GET /admin/backup/uploads?since=` → `~/bbv/backups/uploads/`; then `gcloud storage` to a bucket. 30 days kept locally |
| `prices.sh` | 03:00 | `POST /admin/prices/refresh?limit=50` in a loop until nothing is stale: today's market price for the vault's market-priced cards (suggested for their condition; graded and sealed untouched) and the sellers' cards; sellers far off the market are told once a week per card. Then `POST /admin/catalog/warm` |
| `images.sh` | 03:30 | WebP/AVIF beside new JPEGs, pushed with one `fly ssh sftp shell` session |
| `deploy.sh` | by hand | pull, test, back up, `fly deploy --remote-only`, check `/healthz` |
| `staging.sh` | by hand | last night's copy in the site's Docker image on port 8080 |

Secrets stay in `ops/env` on the VM (never in the repo). The admin routes answer only to the owner's token or a
full admin, and only over HTTPS.

### Restore
1. `fly ssh sftp shell -a bigboysvault`, then `put ~/bbv/backups/db/data-DATE.db /data/data.db` (stop the machine
   first: `fly scale count 0`, restore, `fly scale count 1`), and `put` any missing pictures under `/data/uploads/`.
2. Or start a fresh app from `DEPLOY.md` and `put` the copies there.

## 5. Catalogue importer (later)

Adding stock already pulls names, sets and art from TCGdex on demand, and the nightly warm-up keeps the most
searched names instant. A full local copy of the catalogue (every set, searchable offline) is the next step if the
service's slowness or limits ever get in the way: a nightly `ops/catalog.sh` filling a `catalog_cards` table.
