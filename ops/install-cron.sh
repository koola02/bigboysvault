#!/usr/bin/env bash
# Puts the jobs from ops/crontab.example into this user's crontab, with OPS (this folder) and LOGS (the log
# folder) filled in. Safe to run again: it swaps the old Big Boys Vault lines and leaves any other line alone.
# Prints "unchanged" or "installed". Used by setup-vm.sh and by the nightly update.sh.
set -euo pipefail
OPS="$(cd "$(dirname "$0")" && pwd)"
# The log folder follows DATA_DIR in ops/env (default ~/bbv), the same place lib.sh writes to.
DATA="$( { grep -E '^DATA_DIR=.+' "${BBV_ENV:-$OPS/env}" 2>/dev/null || true; } | tail -1 | cut -d= -f2- | tr -d "\"'\r")"
[ -n "$DATA" ] || DATA="$HOME/bbv"
LOGS="$DATA/logs"; mkdir -p "$LOGS"
WANT="$(grep 'bigboysvault/ops' "$OPS/crontab.example" | tr -d '\r' | sed -e "s#OPS#$OPS#g" -e "s#LOGS#$LOGS#g")"
HAVE="$(crontab -l 2>/dev/null | grep 'bigboysvault/ops' || true)"
if [ "$WANT" = "$HAVE" ]; then echo unchanged; exit 0; fi
{ crontab -l 2>/dev/null | grep -v 'bigboysvault/ops' | grep -v '^# Big Boys Vault jobs' || true; printf '%s\n' "$WANT"; } | crontab -
echo installed
