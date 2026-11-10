#!/usr/bin/env bash
# 3 am: today's market prices for the vault's cards and the sellers' cards, a few at a time, until none are stale.
# Sellers whose own price is far from the market get a note. Takes a while; it runs on the site, not here.
. "$(dirname "$0")/lib.sh"
LIMIT="${PRICE_BATCH:-50}"; MAX_LOOPS="${PRICE_MAX_LOOPS:-60}"
TOTAL_P=0; TOTAL_C=0; FLAGGED=0
for i in $(seq 1 "$MAX_LOOPS"); do
  OUT="$(admin_curl -X POST "$SITE/admin/prices/refresh?limit=$LIMIT")" || { alert "Price refresh failed on loop $i: $OUT"; exit 1; }
  P="$(printf '%s' "$OUT" | json_get "d['products']")"; C="$(printf '%s' "$OUT" | json_get "d['cards']")"
  F="$(printf '%s' "$OUT" | json_get "d['flagged']")"; R="$(printf '%s' "$OUT" | json_get "d['remaining']")"; FAILED="$(printf '%s' "$OUT" | json_get "d['failed']")"
  TOTAL_P=$((TOTAL_P+P)); TOTAL_C=$((TOTAL_C+C)); FLAGGED=$((FLAGGED+F))
  log "loop $i: $P vault cards, $C sellers' cards, $F flagged, $FAILED lookups failed, $R still stale"
  [ "$R" -eq 0 ] && break
  [ "$FAILED" -ge "$LIMIT" ] && { alert "The price service isn't answering (loop $i). Stopping for today."; break; }
  sleep 5
done
log "prices done: $TOTAL_P vault cards, $TOTAL_C sellers' cards, $FLAGGED sellers told"
# Then warm the card search with the names people look for most.
admin_curl -X POST "$SITE/admin/catalog/warm?limit=40" >/dev/null && log "search warmed" || log "search warm-up failed (not important)"
