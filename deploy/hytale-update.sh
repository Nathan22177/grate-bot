#!/usr/bin/env bash

set -euo pipefail

HYTALE_CHECK_UPDATE_COMMAND="${HYTALE_CHECK_UPDATE_COMMAND:-}"
HYTALE_UPDATE_COMMAND="${HYTALE_UPDATE_COMMAND:-}"

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
  printf '{"timestamp":"%s","source":"hytale-update","stage":"%s","status":"%s","message":"%s"}\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    "$(json_escape "$stage")" \
    "$(json_escape "$status")" \
    "$(json_escape "$message")"
}

run_configured_command() {
  local command="$1"
  local stage="$2"

  if [[ -z "$command" ]]; then
    progress "$stage" failed "no command configured"
    printf 'Set HYTALE_%s_COMMAND for this host.\n' "$(tr '[:lower:]-' '[:upper:]_' <<<"$stage")" >&2
    exit 1
  fi

  progress "$stage" running "running configured command"
  bash -lc "$command"
  progress "$stage" completed "configured command completed"
}

case "${1:-}" in
  check-update)
    run_configured_command "$HYTALE_CHECK_UPDATE_COMMAND" check-update
    ;;
  update)
    run_configured_command "$HYTALE_UPDATE_COMMAND" update
    ;;
  *)
    printf 'Usage: %s {check-update|update}\n' "$0" >&2
    exit 2
    ;;
esac
