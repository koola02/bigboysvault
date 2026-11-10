#!/usr/bin/env bash
# Once, on a fresh Google VM (Debian/Ubuntu): the tools the jobs need, flyctl, and the cron lines.
# Each step says what it's doing and carries on if it can; the summary at the end says what's missing.
set -uo pipefail
OPS="$(cd "$(dirname "$0")" && pwd)"
echo "--- tools (apt)"
sudo apt-get update -qq >/dev/null 2>&1 || echo "apt update had a problem; carrying on"
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq curl git python3 jq webp libavif-bin rsync >/dev/null 2>&1 || echo "apt install had a problem; carrying on"
for t in curl git python3 jq cwebp rsync; do command -v "$t" >/dev/null && echo "  $t: ok" || echo "  $t: MISSING"; done
command -v avifenc >/dev/null && echo "  avifenc: ok" || echo "  avifenc: missing (WebP only; fine)"
echo "--- flyctl"
export PATH="$HOME/.fly/bin:$PATH"
if command -v fly >/dev/null; then echo "  fly: ok ($(fly version 2>/dev/null | head -1))"
else
  curl -fsSL https://fly.io/install.sh | sh >/dev/null 2>&1 || echo "  flyctl install failed (pictures and deploys from this VM need it; backups and prices don't)"
  grep -q '.fly/bin' "$HOME/.bashrc" 2>/dev/null || echo 'export PATH="$HOME/.fly/bin:$PATH"' >> "$HOME/.bashrc"
  command -v fly >/dev/null && echo "  fly: installed" || true
fi
chmod +x "$OPS"/*.sh
[ -f "$OPS/env" ] || { cp "$OPS/env.example" "$OPS/env"; echo "Now fill in $OPS/env (ADMIN_TOKEN at least), then run: $OPS/backup.sh"; }
echo "--- cron (nightly backup and pictures, prices at 3 am, uptime every 5 minutes)"
mkdir -p "$HOME/bbv/logs"
( crontab -l 2>/dev/null | grep -v 'bigboysvault/ops' ; sed "s#OPS#$OPS#g" "$OPS/crontab.example" ) | crontab - && echo "  installed:" && crontab -l | grep "$OPS" | sed 's/^/  /' || echo "  cron install failed: is cron installed? (sudo apt-get install -y cron)"
exit 0
