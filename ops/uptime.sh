#!/usr/bin/env bash
# Every few minutes: is the site up? Two misses in a row raise the alarm; the first good answer after that says so.
. "$(dirname "$0")/lib.sh"
STATE="$DATA_DIR/uptime.state"; FAILS=0; [ -f "$STATE" ] && FAILS="$(cat "$STATE")"
if OUT="$(curl -sS --max-time 15 "$SITE/healthz")" && [ "$(printf '%s' "$OUT" | json_get "d.get('ok')" 2>/dev/null)" = "True" ]; then
  if [ "$FAILS" -ge 2 ]; then alert "$SITE is back up ($(printf '%s' "$OUT" | json_get "d['db_ms']") ms)."; fi
  echo 0 > "$STATE"
else
  FAILS=$((FAILS+1)); echo "$FAILS" > "$STATE"
  log "miss $FAILS: ${OUT:-no answer}"
  [ "$FAILS" -eq 2 ] && alert "$SITE is DOWN (two checks in a row). Last answer: ${OUT:-none}. Check: fly status -a ${FLY_APP:-bigboysvault} ; fly logs -a ${FLY_APP:-bigboysvault}"
fi
