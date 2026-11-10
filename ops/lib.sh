#!/usr/bin/env bash
# Shared bits for the VM's jobs. Every script does: source "$(dirname "$0")/lib.sh"
set -euo pipefail
OPS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="${BBV_ENV:-$OPS_DIR/env}"
# KEY=value lines, read as text (never run as shell), so tokens with spaces or commas are fine. Quotes around a value are optional.
if [ -f "$ENV_FILE" ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in ''|'#'*) continue ;; esac
    key="${line%%=*}"; val="${line#*=}"
    val="${val#\"}"; val="${val%\"}"; val="${val#'}"; val="${val%'}"
    case "$key" in *[!A-Za-z0-9_]*) continue ;; esac
    export "$key=$val"
  done < "$ENV_FILE"
fi
: "${SITE:=https://www.bigboysvault.com}"
: "${ADMIN_TOKEN:?Set ADMIN_TOKEN in ops/env (the admin password you gave fly secrets)}"
: "${DATA_DIR:=$HOME/bbv}"
LOG_DIR="$DATA_DIR/logs"
mkdir -p "$DATA_DIR" "$LOG_DIR"

log() { printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*"; }

# The owner's admin cookie, the same one the browser gets after signing in at /admin.
admin_curl() { curl -sS --fail-with-body --max-time "${CURL_TIMEOUT:-600}" -H "Cookie: marcuru_admin=$ADMIN_TOKEN" -H "Accept: application/json" "$@"; }

# Tells you something: Telegram (TELEGRAM_BOT_TOKEN + TELEGRAM_CHAT_ID), a Discord/Slack-style webhook
# (ALERT_WEBHOOK), or just the log.
alert() {
  local msg="$1"
  log "ALERT: $msg"
  if [ -n "${TELEGRAM_BOT_TOKEN:-}" ] && [ -n "${TELEGRAM_CHAT_ID:-}" ]; then
    curl -sS --max-time 20 -o /dev/null "https://api.telegram.org/bot$TELEGRAM_BOT_TOKEN/sendMessage" --data-urlencode "chat_id=$TELEGRAM_CHAT_ID" --data-urlencode "text=$msg" || true
  fi
  if [ -n "${ALERT_WEBHOOK:-}" ]; then
    local payload
    payload="$(printf '%s' "$msg" | python3 -c 'import json,sys; m=sys.stdin.read(); print(json.dumps({"content": m, "text": m}))')"
    curl -sS --max-time 20 -o /dev/null -H 'Content-Type: application/json' -d "$payload" "$ALERT_WEBHOOK" || true
  fi
}

json_get() { python3 -c 'import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1]))' "$1"; }
