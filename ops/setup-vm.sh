#!/usr/bin/env bash
# Once, on a fresh Google VM (Debian/Ubuntu): the tools the jobs need, flyctl, and the cron lines.
set -euo pipefail
sudo apt-get update -qq
sudo apt-get install -y -qq curl git python3 jq webp libavif-bin rsync
if ! command -v fly >/dev/null; then curl -L https://fly.io/install.sh | sh; echo 'export PATH="$HOME/.fly/bin:$PATH"' >> "$HOME/.bashrc"; export PATH="$HOME/.fly/bin:$PATH"; fi
OPS="$(cd "$(dirname "$0")" && pwd)"
chmod +x "$OPS"/*.sh
[ -f "$OPS/env" ] || { cp "$OPS/env.example" "$OPS/env"; echo "Now fill in $OPS/env (ADMIN_TOKEN at least), then run: $OPS/backup.sh"; }
# Cron: nightly backup and pictures, prices at 3 am, uptime every 5 minutes. Edit with: crontab -e
( crontab -l 2>/dev/null | grep -v 'bigboysvault/ops' ; sed "s#OPS#$OPS#g" "$OPS/crontab.example" ) | crontab -
echo "cron installed:"; crontab -l | grep "$OPS"
