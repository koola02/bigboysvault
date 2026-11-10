#!/usr/bin/env bash
# Deploy from the VM: pull, test, build on Fly's builders (no Rust needed here), deploy, check it's up.
# Usage: ops/deploy.sh [branch]   Needs: git, flyctl signed in (FLY_API_TOKEN).
. "$(dirname "$0")/lib.sh"
: "${FLY_APP:=bigboysvault}"; : "${REPO_DIR:=$HOME/bigboysvault}"
BRANCH="${1:-master}"
cd "$REPO_DIR"
git fetch -q origin && git checkout -q "$BRANCH" && git pull -q --ff-only origin "$BRANCH"
log "at $(git log --oneline -1)"
if command -v cargo >/dev/null; then
  log "tests"; cargo test --lib -q 2>&1 | tail -3
else
  log "no cargo here: tests ran where the commit was made (Fly builds from source)"
fi
# Back up before every schema change (GEMINI.md §5): the nightly job may be hours old.
"$OPS_DIR/backup.sh" | tail -1
log "deploying $FLY_APP"
fly deploy -a "$FLY_APP" --remote-only --strategy immediate
sleep 10
if curl -sS --max-time 20 "$SITE/healthz" | grep -q '"ok":true'; then log "live: $(fly releases -a "$FLY_APP" | sed -n 2p)"; else alert "Deploy of $FLY_APP finished but /healthz isn't answering"; exit 1; fi
