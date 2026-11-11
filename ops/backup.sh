#!/usr/bin/env bash
# Nightly: a clean copy of the database and every new picture, kept here and in a Google Cloud Storage bucket.
# Restore: see docs/OPERATIONS.md. Needs: python3 (checks the copy), gcloud (optional, for the bucket).
. "$(dirname "$0")/lib.sh"
one_at_a_time
BK="$DATA_DIR/backups"; UP="$BK/uploads"; mkdir -p "$BK/db" "$UP"
STAMP="$(date +%Y-%m-%d)"
DB="$BK/db/data-$STAMP.db"
NOTES=()

USE="$(disk_pct || echo 0)"
[ "${USE:-0}" -ge 95 ] && alert "The VM disk is ${USE}% full. Tonight's backup may fail: make room (old copies in $BK/db)."

log "database snapshot → $DB"
# Straight from the Fly address: no Cloudflare time limit on a big download.
admin_curl -o "$DB.part" "$ORIGIN/admin/backup/db"
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
NOTES+=("database $(du -h "$DB" | cut -f1)")

# Pictures: only what's new since last time. Once a week (Sunday) every picture is listed again, to catch any gap;
# pictures already here are skipped by size, so that costs little.
SINCE_FILE="$BK/uploads.since"; SINCE="$(cat "$SINCE_FILE" 2>/dev/null || echo 0)"
[ "$(date +%u)" = 7 ] && SINCE=0
log "pictures changed since $SINCE"
LIST="$(admin_curl "$ORIGIN/admin/backup/uploads?since=$SINCE")"
NOW="$(printf '%s' "$LIST" | json_get "d['now']")"
printf '%s' "$LIST" | python3 -c 'import json,sys; [print(f["path"], f["size"]) for f in json.load(sys.stdin)["files"]]' > "$BK/uploads.new"
TOTAL="$(wc -l < "$BK/uploads.new")"; N=0; SKIP=0; GONE=0; F=0; K=0
log "$TOTAL file(s) listed (a first run fetches every picture: give it a while)"
while read -r p size; do
  [ -z "$p" ] && continue
  size="${size%$'\r'}"; rel="${p#/uploads/}"; K=$((K+1))
  [ $((K % 250)) -eq 0 ] && log "  $K of $TOTAL"
  # WebP/AVIF copies made on this VM (ops/images.sh) are already here: no need to fetch them back.
  if [ -f "$DATA_DIR/converted/$rel" ]; then SKIP=$((SKIP+1)); continue; fi
  # Already here and whole: skip, so an interrupted run carries on where it stopped.
  if [ -f "$UP/$rel" ] && [ "$(stat -c %s "$UP/$rel")" = "$size" ]; then SKIP=$((SKIP+1)); continue; fi
  mkdir -p "$UP/$(dirname "$rel")"
  # Up to three tries (a dropped connection is common on a long night). 404 means it was deleted on the site.
  code=000
  for try in 1 2 3; do
    code="$(curl -sS --max-time 120 -H 'Accept: image/jpeg,*/*;q=0.1' -o "$UP/$rel.part" -w '%{http_code}' "$ORIGIN$p" 2>/dev/null || true)"
    [ "$code" = 200 ] || [ "$code" = 404 ] || [ "$try" = 3 ] && break
    sleep $((try * 5))
  done
  if [ "$code" = 200 ]; then mv "$UP/$rel.part" "$UP/$rel"; N=$((N+1))
  elif [ "$code" = 404 ]; then rm -f "$UP/$rel.part"; GONE=$((GONE+1))
  else rm -f "$UP/$rel.part"; F=$((F+1)); log "could not fetch $p (HTTP ${code:-000})"; fi
done < "$BK/uploads.new"
log "$N new picture(s), $SKIP already here, $GONE gone from the site, $F failed"
NOTES+=("$N new picture(s)")
if [ "$F" -eq 0 ]; then
  # Move the marker only when every picture arrived, so a missed one is tried again tomorrow. Step back an hour:
  # a picture saved while the site was making the list is listed again next time (the size check skips it).
  echo "$((NOW - 3600))" > "$SINCE_FILE"
else
  NOTES+=("$F picture(s) failed, tried again tomorrow")
fi

# Keep 30 days of database copies here, and 90 days of the small .gz ones.
find "$BK/db" -name 'data-*.db' -mtime +30 -delete
find "$BK/db" -name 'data-*.db.gz' -mtime +90 -delete

# Off the VM too: a bucket in Google Cloud Storage (gcloud is on every Google VM; first-run.sh makes the bucket).
if [ -n "${GCS_BUCKET:-}" ] && command -v gcloud >/dev/null; then
  log "→ gs://$GCS_BUCKET"
  if gcloud storage cp "$DB.gz" "gs://$GCS_BUCKET/db/" --quiet && gcloud storage rsync "$UP" "gs://$GCS_BUCKET/uploads" --recursive --quiet; then
    NOTES+=("copied to the bucket")
  else
    NOTES+=("copy to the bucket FAILED"); BAD=1
  fi
else
  NOTES+=("no bucket set: copies are on the VM only")
fi

USE="$(disk_pct || echo 0)"
NOTES+=("VM disk ${USE}% full")
if [ "${USE:-0}" -ge 85 ]; then BAD=1; NOTES+=("make room soon"); fi
[ "$F" -gt 0 ] && BAD=1
NOTE="$(IFS=';'; printf '%s' "${NOTES[*]}" | sed 's/;/; /g')"
log "backup done: $NOTE"
if [ -n "${BAD:-}" ]; then alert "Backup finished with a problem: $NOTE"; else done_ok "$NOTE"; fi
