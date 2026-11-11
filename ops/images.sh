#!/usr/bin/env bash
# After the backup: WebP (and AVIF when avifenc is here) beside every JPEG picture, made on this VM, pushed to
# the site. Needs: webp, libavif-bin (optional), flyctl signed in (FLY_API_TOKEN in ops/env).
#
# What has reached the site is written in a list (pushed.txt). Everything converted but not on that list is
# pushed, a few hundred files at a time, so a push that failed last night is simply tried again tonight.
# To check everything again (for example after a restore): empty $DATA_DIR/pushed.txt and run this script. Files
# the site still answers for are only put back on the list; the rest are pushed.
. "$(dirname "$0")/lib.sh"
one_at_a_time
UP="$DATA_DIR/backups/uploads"; OUT="$DATA_DIR/converted"; LEDGER="$DATA_DIR/pushed.txt"
WORK="$DATA_DIR/push-work"; BATCH="${PUSH_BATCH:-300}"
mkdir -p "$OUT" "$UP"; touch "$LEDGER"
command -v cwebp >/dev/null || { alert "cwebp is missing on the VM, so no WebP pictures are made. Fix: sudo apt install -y webp"; exit 1; }
HAVE_AVIF=0; command -v avifenc >/dev/null && HAVE_AVIF=1

# 1. Make what's missing. A copy that fails half-way is thrown away, so it is made again next time.
MADE=0; BAD=0
while IFS= read -r -d '' jpg; do
  rel="${jpg#"$UP"/}"; stem="${rel%.*}"
  mkdir -p "$OUT/$(dirname "$rel")"
  if [ ! -s "$OUT/$stem.webp" ]; then
    if cwebp -quiet -q "${WEBP_QUALITY:-80}" "$jpg" -o "$OUT/$stem.webp"; then MADE=$((MADE+1)); else rm -f "$OUT/$stem.webp"; BAD=$((BAD+1)); fi
  fi
  # AVIF is smaller still but slow to make; thumbnails only, unless AVIF_ALL=1.
  if [ "$HAVE_AVIF" = 1 ] && [ ! -s "$OUT/$stem.avif" ] && { [ "${AVIF_ALL:-0}" = 1 ] || [[ "$stem" == *_s ]]; }; then
    if avifenc --min 20 --max 32 --speed 6 -j 2 "$jpg" "$OUT/$stem.avif" >/dev/null 2>&1; then MADE=$((MADE+1)); else rm -f "$OUT/$stem.avif"; BAD=$((BAD+1)); fi
  fi
done < <(find "$UP" -type f \( -iname '*.jpg' -o -iname '*.jpeg' \) -print0)
log "$MADE new file(s) made, $BAD could not be made"

# 2. Everything made but not yet on the site.
declare -A SENT=()
while IFS= read -r l; do [ -n "$l" ] && SENT["$l"]=1; done < "$LEDGER"
rm -rf "$WORK"; mkdir -p "$WORK"
WAIT="$WORK/waiting.txt"; : > "$WAIT"
while IFS= read -r -d '' f; do
  rel="${f#"$OUT"/}"
  case "$rel" in *[[:space:]]*) log "skipped (space in the name): $rel"; continue ;; esac
  [ -n "${SENT[$rel]:-}" ] || printf '%s\n' "$rel" >> "$WAIT"
done < <(find "$OUT" -type f \( -name '*.webp' -o -name '*.avif' \) -print0)
WAITING="$(wc -l < "$WAIT")"
log "$WAITING file(s) not on the list yet"
if [ "$WAITING" -eq 0 ]; then done_ok "Nothing new to push ($MADE made tonight)"; exit 0; fi

# Files the site already has (a new or emptied list, say after restore-pictures.sh) go straight on the list,
# so they are neither sent again nor counted against the site's free space.
xargs -P 8 -I{} sh -c 'curl -sf -o /dev/null -I --max-time 20 "$1/uploads/$2" && echo "$2"' _ "$ORIGIN" {} < "$WAIT" > "$WORK/already.txt" || true
if [ -s "$WORK/already.txt" ]; then
  cat "$WORK/already.txt" >> "$LEDGER"
  grep -vxF -f "$WORK/already.txt" "$WAIT" > "$WAIT.new" || true
  mv "$WAIT.new" "$WAIT"
  log "$(wc -l < "$WORK/already.txt") of them were on the site already"
fi
WAITING="$(wc -l < "$WAIT")"
log "$WAITING file(s) waiting to go to the site"
if [ "$WAITING" -eq 0 ]; then done_ok "Nothing new to push ($MADE made tonight)"; exit 0; fi

FLY="$(command -v fly || command -v flyctl || true)"
[ -n "$FLY" ] || { alert "flyctl is not on the VM (expected ~/.fly/bin/fly). $WAITING picture file(s) are waiting to be pushed."; exit 1; }
[ -n "${FLY_API_TOKEN:-}" ] || { alert "FLY_API_TOKEN is empty in ops/env, so pictures can't be pushed. $WAITING file(s) waiting."; exit 1; }

# The site's disk is small (1 GB): never fill it. Before each batch (below), at least 150 MB must stay free after
# the batch's tar file and its unpacked files are both on it. Batches already pushed stay on the list.
# 3. Push in batches: one small tar file per batch, sent with one fly command and unpacked on the site.
#    Then each file is checked on the site's own address; only files that answer go on the list.
split -l "$BATCH" -d -a 4 "$WAIT" "$WORK/batch."
PUSHED=0; MISSED=0; B=0; NB="$(ls "$WORK"/batch.* | wc -l)"
for list in "$WORK"/batch.*; do
  B=$((B+1)); n="$(wc -l < "$list")"
  tarf="$WORK/push.tar"; remote="/data/bbv-push-$(date +%s)-$B.tar"
  BKB="$(cd "$OUT" && tr '\n' '\0' < "$list" | du -ck --files0-from=- 2>/dev/null | tail -1 | cut -f1)"; BKB="${BKB:-0}"
  FREE_KB="$(fly_free_kb)"
  if [ -n "$FREE_KB" ] && [ "$FREE_KB" -lt $((2 * BKB + 153600)) ]; then
    alert "Stopped pushing pictures at batch $B of $NB ($PUSHED file(s) done): the site's disk has only $((FREE_KB / 1024)) MB free and this batch needs about $((2 * BKB / 1024)) MB. Grow it: fly volumes extend <id> -s 2 -a $FLY_APP"
    exit 1
  fi
  [ -n "$FREE_KB" ] || log "could not read the site's free space; pushing batch $B anyway"
  tar -cf "$tarf" -C "$OUT" -T "$list"
  if ! "$FLY" ssh sftp put -q -a "$FLY_APP" "$tarf" "$remote" >> "$LOG_DIR/images-push.log" 2>&1; then
    alert "Pushing pictures to Fly failed (batch $B of $NB, $PUSHED file(s) done). See $LOG_DIR/images-push.log; it tries again tomorrow."
    exit 1
  fi
  ok_unpack=1
  "$FLY" ssh console -q -a "$FLY_APP" -C "tar -xf $remote --no-same-owner -C /data/uploads" >> "$LOG_DIR/images-push.log" 2>&1 || ok_unpack=0
  "$FLY" ssh console -q -a "$FLY_APP" -C "rm -f $remote" >> "$LOG_DIR/images-push.log" 2>&1 || log "could not delete $remote on the site"
  if [ "$ok_unpack" = 0 ]; then
    alert "Unpacking pictures on the site failed (batch $B of $NB). See $LOG_DIR/images-push.log; it tries again tomorrow."
    exit 1
  fi
  # Don't trust the exit codes alone: ask the site for every file in the batch.
  xargs -P 8 -I{} sh -c 'curl -sf -o /dev/null -I --max-time 20 "$1/uploads/$2" && echo "$2"' _ "$ORIGIN" {} < "$list" > "$WORK/ok.txt" || true
  got="$(wc -l < "$WORK/ok.txt")"
  cat "$WORK/ok.txt" >> "$LEDGER"
  PUSHED=$((PUSHED + got)); MISSED=$((MISSED + n - got))
  log "batch $B of $NB: $got of $n on the site"
  if [ "$got" -eq 0 ]; then
    alert "Pushed batch $B but none of its $n file(s) show on $ORIGIN. Stopping; it tries again tomorrow."
    exit 1
  fi
done
rm -rf "$WORK"
NOTE="$PUSHED file(s) pushed, $MADE made tonight"
[ "$MISSED" -gt 0 ] && NOTE="$NOTE; $MISSED did not show on the site and will be tried again tomorrow"
[ "$BAD" -gt 0 ] && NOTE="$NOTE; $BAD could not be made"
log "$NOTE"
done_ok "$NOTE"
