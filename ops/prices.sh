#!/usr/bin/env bash
# 3 am: today's market prices for the vault's cards and the sellers' cards, a few at a time, until none are stale.
# Sellers whose own price is far from the market get a note. Takes a while; it runs on the site, not here.
. "$(dirname "$0")/lib.sh"
one_at_a_time
LIMIT="${PRICE_BATCH:-50}"; MAX_LOOPS="${PRICE_MAX_LOOPS:-60}"
TOTAL_P=0; TOTAL_C=0; FLAGGED=0; R="?"
for i in $(seq 1 "$MAX_LOOPS"); do
  OUT="$(admin_curl -X POST "$SITE/admin/prices/refresh?limit=$LIMIT")" || { alert "Price refresh failed on loop $i: ${OUT:0:200}"; exit 1; }
  # A reply that isn't the expected JSON stops the job (and the error catcher in lib.sh raises the alert).
  P="$(printf '%s' "$OUT" | json_get "d['products']")"; C="$(printf '%s' "$OUT" | json_get "d['cards']")"
  F="$(printf '%s' "$OUT" | json_get "d['flagged']")"; R="$(printf '%s' "$OUT" | json_get "d['remaining']")"; FAILED="$(printf '%s' "$OUT" | json_get "d['failed']")"
  TOTAL_P=$((TOTAL_P+P)); TOTAL_C=$((TOTAL_C+C)); FLAGGED=$((FLAGGED+F))
  log "loop $i: $P vault cards, $C sellers' cards, $F flagged, $FAILED lookups failed, $R still stale"
  [ "$R" -eq 0 ] && break
  if [ "$FAILED" -ge "$LIMIT" ]; then alert "The price service isn't answering (loop $i). Stopping for today; $R card(s) still old."; break; fi
  sleep 5
done
NOTE="$TOTAL_P vault cards, $TOTAL_C sellers' cards, $FLAGGED sellers told; $R still old"
log "prices done: $NOTE"
# Then warm the card search with the names people look for most.
if admin_curl -X POST "$SITE/admin/catalog/warm?limit=40" >/dev/null; then log "search warmed"; else log "search warm-up failed (not important)"; fi
done_ok "$NOTE"
