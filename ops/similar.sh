#!/usr/bin/env bash
# Nightly: "More like this" and Ask the Vault's "cards like ..." worked out with a small language model on this
# VM (ops/similar.py). Needs the Python tools from ops/setup-extras.sh; without them this job quietly does nothing.
. "$(dirname "$0")/lib.sh"
one_at_a_time
PY="$DATA_DIR/venv/bin/python"
[ -x "$PY" ] || { log "no Python tools yet: run ops/setup-extras.sh"; exit 0; }
NOTE="$(SITE="$SITE" ADMIN_TOKEN="$ADMIN_TOKEN" DATA_DIR="$DATA_DIR" HF_HOME="$DATA_DIR/hf" nice -n 10 "$PY" "$OPS_DIR/similar.py" 2>>"$LOG_DIR/similar-errors.log" | tail -1)"
[ -n "$NOTE" ] || { alert "The similar-cards job made nothing; see $LOG_DIR/similar-errors.log on the VM."; exit 1; }
log "$NOTE"
done_ok "$NOTE"
