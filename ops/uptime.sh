#!/usr/bin/env bash
# Every few minutes: is the site up? It tells the site only when the site goes down and when it comes back
# (not every 5 minutes).
# A miss is looked at three ways before it counts, so a hiccup on the road there is never mistaken for the site
# being down: the normal address (through Cloudflare), the server's own address, and an outside site (is this VM
# online at all?).
#   The server answers nowhere, two runs in a row (10 minutes)          -> "DOWN", with what Fly says in the log
#   Only the normal address fails, three runs in a row (15 minutes)     -> "can't be reached through Cloudflare"
#   This VM itself is offline                                           -> written in the log; nobody is rung
. "$(dirname "$0")/lib.sh"
one_at_a_time
STATE="$DATA_DIR/uptime.state"; DOWN_AT="$DATA_DIR/uptime.down"; RUNG="$DATA_DIR/uptime.alerted"
FAILS=0; [ -f "$STATE" ] && FAILS="$(cat "$STATE")"; case "$FAILS" in ''|*[!0-9]*) FAILS=0 ;; esac

# One look at an address: prints what came back, and succeeds only when the site itself says it is fine.
look() {
  local out
  out="$(curl -sS --max-time "${2:-15}" "$1/healthz" 2>&1)" || { printf '%s' "$out"; return 1; }
  printf '%s' "$out"
  [ "$(printf '%s' "$out" | json_get "d.get('ok')" 2>/dev/null)" = "True" ]
}

GOOD=""
if OUT="$(look "$SITE")"; then
  GOOD=1
else
  # One lost moment is not a miss: look again before counting it.
  FIRST="$OUT"
  sleep "${RETRY_PAUSE:-20}"
  if OUT="$(look "$SITE")"; then
    GOOD=1
    log "no answer for a moment (${FIRST:-nothing came back}), fine 20 seconds later"
  fi
fi

if [ -n "$GOOD" ]; then
  if [ -f "$RUNG" ]; then
    START="$(cat "$DOWN_AT" 2>/dev/null || date +%s)"; MINS=$(( ($(date +%s) - START) / 60 )); [ "$MINS" -ge 5 ] || MINS=5
    WHEN="$(date -d "@$START" '+%b %-d %H:%M' 2>/dev/null || echo '?')"
    WHAT="$(cat "$RUNG" 2>/dev/null)"; [ -n "$WHAT" ] || WHAT="down"
    MSG="$SITE was $WHAT for about $MINS minutes (from $WHEN). It is fine again ($(printf '%s' "$OUT" | json_get "d.get('db_ms')" 2>/dev/null || echo '?') ms)."
    log "$MSG"
    send "Big Boys Vault: $MSG"
    # First the trouble (so admins get a bell if they haven't had one), then "fine again" (so the dashboard shows
    # a tick and the warning in the bell is marked fixed).
    report uptime false "It was $WHAT for about $MINS minutes from $WHEN; fine again now."
    report uptime true "Fine again since $(date '+%b %-d %H:%M') after about $MINS minutes $WHAT."
    rm -f "$RUNG"
  elif [ "$FAILS" -gt 0 ]; then
    log "fine again after $FAILS miss(es); nobody was rung"
  fi
  rm -f "$DOWN_AT"
  echo 0 > "$STATE"
  exit 0
fi

# Where is the trouble? The server's own address skips Cloudflare; an outside site says whether this VM is online.
DIRECT=""; ONLINE=""
if D="$(look "$ORIGIN")"; then DIRECT=1; fi
# (OUTSIDE can name other addresses to try, separated by spaces.)
for u in ${OUTSIDE:-https://www.google.com/generate_204 https://one.one.one.one/cdn-cgi/trace}; do
  if curl -sS --max-time 10 -o /dev/null "$u" 2>/dev/null; then ONLINE=1; break; fi
done

if [ -z "$DIRECT" ] && [ -z "$ONLINE" ]; then
  # Nothing answers from here, not even an outside site: this VM is the one that is cut off.
  log "this VM could not reach anything just now (the site: ${OUT:-nothing came back}); not counted against the site"
  exit 0
fi

FAILS=$((FAILS+1)); echo "$FAILS" > "$STATE"
[ -f "$DOWN_AT" ] || date +%s > "$DOWN_AT"
if [ -n "$DIRECT" ]; then
  KIND="hard to reach through Cloudflare"; NEED=3
else
  KIND="down"; NEED=2
fi
log "miss $FAILS ($KIND): ${OUT:-nothing came back}"

if [ "$FAILS" -ge "$NEED" ] && [ ! -f "$RUNG" ]; then
  echo "$KIND" > "$RUNG"
  if [ -n "$DIRECT" ]; then
    alert "$SITE could not be reached through Cloudflare for 15 minutes, but the server itself is fine (it answers at its own address). Open the site on your phone: if it loads, visitors are not affected. Last answer: ${OUT:0:60}"
  else
    # What Fly says right now goes into the log, so the reason is on record even after the server restarts.
    if command -v fly >/dev/null 2>&1; then
      { echo "--- what Fly says"; timeout 40 fly machine list -a "$FLY_APP" 2>&1 | tail -n 6; timeout 40 fly logs -a "$FLY_APP" --no-tail 2>&1 | tail -n 25; echo "---"; } | cut -c1-300 || true
    fi
    alert "$SITE is DOWN: two checks in a row failed and the server does not answer at its own address either. Last answer: ${OUT:0:90}. What Fly said is in $LOG_DIR/uptime.log on the VM."
  fi
fi
exit 0
