#!/usr/bin/env bash
# By hand, after the site lost pictures (a new volume, a restore): puts every backed-up picture back on the site.
# Pictures the site still has are left alone. It goes in parts of about 100 MB, so the site's small disk never
# holds more than one part at a time. Afterwards tonight's images.sh puts the WebP/AVIF copies back too.
# Usage: ops/restore-pictures.sh            (from ~/bbv/backups/uploads)
#        ops/restore-pictures.sh <folder>   (from another folder, laid out like /data/uploads)
REPORT=0   # not a nightly job: messages go to the log and Telegram only
. "$(dirname "$0")/lib.sh"
SRC="${1:-$DATA_DIR/backups/uploads}"
[ -d "$SRC" ] || { log "no folder $SRC"; exit 1; }
FLY="$(command -v fly || command -v flyctl || true)"
[ -n "$FLY" ] || { log "flyctl is missing: curl -L https://fly.io/install.sh | sh"; exit 1; }
WORK="$DATA_DIR/restore-work"; rm -rf "$WORK"; mkdir -p "$WORK"
PART_KB="${PART_MB:-100}"; PART_KB=$((PART_KB * 1024))

# The list of pictures (no half-downloaded .part files, no names with spaces), cut into parts by size.
( cd "$SRC" && find . -type f ! -name '*.part' -printf '%k %P\n' ) | { grep -v '^[0-9]* .*[[:space:]]' || true; } \
  | awk -v max="$PART_KB" -v dir="$WORK" '{ if (sum + $1 > max && sum > 0) { n++; sum = 0 } sum += $1; print $2 > (dir "/part." sprintf("%04d", n)) }'
PARTS=("$WORK"/part.*)
[ -e "${PARTS[0]}" ] || { log "no pictures in $SRC"; exit 0; }
TOTAL="$(cat "${PARTS[@]}" | wc -l)"
log "$TOTAL picture(s) in ${#PARTS[@]} part(s) → $FLY_APP:/data/uploads"

"$FLY" ssh console -q -a "$FLY_APP" -C "mkdir -p /data/uploads" >/dev/null 2>&1 || true
i=0
for list in "${PARTS[@]}"; do
  i=$((i+1))
  FREE_KB="$(fly_free_kb)"; NEED_KB="$(( $(cd "$SRC" && tr '\n' '\0' < "$list" | du -ck --files0-from=- | tail -1 | cut -f1) * 2 ))"
  if [ -n "$FREE_KB" ] && [ "$FREE_KB" -lt $((NEED_KB + 51200)) ]; then
    alert "Restoring pictures stopped at part $i of ${#PARTS[@]}: the site's disk has only $((FREE_KB / 1024)) MB free. Grow it (fly volumes extend), then run this again."
    exit 1
  fi
  tar -cf "$WORK/part.tar" -C "$SRC" -T "$list"
  remote="/data/bbv-restore-$(date +%s)-$i.tar"
  "$FLY" ssh sftp put -q -a "$FLY_APP" "$WORK/part.tar" "$remote"
  # --skip-old-files: a picture the site already has stays as it is.
  ok=1; "$FLY" ssh console -q -a "$FLY_APP" -C "tar -xf $remote --no-same-owner --skip-old-files -C /data/uploads" || ok=0
  "$FLY" ssh console -q -a "$FLY_APP" -C "rm -f $remote" || log "could not delete $remote on the site: delete it by hand"
  [ "$ok" = 1 ] || { alert "Unpacking part $i on the site failed. Nothing else changed; run this again to carry on."; exit 1; }
  log "part $i of ${#PARTS[@]} done"
done
rm -rf "$WORK"
# The site's WebP/AVIF copies may be gone too: tonight's images.sh checks each one again and pushes the missing ones.
: > "$DATA_DIR/pushed.txt"
log "done: $TOTAL picture(s) checked. WebP/AVIF copies go back with tonight's images.sh (or run it now)."
send "Big Boys Vault: pictures restored to $FLY_APP ($TOTAL checked)."
