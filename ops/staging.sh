#!/usr/bin/env bash
# A staging copy on this VM: the site's Docker image with last night's database and pictures.
# Click through a change here before deploying it. Usage: ops/staging.sh [branch]   Needs: docker, git.
# It holds real members' data, so it only listens on the VM itself (127.0.0.1): you reach it through an SSH
# tunnel, never through the firewall. It has its own random admin password and stops by itself after a few hours.
REPORT=0   # not a nightly job
. "$(dirname "$0")/lib.sh"
: "${REPO_DIR:=$(dirname "$OPS_DIR")}"; BRANCH="${1:-master}"; HOURS="${STAGING_HOURS:-4}"
BK="$DATA_DIR/backups"; ST="$DATA_DIR/staging"; SRC="$DATA_DIR/staging-src"
mkdir -p "$ST/uploads"; chmod 700 "$ST"
LATEST="$(ls -t "$BK"/db/data-*.db 2>/dev/null | head -1 || true)"; [ -n "$LATEST" ] || { log "no backup yet: run ops/backup.sh first"; exit 1; }
docker rm -f bbv-staging >/dev/null 2>&1 || true
rm -f "$ST"/data.db-wal "$ST"/data.db-shm
cp "$LATEST" "$ST/data.db"
rsync -a --delete "$BK/uploads/" "$ST/uploads/" 2>/dev/null || cp -r "$BK/uploads/." "$ST/uploads/"
# Build from a separate copy of the code, so the scripts cron runs (in $REPO_DIR) never change branch.
if [ ! -d "$SRC/.git" ]; then git clone -q "$(git -C "$REPO_DIR" remote get-url origin)" "$SRC"; fi
cd "$SRC" && git fetch -q origin && git checkout -q -B "$BRANCH" "origin/$BRANCH"
log "building $(git log --oneline -1) on Fly's builder (a few minutes; nothing is deployed)"
IMG="registry.fly.io/${FLY_APP:-bigboysvault}:preview"
if fly deploy -a "${FLY_APP:-bigboysvault}" --build-only --push --remote-only --image-label preview >"$DATA_DIR/staging-build.log" 2>&1    && fly auth docker >/dev/null 2>&1 && docker pull -q "$IMG" >/dev/null; then
  docker tag "$IMG" bbv-staging
else
  log "Fly's builder didn't work (see $DATA_DIR/staging-build.log); building here instead, which is slow"
  docker build -q -t bbv-staging . >/dev/null
fi
# Never a fixed password: STAGING_ADMIN_TOKEN from ops/env, or a new random one each time.
TOKEN="${STAGING_ADMIN_TOKEN:-}"
if [ -z "$TOKEN" ] || [ "$TOKEN" = staging ] || [ "${#TOKEN}" -lt 16 ]; then TOKEN="$(openssl rand -hex 16 2>/dev/null || head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')"; fi
( umask 077; printf '%s\n' "$TOKEN" > "$ST/admin-password.txt" )
docker run -d --rm --name bbv-staging -p 127.0.0.1:8080:8080 -v "$ST:/data" -e ADMIN_TOKEN="$TOKEN" -e PUBLIC_URL="http://localhost:8080" bbv-staging >/dev/null
# Stops by itself after $HOURS hours, so last night's data doesn't stay up and forgotten.
nohup sh -c "sleep $((HOURS * 3600)); docker rm -f bbv-staging >/dev/null 2>&1" >/dev/null 2>&1 &
sleep 3
curl -sS --max-time 20 "http://127.0.0.1:8080/healthz" && echo
log "staging is running on this VM only, for $HOURS hours."
log "To see it: open Google Cloud Shell (the >_ button in the Cloud console) and run"
log "  gcloud compute ssh $(hostname) --zone $(curl -s -H 'Metadata-Flavor: Google' http://metadata.google.internal/computeMetadata/v1/instance/zone 2>/dev/null | awk -F/ '{print $NF}') -- -N -L 8080:localhost:8080"
log "then Web Preview (the eye button) > Preview on port 8080. Only you can open that link."
log "The preview's admin password is in $ST/admin-password.txt"
log "Do not open port 8080 in the firewall. Stop it early with: docker rm -f bbv-staging"
