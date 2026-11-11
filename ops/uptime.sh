#!/usr/bin/env bash
# Every few minutes: is the site up? Two misses in a row raise the alarm; the first good answer after that says so.
# It tells the site only when the site goes down and when it comes back (not every 5 minutes).
. "$(dirname "$0")/lib.sh"
STATE="$DATA_DIR/uptime.state"; DOWN_AT="$DATA_DIR/uptime.down"
FAILS=0; [ -f "$STATE" ] && FAILS="$(cat "$STATE")"; case "$FAILS" in ''|*[!0-9]*) FAILS=0 ;; esac
if OUT="$(curl -sS --max-time 15 "$SITE/healthz" 2>&1)" && [ "$(printf '%s' "$OUT" | json_get "d.get('ok')" 2>/dev/null)" = "True" ]; then
  if [ "$FAILS" -ge 2 ]; then
    START="$(cat "$DOWN_AT" 2>/dev/null || date +%s)"; MINS=$(( ($(date +%s) - START) / 60 )); [ "$MINS" -ge 5 ] || MINS=5
    WHEN="$(date -d "@$START" '+%b %-d %H:%M' 2>/dev/null || echo '?')"
    MSG="$SITE was down for about $MINS minutes (from $WHEN). It is back up ($(printf '%s' "$OUT" | json_get "d.get('db_ms')" 2>/dev/null || echo '?') ms)."
    log "$MSG"
    send "Big Boys Vault: $MSG"
    # First the outage (so admins get a bell), then "back up" (so the dashboard shows a tick again).
    report uptime false "Down for about $MINS minutes from $WHEN; now back up."
    report uptime true "Back up since $(date '+%b %-d %H:%M') after about $MINS minutes down."
    rm -f "$DOWN_AT"
  fi
  echo 0 > "$STATE"
else
  FAILS=$((FAILS+1)); echo "$FAILS" > "$STATE"
  [ "$FAILS" -eq 1 ] && date +%s > "$DOWN_AT"
  log "miss $FAILS: ${OUT:-no answer}"
  if [ "$FAILS" -eq 2 ]; then
    alert "$SITE is DOWN (two checks in a row). Last answer: ${OUT:-none}. Check: fly status -a $FLY_APP ; fly logs -a $FLY_APP"
  fi
fi
exit 0
