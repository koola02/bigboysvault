#!/usr/bin/env bash
# Deploy from the VM: fetch, test, back up, build on Fly's builders (no Rust needed here), deploy, check it's up.
# Usage: ops/deploy.sh [branch]   Needs: git, flyctl signed in (FLY_API_TOKEN).
REPORT=0   # not a nightly job: problems go to the log and Telegram
. "$(dirname "$0")/lib.sh"
: "${REPO_DIR:=$(dirname "$OPS_DIR")}"
BRANCH="${1:-master}"; SRC="$DATA_DIR/deploy-src"
# Build from a separate copy of the code, so the scripts cron runs (in $REPO_DIR) never change branch.
if [ ! -d "$SRC/.git" ]; then git clone -q "$(git -C "$REPO_DIR" remote get-url origin)" "$SRC"; fi
cd "$SRC"
git fetch -q origin && git checkout -q -B "$BRANCH" "origin/$BRANCH"
log "at $(git log --oneline -1)"
if command -v cargo >/dev/null; then
  log "tests"; cargo test --lib -q 2>&1 | tail -3
else
  log "no cargo here: tests ran where the commit was made (Fly builds from source)"
fi
# Back up before every schema change (docs/HANDOFF.md §5): the nightly job may be hours old.
bash "$OPS_DIR/backup.sh" | tail -1
log "deploying $FLY_APP"
fly deploy -a "$FLY_APP" --remote-only --strategy immediate
sleep 10
if curl -sS --max-time 20 "$SITE/healthz" | grep -q '"ok":true'; then log "live: $(fly releases -a "$FLY_APP" | sed -n 2p)"; else alert "Deploy of $FLY_APP finished but /healthz isn't answering"; exit 1; fi
# The phone, tablet and computer check (docs/HANDOFF.md §6), where this machine has Node, Chrome and the check's packages.
# It writes nothing on the live site.
if command -v node >/dev/null && [ -d "$SRC/tests/browser/node_modules" ]; then
  if node "$SRC/tests/browser/smoke.mjs" "$SITE" | tail -4; then log "the phone, tablet and computer check passed"; else alert "Deploy of $FLY_APP is live, but the phone, tablet and computer check failed: run node tests/browser/smoke.mjs $SITE"; exit 1; fi
else
  log "the phone, tablet and computer check wasn't run here (no Node, or tests/browser isn't installed): run it where the commit was made"
fi
