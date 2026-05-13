#!/usr/bin/env bash

set -euo pipefail

SERVICE_NAME="${SERVICE_NAME:-hytale-server.service}"
DOWNLOAD_TIMEOUT_SECONDS="${DOWNLOAD_TIMEOUT_SECONDS:-1800}"
HYTALE_UPDATE_SCRIPT="${HYTALE_UPDATE_SCRIPT:-}"
LOG_LINES="${LOG_LINES:-80}"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if [[ -z "$HYTALE_UPDATE_SCRIPT" ]]; then
  HYTALE_UPDATE_SCRIPT="$script_dir/hytale-update.sh"
fi

json_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  value="${value//$'\r'/\\r}"
  value="${value//$'\t'/\\t}"
  printf '%s' "$value"
}

progress() {
  local stage="$1"
  local status="$2"
  local message="$3"
  printf '{"timestamp":"%s","source":"hytale-manage","stage":"%s","status":"%s","message":"%s"}\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    "$(json_escape "$stage")" \
    "$(json_escape "$status")" \
    "$(json_escape "$message")"
}

run_with_timeout() {
  local seconds="$1"
  shift

  if command -v timeout >/dev/null 2>&1; then
    timeout "$seconds" "$@"
  else
    "$@"
  fi
}

need_update_script() {
  [[ -x "$HYTALE_UPDATE_SCRIPT" ]] || {
    progress "$1" failed "update script is not executable: $HYTALE_UPDATE_SCRIPT"
    exit 1
  }
}

action="${1:-}"

case "$action" in
  status)
    sudo -n systemctl status "$SERVICE_NAME" --no-pager
    ;;
  logs)
    journalctl -u "$SERVICE_NAME" -n "$LOG_LINES" --no-pager
    ;;
  start)
    progress start running "starting $SERVICE_NAME"
    sudo -n systemctl start "$SERVICE_NAME"
    progress start completed "$SERVICE_NAME started"
    ;;
  stop)
    progress stop running "stopping $SERVICE_NAME"
    sudo -n systemctl stop "$SERVICE_NAME"
    progress stop completed "$SERVICE_NAME stopped"
    ;;
  restart)
    progress restart running "restarting $SERVICE_NAME"
    sudo -n systemctl restart "$SERVICE_NAME"
    progress restart completed "$SERVICE_NAME restarted"
    ;;
  check-update)
    need_update_script check-update
    progress check-update running "checking for Hytale server updates"
    run_with_timeout "$DOWNLOAD_TIMEOUT_SECONDS" "$HYTALE_UPDATE_SCRIPT" check-update
    progress check-update completed "Hytale update check completed"
    ;;
  update)
    need_update_script update
    was_active=0
    if systemctl is-active --quiet "$SERVICE_NAME"; then
      was_active=1
    fi

    if [[ "$was_active" == "1" ]]; then
      progress update running "stopping $SERVICE_NAME before update"
      sudo -n systemctl stop "$SERVICE_NAME"
    else
      progress update running "$SERVICE_NAME is not active before update"
    fi

    progress update running "running updater"
    if ! run_with_timeout "$DOWNLOAD_TIMEOUT_SECONDS" "$HYTALE_UPDATE_SCRIPT" update; then
      progress update failed "updater failed"
      if [[ "$was_active" == "1" ]]; then
        progress update running "attempting to restart $SERVICE_NAME after failed update"
        sudo -n systemctl start "$SERVICE_NAME"
      fi
      exit 1
    fi

    progress update running "starting $SERVICE_NAME after update"
    sudo -n systemctl start "$SERVICE_NAME"
    progress update completed "$SERVICE_NAME updated and started"
    ;;
  *)
    printf 'Usage: %s {status|logs|start|stop|restart|check-update|update}\n' "$0" >&2
    exit 2
    ;;
esac
