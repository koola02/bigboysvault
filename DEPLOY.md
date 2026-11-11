# Putting Big Boys Vault on the internet

## Before anything is public (5 minutes)
1. **Set a password.** Admin is open by default. Always start with `ADMIN_TOKEN` set to a long random string.
2. **Replace the demo catalogue.** The 2,657 cards, photos and prices came from marcurushop.com. Don't publish someone else's catalogue and photos.
   In `/admin`, tick "Select all", choose "Select all 2,657", then Delete. Add your own cards after.
3. **Back up.** Everything lives in two places: `data.db` and the `uploads/` folder.

## Route A: public link in about 2 minutes (temporary, free)
Keeps running on your PC; the link works while the PC and server are on.
```powershell
winget install Cloudflare.cloudflared
$env:ADMIN_TOKEN = "your-long-secret"
cd marcuru-rs; cargo run --release
# in a second window:
cloudflared tunnel --url http://localhost:3000
```
It prints an `https://something.trycloudflare.com` address. Open it on your phone over mobile data.

## Route B: permanent on Fly.io (about 20 to 30 minutes, roughly $2 to $5 a month)
```powershell
winget install Fly-io.flyctl
fly auth signup            # or: fly auth login
cd marcuru-rs
# edit `app = "selxjames"` in fly.toml to a name nobody else has (fly.toml is already set to bigboysvault)
fly launch --no-deploy --copy-config
fly volumes create data --size 1 --region sin
fly secrets set ADMIN_TOKEN="your-long-secret"
fly deploy
```
Then copy your data up (only if you want to keep what's on this PC). Stop the local server first, so `data.db`
is complete. The site swaps `restore.db` in safely when it restarts; never `put` straight over `/data/data.db`.
```powershell
fly ssh sftp put data.db /data/restore.db.part
fly ssh console -C "mv /data/restore.db.part /data/restore.db"
fly apps restart bigboysvault
# pictures: one tar file (tar is built into Windows 10 and 11), unpacked on the server, then deleted
tar -cf up.tar -C uploads .
fly ssh sftp put up.tar /data/up.tar
fly ssh console -C "mkdir -p /data/uploads"
fly ssh console -C "tar -xf /data/up.tar --no-same-owner -C /data/uploads"
fly ssh console -C "rm /data/up.tar"
```
Custom domain: `fly certs add yourdomain.com`, then add the DNS records it prints.

Keep it at one machine (SQLite is a single-writer database). Fly takes volume snapshots daily, and the Google VM in `docs/OPERATIONS.md` keeps nightly copies of the database and pictures.

## Route C: home PC, permanent address
A named Cloudflare Tunnel plus a domain on Cloudflare gives a fixed address for free, but the PC must stay on.

## Newer browser features

These only run in browsers that have them. Everyone else gets the normal site.

- **Card finder** (`/all`, `/search`): Rust compiled to WebAssembly, from `crates/finder`. The Docker build compiles it. To build it locally, run `sh build-wasm.sh` with wasm-bindgen 0.2.130 and wasm-opt on the PATH. Keep that wasm-bindgen version equal to the one pinned in `crates/finder/Cargo.toml` and the `Dockerfile`.
- **WebMCP** (tools for AI assistants, `static/webmcp.js`): needs a free Chrome origin-trial token until the feature ships. Register `https://www.bigboysvault.com` at https://developer.chrome.com/origintrials/#/register_trial/4163014905550602241 (tick the subdomain option so the bare domain is covered too), then set it with `flyctl secrets set ORIGIN_TRIALS=<token> --app bigboysvault`. Several tokens can be separated by commas. The trial runs out in early 2027, and the feature may change before then.
- **Card page languages** (`src/translate.rs`, `static/card-lang.js`): English, Japanese and Chinese from official name lists plus tables in that file. To cover a new set name, add it to `SETS`. There's no outside translation service, so no keys or limits.
