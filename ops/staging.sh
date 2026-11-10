#!/usr/bin/env bash
# A staging copy on this VM: the site's Docker image with last night's database and pictures, on port 8080.
# Click through a change here before deploying it. Usage: ops/staging.sh [branch]   Needs: docker, git.
. "$(dirname "$0")/lib.sh"
: "${REPO_DIR:=$HOME/bigboysvault}"; BRANCH="${1:-master}"
BK="$DATA_DIR/backups"; ST="$DATA_DIR/staging"; mkdir -p "$ST/uploads"
LATEST="$(ls -t "$BK"/db/data-*.db 2>/dev/null | head -1)"; [ -n "$LATEST" ] || { log "no backup yet: run ops/backup.sh first"; exit 1; }
cp "$LATEST" "$ST/data.db"
rsync -a --delete "$BK/uploads/" "$ST/uploads/" 2>/dev/null || cp -r "$BK/uploads/." "$ST/uploads/"
cd "$REPO_DIR" && git fetch -q origin && git checkout -q "$BRANCH" && git pull -q --ff-only origin "$BRANCH"
log "building $(git log --oneline -1)"
docker build -q -t bbv-staging . >/dev/null
docker rm -f bbv-staging >/dev/null 2>&1 || true
docker run -d --name bbv-staging -p 8080:8080 -v "$ST:/data" -e ADMIN_TOKEN="${STAGING_ADMIN_TOKEN:-staging}" -e PUBLIC_URL="http://$(hostname -I | awk '{print $1}'):8080" bbv-staging >/dev/null
sleep 3
curl -sS --max-time 20 "http://127.0.0.1:8080/healthz" && echo
log "staging is at http://$(hostname -I | awk '{print $1}'):8080 (admin password: ${STAGING_ADMIN_TOKEN:-staging}). Open the VM's firewall for port 8080 to your own address only."
