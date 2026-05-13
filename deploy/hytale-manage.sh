#!/usr/bin/env bash

set -euo pipefail

SERVICE_NAME="${SERVICE_NAME:-hytale-server.service}"
DOWNLOAD_TIMEOUT_SECONDS="${DOWNLOAD_TIMEOUT_SECONDS:-1800}"
START_TIMEOUT_SECONDS="${START_TIMEOUT_SECONDS:-120}"
START_STABLE_SECONDS="${START_STABLE_SECONDS:-10}"
HYTALE_UPDATE_SCRIPT="${HYTALE_UPDATE_SCRIPT:-}"
HYTALE_DIR="${HYTALE_DIR:-$HOME/hytale}"
HYTALE_PORT="${HYTALE_PORT:-5520}"
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

print_service_diagnostics() {
  sudo -n systemctl status "$SERVICE_NAME" --no-pager || true
  sudo -n journalctl -u "$SERVICE_NAME" -n "$LOG_LINES" --no-pager || true
}

print_diagnostics() {
  printf '== service ==\n'
  sudo -n systemctl status "$SERVICE_NAME" --no-pager || true

  printf '\n== udp listeners ==\n'
  if command -v ss >/dev/null 2>&1; then
    if ! sudo -n ss -H -lunp | grep -E "(:|\\*)${HYTALE_PORT}\\b|hytale|java"; then
      printf 'No UDP listener matched port %s, hytale, or java.\n' "$HYTALE_PORT"
      printf 'All UDP listeners:\n'
      sudo -n ss -H -lunp || true
    fi
  else
    printf 'ss is not installed; cannot inspect UDP listeners.\n'
  fi

  printf '\n== hytale config hints ==\n'
  if [[ -f "$HYTALE_DIR/Server/config.json" ]]; then
    grep -E '"(bind|port|address|host)"' "$HYTALE_DIR/Server/config.json" || true
  else
    printf 'No config file found at %s/Server/config.json\n' "$HYTALE_DIR"
  fi

  printf '\n== recent logs ==\n'
  sudo -n journalctl -u "$SERVICE_NAME" -n "$LOG_LINES" --no-pager || true

  printf '\n== quic note ==\n'
  printf 'Hytale client connections use QUIC over UDP. If the service is active and listening, verify host firewall and cloud security rules allow UDP %s inbound to this server.\n' "$HYTALE_PORT"
}

wait_for_service_ready() {
  local stage="$1"
  local waited=0
  local stable=0

  while (( waited < START_TIMEOUT_SECONDS )); do
    if systemctl is-failed --quiet "$SERVICE_NAME"; then
      progress "$stage" failed "$SERVICE_NAME entered failed state"
      print_service_diagnostics
      return 1
    fi

    if systemctl is-active --quiet "$SERVICE_NAME"; then
      stable=$((stable + 2))
      if (( stable >= START_STABLE_SECONDS )); then
        progress "$stage" completed "$SERVICE_NAME stayed active for ${START_STABLE_SECONDS}s"
        return 0
      fi
    else
      stable=0
    fi

    sleep 2
    waited=$((waited + 2))
  done

  progress "$stage" failed "$SERVICE_NAME did not stay active within ${START_TIMEOUT_SECONDS}s"
  print_service_diagnostics
  return 1
}

action="${1:-}"

case "$action" in
  status)
    sudo -n systemctl status "$SERVICE_NAME" --no-pager
    ;;
  logs)
    sudo -n journalctl -u "$SERVICE_NAME" -n "$LOG_LINES" --no-pager
    ;;
  diagnose)
    print_diagnostics
    ;;
  start)
    progress start running "starting $SERVICE_NAME"
    sudo -n systemctl start "$SERVICE_NAME"
    wait_for_service_ready start
    ;;
  stop)
    progress stop running "stopping $SERVICE_NAME"
    sudo -n systemctl stop "$SERVICE_NAME"
    progress stop completed "$SERVICE_NAME stopped"
    ;;
  restart)
    progress restart running "restarting $SERVICE_NAME"
    sudo -n systemctl restart "$SERVICE_NAME"
    wait_for_service_ready restart
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
    wait_for_service_ready update
    ;;
  *)
    printf 'Usage: %s {status|logs|diagnose|start|stop|restart|check-update|update}\n' "$0" >&2
    exit 2
    ;;
esac
