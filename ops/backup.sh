#!/usr/bin/env bash
# Nightly: a clean copy of the database and every new picture, kept here and in a Google Cloud Storage bucket.
# Restore: see docs/OPERATIONS.md. Needs: python3 (checks the copy), gcloud (optional, for the bucket).
. "$(dirname "$0")/lib.sh"
BK="$DATA_DIR/backups"; UP="$BK/uploads"; mkdir -p "$BK/db" "$UP"
STAMP="$(date +%Y-%m-%d)"
DB="$BK/db/data-$STAMP.db"

log "database snapshot → $DB"
admin_curl -o "$DB.part" "$SITE/admin/backup/db"
# A snapshot that doesn't open is worse than none: check it before keeping it.
python3 - "$DB.part" <<'EOF'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1])
assert c.execute("PRAGMA integrity_check").fetchone()[0] == "ok", "integrity check failed"
n = c.execute("SELECT COUNT(*) FROM products").fetchone()[0]
m = c.execute("SELECT COUNT(*) FROM members").fetchone()[0]
print(f"ok: {n} cards, {m} trainers")
EOF
mv "$DB.part" "$DB"
gzip -kf "$DB"

# Pictures: only what's new since last time.
SINCE_FILE="$BK/uploads.since"; SINCE="$(cat "$SINCE_FILE" 2>/dev/null || echo 0)"
log "pictures changed since $SINCE"
LIST="$(admin_curl "$SITE/admin/backup/uploads?since=$SINCE")"
NOW="$(printf '%s' "$LIST" | json_get "d['now']")"
printf '%s' "$LIST" | python3 -c 'import json,sys; [print(f["path"], f["size"]) for f in json.load(sys.stdin)["files"]]' > "$BK/uploads.new"
TOTAL="$(wc -l < "$BK/uploads.new")"; N=0; SKIP=0
log "$TOTAL file(s) listed (a first run fetches every picture: give it a while)"
while read -r p size; do
  [ -z "$p" ] && continue
  rel="${p#/uploads/}"; mkdir -p "$UP/$(dirname "$rel")"
  # Already here and whole: skip, so an interrupted run carries on where it stopped.
  if [ -f "$UP/$rel" ] && [ "$(stat -c %s "$UP/$rel")" = "$size" ]; then SKIP=$((SKIP+1)); continue; fi
  if curl -sS --fail --max-time 120 -o "$UP/$rel.part" "$SITE$p"; then mv "$UP/$rel.part" "$UP/$rel"; N=$((N+1)); else rm -f "$UP/$rel.part"; log "could not fetch $p"; fi
  [ $(( (N + SKIP) % 250 )) -eq 0 ] && log "  $((N + SKIP)) of $TOTAL"
done < "$BK/uploads.new"
log "$N new picture(s), $SKIP already here"
echo "$NOW" > "$SINCE_FILE"

# Keep 30 days of database copies here.
find "$BK/db" -name 'data-*.db' -mtime +30 -delete
find "$BK/db" -name 'data-*.db.gz' -mtime +90 -delete

# Off the VM too: a bucket in Google Cloud Storage (gcloud is on every Google VM; make the bucket once).
if [ -n "${GCS_BUCKET:-}" ] && command -v gcloud >/dev/null; then
  log "→ gs://$GCS_BUCKET"
  gcloud storage cp "$DB.gz" "gs://$GCS_BUCKET/db/" --quiet
  gcloud storage rsync "$UP" "gs://$GCS_BUCKET/uploads" --recursive --quiet
fi
log "backup done"
