#!/usr/bin/env bash
# 02:15 every night, before the backup: bring this VM's copy of the scripts up to date from GitHub (fast-forward
# only, so nothing on the VM is ever overwritten), then refresh the cron lines if crontab.example changed.
# Also keeps the logs small. Tells the site how it went (job "update").
. "$(dirname "$0")/lib.sh"
one_at_a_time
REPO="$(git -C "$OPS_DIR" rev-parse --show-toplevel)"
BRANCH="${OPS_BRANCH:-master}"
cd "$REPO"
export GIT_TERMINAL_PROMPT=0   # never sit waiting for a password under cron
BEFORE="$(git rev-parse --short HEAD)"
# fileMode=false: a `chmod +x` done on the VM must not count as a local change and block the pull.
G=(git -c core.fileMode=false)
if [ "$(git rev-parse --abbrev-ref HEAD)" != "$BRANCH" ]; then
  if ! OUT="$("${G[@]}" checkout -q "$BRANCH" 2>&1)"; then
    alert "The VM's scripts are on another branch and can't go back to $BRANCH: ${OUT:0:200}"; exit 1
  fi
fi
if ! OUT="$(timeout 300 "${G[@]}" pull -q --ff-only origin "$BRANCH" 2>&1)"; then
  alert "Could not update the VM's scripts from GitHub (still at $BEFORE): ${OUT:0:200}. Fix by hand: cd $REPO && git status"
  exit 1
fi
AFTER="$(git rev-parse --short HEAD)"
if [ "$BEFORE" = "$AFTER" ]; then NOTE="Already up to date ($AFTER)"
else NOTE="Updated $BEFORE to $AFTER: $(git log -1 --format=%s | cut -c1-120)"; log "$NOTE"; fi
# The new install-cron.sh runs (git swapped the files; this script keeps running the old copy it already read).
if ! CRON="$(bash "$OPS_DIR/install-cron.sh" 2>&1)"; then
  alert "Scripts updated to $AFTER, but the cron lines could not be installed: ${CRON:0:200}"; exit 1
fi
[ "$CRON" = installed ] && NOTE="$NOTE; cron lines updated" && log "cron lines updated"
trim_logs
done_ok "$NOTE"
