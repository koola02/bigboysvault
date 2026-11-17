#!/usr/bin/env bash
# Nightly: "More like this" and Ask the Vault's "cards like ..." worked out with a small language model on this
# VM (ops/similar.py). Needs the Python tools from ops/setup-extras.sh; without them this job quietly does nothing.
. "$(dirname "$0")/lib.sh"
one_at_a_time
PY="$DATA_DIR/venv/bin/python"
[ -x "$PY" ] || { log "no Python tools yet: run ops/setup-extras.sh"; exit 0; }
ERRS="$LOG_DIR/similar-errors.log"
log "run" >> "$ERRS"   # so the last lines of the file are always about this run
# ORIGIN: the site's own address, skipping Cloudflare. similar.py asks again when the site is slow (17 Nov 2026).
if ! NOTE="$(SITE="$SITE" ORIGIN="$ORIGIN" ADMIN_TOKEN="$ADMIN_TOKEN" DATA_DIR="$DATA_DIR" HF_HOME="$DATA_DIR/hf" nice -n 10 "$PY" "$OPS_DIR/similar.py" 2>>"$ERRS" | tail -1)"; then
  WHY="$( { grep -v '^ ' "$ERRS" || true; } | tail -n 1 | cut -c1-160)"
  case "$WHY" in
    *Timeout*|*timed\ out*) PLAIN="the site took too long to answer, four times" ;;
    *HTTPError*|*HTTP\ Error*) PLAIN="the site answered with an error" ;;
    *URLError*|*Connection*) PLAIN="the site could not be reached" ;;
    *) PLAIN="the program stopped" ;;
  esac
  alert "\"More like this\" was not updated tonight: $PLAIN ($WHY). Yesterday's look-alikes stay on the site; it tries again tomorrow night."
  exit 1
fi
[ -n "$NOTE" ] || { alert "The similar-cards job made nothing; see $ERRS on the VM."; exit 1; }
log "$NOTE"
done_ok "$NOTE"
