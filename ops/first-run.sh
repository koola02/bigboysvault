#!/usr/bin/env bash
# Once, on the VM, after `git pull`: puts the uploaded settings file in place, asks for the admin password (and,
# if you like, a Telegram bot), makes a backup bucket, installs the tools and cron, and runs the first backup.
# Paste in the SSH window:
#   cd ~/bigboysvault && git pull -q --ff-only origin master && bash ops/first-run.sh
set -uo pipefail
OPS="$(cd "$(dirname "$0")" && pwd)"
if [ -f "$HOME/bbv-ops-env" ]; then mv "$HOME/bbv-ops-env" "$OPS/env"; fi
[ -f "$OPS/env" ] || cp "$OPS/env.example" "$OPS/env"
chmod 600 "$OPS/env"
# Sets KEY=value in ops/env (replacing an old line).
put_env() { sed -i "/^$1=/d" "$OPS/env"; printf '%s=%s\n' "$1" "$2" >> "$OPS/env"; }
if ! grep -q '^ADMIN_TOKEN=.\+' "$OPS/env"; then
  echo "It must be the same as the site's (fly secrets). For a new one, long and random: openssl rand -hex 24"
  read -r -s -p "The site's admin password (ADMIN_TOKEN, the one you type at /admin): " T; echo
  put_env ADMIN_TOKEN "$T"
fi
if ! grep -q '^TELEGRAM_BOT_TOKEN=.\+' "$OPS/env" && ! grep -q '^ALERT_WEBHOOK=.\+' "$OPS/env"; then
  echo "Problems always show on the admin dashboard. For a message on your phone too, make a Telegram bot"
  echo "(@BotFather gives the token, @userinfobot gives your chat id). Press Enter to skip."
  read -r -p "Telegram bot token: " TB
  if [ -n "$TB" ]; then
    read -r -p "Your Telegram chat id: " TC
    put_env TELEGRAM_BOT_TOKEN "$TB"; put_env TELEGRAM_CHAT_ID "$TC"
  fi
fi
sudo timedatectl set-timezone Asia/Manila 2>/dev/null || true
# A bucket for copies off this VM. Google VMs come with gcloud and a service account; if it can't make one, backups stay on the VM.
if ! grep -q '^GCS_BUCKET=.\+' "$OPS/env" && command -v gcloud >/dev/null; then
  B="bbv-backups-$(date +%s)"
  if gcloud storage buckets create "gs://$B" --location=asia-southeast1 --uniform-bucket-level-access --quiet 2>/dev/null; then
    put_env GCS_BUCKET "$B"; echo "backup bucket: gs://$B"
  else
    echo "couldn't make a bucket (no permission?): backups stay on this VM for now"
  fi
fi
bash "$OPS/setup-vm.sh"
if grep -q '^TELEGRAM_BOT_TOKEN=.\+' "$OPS/env"; then
  bash -c ". '$OPS/lib.sh' && send 'Big Boys Vault: alerts from the VM are working.'" && echo "test message sent to Telegram"
fi
LOGS="$HOME/bbv/logs"; mkdir -p "$LOGS"
echo "--- first backup"; bash "$OPS/backup.sh" 2>&1 | tail -4; [ "${PIPESTATUS[0]}" = 0 ] || echo "backup failed: see the lines above (often ADMIN_TOKEN in $OPS/env)"
echo "--- first uptime check"; bash "$OPS/uptime.sh" 2>&1 | tail -2; echo "up"
echo "--- pictures (WebP/AVIF) in the background; check $LOGS/images.log later"
nohup bash "$OPS/images.sh" >> "$LOGS/images.log" 2>&1 &
echo "DONE. Jobs run by themselves from now on: crontab -l"
