#!/usr/bin/env bash
# Once, on a fresh Google VM (Debian/Ubuntu): the tools the jobs need, flyctl, and the cron lines.
# Each step says what it's doing and carries on if it can; the summary at the end says what's missing.
set -uo pipefail
OPS="$(cd "$(dirname "$0")" && pwd)"
echo "--- tools (apt)"
sudo apt-get update -qq >/dev/null 2>&1 || echo "apt update had a problem; carrying on"
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq curl git python3 jq webp libavif-bin rsync cron util-linux >/dev/null 2>&1 || echo "apt install had a problem; carrying on"
for t in curl git python3 jq cwebp rsync flock crontab; do command -v "$t" >/dev/null && echo "  $t: ok" || echo "  $t: MISSING"; done
command -v avifenc >/dev/null && echo "  avifenc: ok" || echo "  avifenc: missing (WebP only; fine)"
echo "--- flyctl"
# The jobs find flyctl here by themselves (ops/lib.sh puts ~/.fly/bin on their PATH).
export PATH="$HOME/.fly/bin:$PATH"
if command -v fly >/dev/null; then echo "  fly: ok ($(fly version 2>/dev/null | head -1))"
else
  curl -fsSL https://fly.io/install.sh | sh >/dev/null 2>&1 || echo "  flyctl install failed (pictures and deploys from this VM need it; backups and prices don't)"
  grep -q '.fly/bin' "$HOME/.bashrc" 2>/dev/null || echo 'export PATH="$HOME/.fly/bin:$PATH"' >> "$HOME/.bashrc"
  command -v fly >/dev/null && echo "  fly: installed" || true
fi
chmod +x "$OPS"/*.sh
# A chmod on the VM must never block the nightly `git pull` (ops/update.sh).
git -C "$OPS" config core.fileMode false 2>/dev/null || true
[ -f "$OPS/env" ] || { cp "$OPS/env.example" "$OPS/env"; chmod 600 "$OPS/env"; echo "Now fill in $OPS/env (ADMIN_TOKEN at least), then run: bash $OPS/backup.sh"; }
echo "--- cron (scripts update 02:15, backup 02:30, prices 03:00, pictures 03:30, uptime every 5 minutes)"
if R="$(bash "$OPS/install-cron.sh" 2>&1)"; then
  echo "  $R:"; crontab -l | grep 'bigboysvault/ops' | sed 's/^/  /'
else
  echo "  cron install failed: $R (is cron installed? sudo apt-get install -y cron)"
fi
exit 0
