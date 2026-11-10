#!/usr/bin/env bash
# After the backup: WebP (and AVIF when avifenc is here) beside every JPEG picture, made on this VM, pushed to
# the site. The site serves them to browsers that take them, and the JPEG to the rest. Needs: webp, libavif-bin,
# flyctl signed in (FLY_API_TOKEN). Only new pictures are converted; a run with nothing new takes a second.
. "$(dirname "$0")/lib.sh"
: "${FLY_APP:=bigboysvault}"
UP="$DATA_DIR/backups/uploads"; OUT="$DATA_DIR/converted"; mkdir -p "$OUT"
command -v cwebp >/dev/null || { log "cwebp missing: sudo apt install -y webp"; exit 1; }
HAVE_AVIF=0; command -v avifenc >/dev/null && HAVE_AVIF=1
PUT="$DATA_DIR/put.txt"; : > "$PUT"; N=0
while IFS= read -r -d '' jpg; do
  rel="${jpg#$UP/}"; stem="${rel%.*}"
  mkdir -p "$OUT/$(dirname "$rel")"
  if [ ! -f "$OUT/$stem.webp" ]; then
    cwebp -quiet -q "${WEBP_QUALITY:-80}" "$jpg" -o "$OUT/$stem.webp" && { echo "put $OUT/$stem.webp /data/uploads/$stem.webp" >> "$PUT"; N=$((N+1)); }
  fi
  if [ "$HAVE_AVIF" = 1 ] && [ ! -f "$OUT/$stem.avif" ]; then
    # AVIF is smaller still but slow to make; thumbnails only, unless AVIF_ALL=1.
    if [ "${AVIF_ALL:-0}" = 1 ] || [[ "$stem" == *_s ]]; then
      avifenc --min 20 --max 32 --speed 6 -j 2 "$jpg" "$OUT/$stem.avif" >/dev/null 2>&1 && { echo "put $OUT/$stem.avif /data/uploads/$stem.avif" >> "$PUT"; N=$((N+1)); }
    fi
  fi
done < <(find "$UP" -type f \( -iname '*.jpg' -o -iname '*.jpeg' \) -print0)
log "$N new file(s) to push"
[ "$N" -eq 0 ] && exit 0
# One SSH session for the whole batch. fly's sftp shell reads commands from stdin.
if command -v fly >/dev/null; then
  fly ssh sftp shell -a "$FLY_APP" < "$PUT" > "$LOG_DIR/images-push.log" 2>&1 && log "pushed $N file(s)" || { alert "Pushing converted pictures to Fly failed: see $LOG_DIR/images-push.log"; exit 1; }
else
  log "flyctl missing: curl -L https://fly.io/install.sh | sh ; then set FLY_API_TOKEN"; exit 1
fi
