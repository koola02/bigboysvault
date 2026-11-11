#!/usr/bin/env bash
# Every 30 minutes: tidies trainers' own card photos (the ones they took themselves). Each is straightened and
# cropped to the card on this VM (ops/cardcrop.py), then sent back to the site, which keeps the original so the
# trainer can put it back ("Use my original photo"). A photo with no clear card in it is left as it was.
# Needs the Python tools from ops/setup-extras.sh; without them this job quietly does nothing.
. "$(dirname "$0")/lib.sh"
one_at_a_time
PY="$DATA_DIR/venv/bin/python"
[ -x "$PY" ] || { log "no Python tools yet: run ops/setup-extras.sh"; exit 0; }
LIST="$(admin_curl "$SITE/admin/photos/pending?limit=20")"
WORK="$(mktemp -d)"
DONE=0; SKIP=0; BAD=0
while IFS=$'\t' read -r id path; do
  [ -n "$id" ] || continue
  if ! curl -sS --fail --max-time 60 -H 'Accept: image/jpeg' -o "$WORK/in.jpg" "$ORIGIN$path"; then BAD=$((BAD + 1)); continue; fi
  set +e; "$PY" "$OPS_DIR/cardcrop.py" "$WORK/in.jpg" "$WORK/out.jpg"; rc=$?; set -e
  if [ "$rc" = 0 ]; then
    admin_curl -X POST -H 'Content-Type: image/jpeg' --data-binary @"$WORK/out.jpg" "$SITE/admin/photos/$id" >/dev/null && DONE=$((DONE + 1)) || BAD=$((BAD + 1))
  else
    # No clear card (2) or a file that can't be read (3): left as it was, and not tried again.
    admin_curl -X POST "$SITE/admin/photos/$id?skip=1" >/dev/null && SKIP=$((SKIP + 1)) || BAD=$((BAD + 1))
  fi
  rm -f "$WORK/in.jpg" "$WORK/out.jpg"
done < <(printf '%s' "$LIST" | python3 -c 'import json,sys
for p in json.load(sys.stdin).get("photos", []): print(str(p["id"]) + "	" + p["image"])')
rm -rf "$WORK"
[ $((DONE + SKIP + BAD)) -eq 0 ] && exit 0   # nothing waiting: no report, so the dashboard keeps the last real one
NOTE="$DONE photo(s) tidied, $SKIP left as they were"
[ "$BAD" -gt 0 ] && NOTE="$NOTE, $BAD could not be done (tried again next time)"
log "$NOTE"
done_ok "$NOTE"
