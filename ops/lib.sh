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

# flyctl, always through this: with a time limit, and without the job's lock (fd 9). flyctl starts a helper program
# (its "agent") that keeps running in the background after the job ends; started straight from a job, the helper
# kept that job's lock. That is what stopped the picture job from 14 to 17 Nov 2026: every night it found its own
# lock taken, said "already running" and quietly stopped, while the only thing holding it was that helper.
FLYCTL_BIN="$(command -v flyctl || command -v fly || true)"
fly() {
  if [ -z "$FLYCTL_BIN" ]; then log "flyctl is not on this VM (expected ~/.fly/bin/flyctl)"; return 127; fi
  # </dev/null: under `timeout`, flyctl isn't in the terminal's foreground, so a run started by hand would freeze
  # the moment flyctl looked at the keyboard (seen 17 Nov 2026). No job ever types into flyctl.
  timeout -k 30 "${FLY_TIMEOUT:-1200}" "$FLYCTL_BIN" "$@" 9>&- </dev/null
}

# Only one copy of a job at a time (a slow night must not overlap the next run). The lock file also says which run
# holds it and since when, so a run that is truly stuck is reported instead of every later run stopping in silence.
one_at_a_time() {
  local lock="$DATA_DIR/$JOB.lock" held now since
  clear_leftover_lock "$lock"
  held="$(cat "$lock" 2>/dev/null || true)"
  exec 9>>"$lock"   # append: opening it must not wipe the note of a run that holds it
  if ! flock -n 9; then
    since="${held##* }"; now="$(date +%s)"
    if [[ "$since" =~ ^[0-9]+$ ]] && [ $((now - since)) -gt $(( ${LONG_RUN_HOURS:-6} * 3600 )) ]; then
      alert "$JOB has been running since $(date -d "@$since" '+%b %d %H:%M') and seems stuck (process ${held%% *}), so tonight's run could not start. Stop it on the VM: kill ${held%% *}"
      exit 0
    fi
    log "$JOB is already running; this run stops here"
    exit 0
  fi
  printf '%s %s\n' "$$" "$(date +%s)" > "$lock"
}

# Before taking the lock: if no run of this job is alive but something still holds its lock (a helper program an
# earlier run started, or a leftover child of a run that was killed), that holder is stopped and the job goes on.
# A real run (bash running ops/<job>.sh) is never touched.
clear_leftover_lock() {
  local lock="$1" pids="" real="" pid cmd
  [ -e "$lock" ] || return 0
  for pid in $( { find /proc/[0-9]*/fd -maxdepth 1 -lname "$lock" 2>/dev/null || true; } | cut -d/ -f3 | sort -u); do
    cmd="$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null || true)"
    case "$cmd" in *"$JOB.sh"*) real=1 ;; *) pids="$pids $pid" ;; esac
  done
  [ -z "$real" ] && [ -n "$pids" ] || return 0
  for pid in $pids; do log "the lock was held by a leftover program, not by a run of $JOB; stopping it: $pid $(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null | cut -c1-120)"; done
  # shellcheck disable=SC2086
  kill $pids 2>/dev/null || true
  sleep 2
  # shellcheck disable=SC2086
  kill -9 $pids 2>/dev/null || true
  return 0
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
  FLY_TIMEOUT=120 fly ssh console -q -a "$FLY_APP" -C "df -Pk /data" 2>/dev/null | tr -d '\r' | awk '$NF == "/data" { print $4 }' | tail -1 || true
}

# The admin cookie the VM's jobs send (the owner's token, kept in ops/env).
admin_curl() { curl -sS --fail-with-body --max-time "${CURL_TIMEOUT:-600}" -H "Cookie: marcuru_admin=$ADMIN_TOKEN" -H "Accept: application/json" "$@"; }

json_get() { python3 -c 'import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1]))' "$1"; }

# No way to reach a person? Say so in the log (not for uptime, which runs every 5 minutes).
if [ "$JOB" != uptime ] && [ -z "${TELEGRAM_BOT_TOKEN:-}${ALERT_WEBHOOK:-}" ]; then
  log "note: no Telegram or webhook in ops/env, so problems only show on the admin dashboard and bell"
fi
