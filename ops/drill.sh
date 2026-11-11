#!/usr/bin/env bash
# Every night after the backup: proves last night's copy can really be restored, before the day you need it.
#  1. the database copy opens, passes SQLite's own check, has every table, and holds what the live site holds;
#  2. the zipped copy unzips;
#  3. every picture the site lists is in the backup, at the right size;
#  4. with Docker here: the site's own program (the image that is live now) starts on the copy and answers pages.
. "$(dirname "$0")/lib.sh"
one_at_a_time
BK="$DATA_DIR/backups"
LATEST="$(ls -t "$BK"/db/data-*.db 2>/dev/null | head -1 || true)"
[ -n "$LATEST" ] || { alert "No database backup on the VM to test."; exit 1; }
AGE_H=$(( ( $(date +%s) - $(stat -c %Y "$LATEST") ) / 3600 ))
[ "$AGE_H" -le 30 ] || { alert "The newest database backup is $AGE_H hours old: the backup job isn't working."; exit 1; }
WORK="$(mktemp -d)"; cleanup() { docker rm -f bbv-drill >/dev/null 2>&1 || true; rm -rf "$WORK"; }; trap 'rc=$?; cleanup; (exit $rc); on_exit' EXIT
cp "$LATEST" "$WORK/data.db"
if [ -f "$LATEST.gz" ]; then gzip -t "$LATEST.gz" || { alert "The zipped backup $(basename "$LATEST").gz is damaged."; exit 1; }; fi

# 1. The copy against the live site.
LIVE="$(curl -sS --max-time 20 "$SITE/healthz" || true)"
DBNOTE="$(python3 - "$WORK/data.db" "$LIVE" <<'EOF'
import json, sqlite3, sys
c = sqlite3.connect(sys.argv[1])
ok = c.execute("PRAGMA integrity_check").fetchone()[0]
if ok != "ok":
    print("FAIL integrity check: " + ok[:80]); sys.exit()
need = ["products", "members", "member_cards", "collections", "vault_orders", "reservations", "offers", "notifications", "settings"]
have = {r[0] for r in c.execute("SELECT name FROM sqlite_master WHERE type='table'")}
miss = [t for t in need if t not in have]
if miss:
    print("FAIL missing tables: " + ", ".join(miss)); sys.exit()
cards = c.execute("SELECT COUNT(*) FROM products").fetchone()[0]
members = c.execute("SELECT COUNT(*) FROM members").fetchone()[0]
try:
    live = json.loads(sys.argv[2])
except Exception:
    live = {}
lc, lm = live.get("cards"), live.get("members")
# The site changes during the day, so a small difference is normal.
if lc is not None and abs(lc - cards) > max(25, lc // 50):
    print(f"FAIL the copy has {cards} cards, the site {lc}"); sys.exit()
if lm is not None and abs(lm - members) > max(5, lm // 20):
    print(f"FAIL the copy has {members} trainers, the site {lm}"); sys.exit()
print(f"{cards} cards, {members} trainers")
EOF
)"
case "$DBNOTE" in FAIL*) alert "Restore test: ${DBNOTE#FAIL }"; exit 1 ;; esac

# 2. Every picture the site lists is here, whole: the originals in the backup, and the WebP/AVIF copies (made on
# this VM by images.sh, in converted/) next to them. A missing copy is only counted: images.sh can make it again.
LIST="$(admin_curl "$SITE/admin/backup/uploads?since=0" || true)"
PICNOTE="$(printf '%s' "$LIST" | python3 -c '
import json, os, sys
up, conv = sys.argv[1], sys.argv[2]
try:
    files = json.load(sys.stdin)["files"]
except Exception:
    print("FAIL the site did not list its pictures"); sys.exit()
def here(f):
    name = f["path"][len("/uploads/"):]
    for d in ((conv, up) if name.endswith((".webp", ".avif")) else (up,)):
        p = os.path.join(d, name)
        if os.path.isfile(p) and os.path.getsize(p) == f["size"]:
            return True
    return False
orig = [f for f in files if not f["path"].endswith((".webp", ".avif"))]
copies = [f for f in files if f["path"].endswith((".webp", ".avif"))]
missing = [f["path"] for f in orig if not here(f)]
gone = sum(1 for f in copies if not here(f))
if len(missing) > max(5, len(orig) // 100):
    print(f"FAIL {len(missing)} of {len(orig)} pictures are not in the backup, like {missing[0]}"); sys.exit()
note = f"{len(orig) - len(missing)} of {len(orig)} pictures" + (f" ({len(missing)} new since last night)" if missing else "")
if copies:
    note += f", {len(copies) - gone} of {len(copies)} WebP/AVIF copies"
print(note)
' "$BK/uploads" "$DATA_DIR/converted")"
case "$PICNOTE" in FAIL*) alert "Restore test: ${PICNOTE#FAIL }"; exit 1 ;; esac

# 3. The real program on the copy (when Docker is installed: ops/setup-extras.sh).
RUNNOTE="program test skipped (no Docker)"
if command -v docker >/dev/null && [ -n "${FLY_API_TOKEN:-}" ] && command -v fly >/dev/null; then
  IMAGE="$(fly image show -a "${FLY_APP:-bigboysvault}" --json 2>/dev/null | python3 -c 'import json,sys
d = json.load(sys.stdin); d = d[0] if isinstance(d, list) else d
print(d.get("Registry","registry.fly.io") + "/" + d["Repository"] + ":" + d["Tag"])' 2>/dev/null || true)"
  if [ -n "$IMAGE" ] && fly auth docker >/dev/null 2>&1 && docker pull -q "$IMAGE" >/dev/null 2>&1; then
    mkdir -p "$WORK/uploads"
    docker run -d --rm --name bbv-drill -p 127.0.0.1:18080:8080 -v "$WORK:/data" -e ADMIN_TOKEN="$(openssl rand -hex 16)" -e DATABASE=/data/data.db -e UPLOADS=/data/uploads "$IMAGE" >/dev/null
    UP=""; for i in $(seq 1 30); do UP="$(curl -s --max-time 3 http://127.0.0.1:18080/healthz || true)"; [ -n "$UP" ] && break; sleep 2; done
    PAGES=0; for u in / /all /stores /login; do [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 "http://127.0.0.1:18080$u")" = 200 ] && PAGES=$((PAGES + 1)); done
    docker rm -f bbv-drill >/dev/null 2>&1 || true
    if [ -z "$UP" ] || [ "$PAGES" -lt 4 ]; then alert "Restore test: the site's program started on the copy but $((4 - PAGES)) of 4 pages failed."; exit 1; fi
    RUNNOTE="the live program ran on it and answered 4 of 4 pages"
  else
    RUNNOTE="program test skipped (could not get the live image)"
  fi
fi
NOTE="Copy of $(basename "$LATEST" .db | sed 's/data-//'): $DBNOTE, $PICNOTE; $RUNNOTE"
log "$NOTE"
done_ok "$NOTE"
