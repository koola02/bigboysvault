#!/usr/bin/env bash
# Shared bits for the VM's jobs. Every script does: . "$(dirname "$0")/lib.sh"
# -E: the error catcher below also works inside functions. A failing job always tells someone (see on_exit).
set -Eeuo pipefail
# cron starts jobs with a very short PATH. flyctl lives in ~/.fly/bin, so put that first.
export PATH="$HOME/.fly/bin:/usr/local/bin:/usr/bin:/bin:$PATH"
OPS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="${BBV_ENV:-$OPS_DIR/env}"
# KEY=value lines, read as text (never run as shell), so tokens with spaces or commas are fine. Quotes around a value are optional.
if [ -f "$ENV_FILE" ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    line="${line%$'\r'}"
    case "$line" in ''|'#'*) continue ;; esac
    key="${line%%=*}"; val="${line#*=}"
    val="${val#\"}"; val="${val%\"}"; val="${val#\'}"; val="${val%\'}"
    case "$key" in ''|*[!A-Za-z0-9_]*) continue ;; esac
    export "$key=$val"
  done < "$ENV_FILE"
fi
# The job's name, used in messages and in the report to the site: backup, prices, images, uptime, update.
: "${JOB:=$(basename "$0" .sh)}"
: "${SITE:=https://www.bigboysvault.com}"
: "${FLY_APP:=bigboysvault}"
# The site's own Fly address. It skips Cloudflare, so big downloads go straight to the site.
: "${ORIGIN:=https://$FLY_APP.fly.dev}"
: "${DATA_DIR:=$HOME/bbv}"
LOG_DIR="$DATA_DIR/logs"
ALERTED=""

log() { printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*"; }

# Sends a message to a person: Telegram (TELEGRAM_BOT_TOKEN + TELEGRAM_CHAT_ID) and/or a Discord/Slack-style
# webhook (ALERT_WEBHOOK). Does nothing when neither is set. Never fails.
send() {
  local msg="$1" payload
  if [ -n "${TELEGRAM_BOT_TOKEN:-}" ] && [ -n "${TELEGRAM_CHAT_ID:-}" ]; then
    curl -sS --max-time 20 -o /dev/null "https://api.telegram.org/bot$TELEGRAM_BOT_TOKEN/sendMessage" --data-urlencode "chat_id=$TELEGRAM_CHAT_ID" --data-urlencode "text=$msg" || true
  fi
  if [ -n "${ALERT_WEBHOOK:-}" ]; then
    payload="$(printf '%s' "$msg" | python3 -c 'import json,sys; m=sys.stdin.read(); print(json.dumps({"content": m, "text": m}))' 2>/dev/null)" || return 0
    curl -sS --max-time 20 -o /dev/null -H 'Content-Type: application/json' -d "$payload" "$ALERT_WEBHOOK" || true
  fi
  return 0
}

# Tells the site how a job went: POST /admin/ops/report {"job","ok","note"}. The admin dashboard shows it, and a
# failure rings the admins' bell. Tries the normal address, then the Fly address. Never fails, never stops a job.
# Usage: report <job> <true|false> "<note>"     (REPORT=0 turns it off, for scripts that are not nightly jobs)
report() {
  local job="$1" ok="$2" note="${3:-}" body u
  [ "${REPORT:-1}" = 1 ] && [ -n "${ADMIN_TOKEN:-}" ] || return 0
  body="$(python3 -c 'import json,sys; print(json.dumps({"job": sys.argv[1], "ok": sys.argv[2] == "true", "note": sys.argv[3][:300]}))' "$job" "$ok" "$note" 2>/dev/null)" || return 0
  for u in "$SITE" "$ORIGIN"; do
    if curl -sS --fail --max-time 20 -o /dev/null -X POST -H "Cookie: marcuru_admin=$ADMIN_TOKEN" -H 'Content-Type: application/json' --data-binary "$body" "$u/admin/ops/report" 2>/dev/null; then
      return 0
    fi
  done
  log "could not tell the site about $job (ok=$ok)"
  return 0
}

# Something went wrong: the log, Telegram/webhook, and the site (so admins see it even with no Telegram set).
alert() {
  local msg="$1"
  ALERTED=1
  log "ALERT: $msg"
  send "Big Boys Vault ($JOB): $msg"
  report "$JOB" false "$msg"
  return 0
}

# The job finished well: tell the site. Skipped when the job already raised an alert, so the warning stays up.
done_ok() {
  [ -n "$ALERTED" ] || report "$JOB" true "$1"
  return 0
}

# Any job that stops with an error (set -e, a failed check, `exit 1`) raises an alert on the way out.
FAIL_AT=""
trap 'FAIL_AT="line $LINENO: ${BASH_COMMAND:0:60}"' ERR
on_exit() {
  local rc=$?
  if [ "$rc" -ne 0 ] && [ -z "$ALERTED" ]; then
    alert "$JOB stopped with an error (exit $rc${FAIL_AT:+, $FAIL_AT}). See $LOG_DIR/$JOB.log on the VM."
  fi
}
trap on_exit EXIT

: "${ADMIN_TOKEN:?Set ADMIN_TOKEN in ops/env (the admin password you gave fly secrets)}"
mkdir -p "$DATA_DIR" "$LOG_DIR"

# Only one copy of a job at a time (a slow night must not overlap the next run).
one_at_a_time() {
  exec 9>"$DATA_DIR/$JOB.lock"
  if ! flock -n 9; then log "$JOB is already running; this run stops here"; exit 0; fi
}

# How full this VM's disk is, in percent (just the number).
disk_pct() { df -P "$DATA_DIR" | awk 'NR==2 { gsub("%", "", $5); print $5 }'; }

# Keeps every log under 5 MB: a big one is cut down to its last 5000 lines (same file, so cron keeps writing to it).
trim_logs() {
  local f
  for f in "$LOG_DIR"/*.log; do
    [ -f "$f" ] || continue
    if [ "$(stat -c %s "$f")" -gt 5242880 ]; then
      tail -n 5000 "$f" > "$f.tmp" && cat "$f.tmp" > "$f"
      rm -f "$f.tmp"
    fi
  done
  return 0
}

# Free space on the site's /data volume, in KB (empty when Fly can't be reached).
fly_free_kb() {
  fly ssh console -q -a "$FLY_APP" -C "df -Pk /data" 2>/dev/null | tr -d '\r' | awk '$NF == "/data" { print $4 }' | tail -1 || true
}

# The admin cookie the VM's jobs send (the owner's token, kept in ops/env).
admin_curl() { curl -sS --fail-with-body --max-time "${CURL_TIMEOUT:-600}" -H "Cookie: marcuru_admin=$ADMIN_TOKEN" -H "Accept: application/json" "$@"; }

json_get() { python3 -c 'import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1]))' "$1"; }

# No way to reach a person? Say so in the log (not for uptime, which runs every 5 minutes).
if [ "$JOB" != uptime ] && [ -z "${TELEGRAM_BOT_TOKEN:-}${ALERT_WEBHOOK:-}" ]; then
  log "note: no Telegram or webhook in ops/env, so problems only show on the admin dashboard and bell"
fi
