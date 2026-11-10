#!/usr/bin/env bash
# Once, on the VM, after `git pull`: puts the uploaded settings file in place, asks for the admin password,
# makes a backup bucket, installs the tools and cron, and runs the first backup. Paste in the SSH window:
#   cd ~/bigboysvault && git pull -q --ff-only origin master && bash ops/first-run.sh
set -euo pipefail
OPS="$(cd "$(dirname "$0")" && pwd)"
if [ -f "$HOME/bbv-ops-env" ]; then mv "$HOME/bbv-ops-env" "$OPS/env"; fi
[ -f "$OPS/env" ] || cp "$OPS/env.example" "$OPS/env"
chmod 600 "$OPS/env"
if ! grep -q '^ADMIN_TOKEN=.\+' "$OPS/env"; then
  read -r -s -p "The site's admin password (ADMIN_TOKEN, the one you type at /admin): " T; echo
  sed -i '/^ADMIN_TOKEN=/d' "$OPS/env"; echo "ADMIN_TOKEN=$T" >> "$OPS/env"
fi
sudo timedatectl set-timezone Asia/Manila 2>/dev/null || true
# A bucket for copies off this VM. Google VMs come with gcloud and a service account; if it can't make one, backups stay on the VM.
if ! grep -q '^GCS_BUCKET=.\+' "$OPS/env" && command -v gcloud >/dev/null; then
  B="bbv-backups-$(date +%s)"
  if gcloud storage buckets create "gs://$B" --location=asia-southeast1 --uniform-bucket-level-access --quiet 2>/dev/null; then
    sed -i '/^GCS_BUCKET=/d' "$OPS/env"; echo "GCS_BUCKET=$B" >> "$OPS/env"; echo "backup bucket: gs://$B"
  else
    echo "couldn't make a bucket (no permission?): backups stay on this VM for now"
  fi
fi
bash "$OPS/setup-vm.sh"
export PATH="$HOME/.fly/bin:$PATH"
echo "--- first backup"; bash "$OPS/backup.sh" | tail -3
echo "--- first uptime check"; bash "$OPS/uptime.sh" && echo "up"
echo "--- pictures (WebP/AVIF) in the background; check ~/bbv/logs/images.log later"
nohup bash "$OPS/images.sh" > "$HOME/bbv/logs/images.log" 2>&1 &
echo "DONE. Jobs run by themselves from now on: crontab -l"
