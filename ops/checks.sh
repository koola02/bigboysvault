#!/usr/bin/env bash
# Every 15 minutes: the pages people use most, each checked for a good answer, the right content and its speed,
# from this VM in the US. uptime.sh only asks "is the site up?"; this asks "does each page work?".
# A page that fails twice in a row (a retry 20 seconds later) rings the admins' bell through the site.
. "$(dirname "$0")/lib.sh"
one_at_a_time
SLOW="${CHECK_SLOW_S:-3}"

# One product page and one store page from the sitemap, picked again once a day, so the check follows what's there.
PICK="$DATA_DIR/checks.pick"
if [ ! -s "$PICK" ] || [ -n "$(find "$PICK" -mmin +1440 2>/dev/null)" ]; then
  SM="$(curl -sS --max-time 30 "$SITE/sitemap.xml" 2>/dev/null || true)"
  P="$(printf '%s' "$SM" | grep -o '<loc>[^<]*/products/[^<]*</loc>' | head -1 | sed -e 's#<loc>##' -e 's#</loc>##' || true)"
  S="$(printf '%s' "$SM" | grep -o '<loc>[^<]*/store/[^<]*</loc>' | head -1 | sed -e 's#<loc>##' -e 's#</loc>##' || true)"
  [ -n "$P" ] && printf '%s\n%s\n' "$P" "$S" > "$PICK"
fi
PRODUCT="$(sed -n 1p "$PICK" 2>/dev/null)"; STORE="$(sed -n 2p "$PICK" 2>/dev/null)"

# name | address | words the page must contain
CHECKS=(
  "home|$SITE/|Big Boys Vault"
  "browse|$SITE/all|All cards"
  "stores|$SITE/stores|Stores"
  "log in|$SITE/login|Log in"
  "cart|$SITE/cart/lines?v=&c=|\"lines\""
  "health|$SITE/healthz|\"ok\":true"
)
[ -n "$PRODUCT" ] && CHECKS+=("card page|$PRODUCT|· Big Boys Vault</title>")
[ -n "$STORE" ] && CHECKS+=("store|$STORE|sf-head")

BODY="$(mktemp)"
# One check: prints "ok <seconds>" or "fail <why>".
check() {
  local url="$1" want="$2" out code t
  out="$(curl -sS -o "$BODY" -w '%{http_code} %{time_total}' --max-time 20 -H 'User-Agent: bbv-checks' "$url" 2>/dev/null || echo "000 20")"
  code="${out%% *}"; t="${out##* }"
  if [ "$code" != 200 ]; then echo "fail answered $code"; return; fi
  if ! grep -qF "$want" "$BODY"; then echo "fail page looks wrong"; return; fi
  echo "ok $t"
}

FAILED=(); SLOWS=(); WORST_N=""; WORST_T=0
for c in "${CHECKS[@]}"; do
  IFS='|' read -r name url want <<< "$c"
  r="$(check "$url" "$want")"
  if [ "${r%% *}" = fail ]; then sleep 20; r="$(check "$url" "$want")"; fi   # a blip is not an alarm
  if [ "${r%% *}" = fail ]; then FAILED+=("$name (${r#fail })"); continue; fi
  t="${r#ok }"
  if awk "BEGIN{exit !($t > $WORST_T)}"; then WORST_T="$t"; WORST_N="$name"; fi
  if awk "BEGIN{exit !($t > $SLOW)}"; then SLOWS+=("$name $(printf '%.1f' "$t") s"); fi
done
rm -f "$BODY"
N=${#CHECKS[@]}; BAD=${#FAILED[@]}
NOTE="$((N - BAD)) of $N pages fine; slowest $WORST_N $(printf '%.1f' "$WORST_T") s"
[ ${#SLOWS[@]} -gt 0 ] && NOTE="$NOTE; slow: ${SLOWS[*]}"
if [ "$BAD" -gt 0 ]; then
  alert "$BAD of $N pages failed: ${FAILED[*]}"
  exit 0
fi
log "$NOTE"
done_ok "$NOTE"
