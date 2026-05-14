#!/usr/bin/env bash

set -euo pipefail

PATCHLINE="${PATCHLINE:-pre-release}"
HTYALE_DIR="${HYTALE_DIR:-${HTYALE_DIR:-$HOME/hytale}}"
BACKUP_DIR="${BACKUP_DIR:-$HOME/hytale-backups}"
AMD64_SOURCES_FILE="${AMD64_SOURCES_FILE:-/etc/apt/sources.list.d/amd64-temp.list}"
SERVICE_NAME="${SERVICE_NAME:-hytale-server}"
AUTO_START_SERVICE="${AUTO_START_SERVICE:-0}"
START_TIMEOUT_SECONDS="${START_TIMEOUT_SECONDS:-120}"
DOWNLOAD_TIMEOUT_SECONDS="${DOWNLOAD_TIMEOUT_SECONDS:-1800}"
STATE_DIR="${STATE_DIR:-$HTYALE_DIR/.bot-status}"
STATE_FILE="${STATE_FILE:-$STATE_DIR/hytale-update.status}"
LAST_STAGE_FILE="${LAST_STAGE_FILE:-$STATE_DIR/hytale-update.last}"

CURRENT_STAGE="init"
BACKUP_ARCHIVE_PATH=""

log() {
  printf '[hytale-update] %s\n' "$*"
}

sudo_cmd() {
  sudo -n "$@"
}

json_escape() {
  local value="${1//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  value="${value//$'\r'/\\r}"
  value="${value//$'\t'/\\t}"
  printf '%s' "$value"
}

report_stage() {
  local stage="$1"
  local status="$2"
  local message="$3"
  local ts
  local json
  ts="$(date -u +%FT%TZ)"

  CURRENT_STAGE="$stage"
  mkdir -p "$STATE_DIR"
  json=$(printf '{"timestamp":"%s","source":"hytale-update","stage":"%s","status":"%s","message":"%s"}' \
    "$ts" "$(json_escape "$stage")" "$(json_escape "$status")" "$(json_escape "$message")")
  printf '%s\n' "$json" | tee -a "$STATE_FILE"
  printf '%s\n' "$json" >"$LAST_STAGE_FILE"
}

fail() {
  report_stage "$CURRENT_STAGE" failed "$1"
  exit 1
}

on_error() {
  local exit_code=$?
  if (( exit_code != 0 )); then
    report_stage "$CURRENT_STAGE" failed "command failed with exit code $exit_code"
  fi
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    fail "missing required command: $1"
  fi
}

ensure_executable() {
  local path="$1"

  if [[ -x "$path" ]]; then
    return
  fi

  if chmod +x "$path" 2>/dev/null; then
    return
  fi

  sudo_cmd chmod +x "$path"
}

service_active() {
  systemctl is-active --quiet "$SERVICE_NAME"
}

wait_for_active() {
  local waited=0
  while (( waited < START_TIMEOUT_SECONDS )); do
    if service_active; then
      return 0
    fi
    sleep 2
    waited=$((waited + 2))
  done
  return 1
}

ensure_temp_amd64_sources() {
  if sudo_cmd test -f "$AMD64_SOURCES_FILE"; then
    return
  fi

  report_stage runtime running "writing temporary amd64 apt sources"
  sudo_cmd tee "$AMD64_SOURCES_FILE" >/dev/null <<'EOF'
deb [arch=amd64] http://archive.ubuntu.com/ubuntu noble main universe multiverse restricted
deb [arch=amd64] http://archive.ubuntu.com/ubuntu noble-updates main universe multiverse restricted
deb [arch=amd64] http://security.ubuntu.com/ubuntu noble-security main universe multiverse restricted
deb [arch=amd64] http://archive.ubuntu.com/ubuntu noble-backports main universe multiverse restricted
EOF
}

ensure_downloader_runtime() {
  require_cmd sudo
  require_cmd curl
  require_cmd unzip
  require_cmd timeout
  require_cmd tee
  require_cmd apt-get

  report_stage runtime running "checking amd64 downloader runtime"
  local foreign_arches
  local apt_log
  foreign_arches="$(dpkg --print-foreign-architectures || true)"
  if ! grep -qx 'amd64' <<<"$foreign_arches"; then
    log "Enabling amd64 as a temporary foreign architecture"
    sudo_cmd dpkg --add-architecture amd64
  fi

  ensure_temp_amd64_sources

  report_stage runtime running "refreshing apt metadata"
  apt_log="$(mktemp)"
  if ! sudo_cmd apt-get update >"$apt_log" 2>&1; then
    log "apt update reported expected amd64 index errors from ubuntu-ports; continuing"
    report_stage runtime running "apt update reported expected amd64 index errors; continuing"
  fi
  rm -f "$apt_log"

  report_stage runtime running "installing downloader runtime dependencies"
  apt_log="$(mktemp)"
  if ! sudo_cmd apt-get install -y qemu-user-static libc6:amd64 libstdc++6:amd64 zlib1g:amd64 >"$apt_log" 2>&1; then
    tail -n 20 "$apt_log" >&2 || true
    rm -f "$apt_log"
    fail "failed to install downloader runtime dependencies"
  fi
  rm -f "$apt_log"
  report_stage runtime completed "downloader runtime is ready"
}

download_downloader() {
  cd "$HTYALE_DIR"
  report_stage downloader running "ensuring Hytale downloader is present"

  if [[ ! -f hytale-downloader-linux-amd64 ]]; then
    log "Downloading Hytale downloader bundle"
    curl -L -o hytale-downloader.zip https://downloader.hytale.com/hytale-downloader.zip
    unzip -o hytale-downloader.zip
  fi

  ensure_executable hytale-downloader-linux-amd64
  report_stage downloader completed "downloader is ready"
}

backup_existing_install() {
  mkdir -p "$BACKUP_DIR"

  local stamp
  stamp="$(date +%F-%H%M%S)"
  BACKUP_ARCHIVE_PATH="$BACKUP_DIR/hytale-state-$stamp.tar.gz"

  local items=()
  for item in Server/config.json Server/config.json.bak Server/permissions.json Server/bans.json Server/whitelist.json universe mods auth.enc; do
    if [[ -e "$HTYALE_DIR/$item" ]]; then
      items+=("$item")
    fi
  done

  if [[ ${#items[@]} -eq 0 ]]; then
    report_stage backup completed "no mutable server state found to back up"
    BACKUP_ARCHIVE_PATH=""
    return
  fi

  report_stage backup running "creating mutable-state backup at $BACKUP_ARCHIVE_PATH"
  tar -C "$HTYALE_DIR" -czf "$BACKUP_ARCHIVE_PATH" "${items[@]}"
  report_stage backup completed "backup created at $BACKUP_ARCHIVE_PATH"
}

run_downloader() {
  cd "$HTYALE_DIR"
  report_stage download running "launching downloader for patchline $PATCHLINE"

  local download_log
  local download_pid
  local auth_reported=0
  local exit_code=0
  download_log="$(mktemp)"

  (
    set -o pipefail
    timeout --foreground "${DOWNLOAD_TIMEOUT_SECONDS}s" \
      ./hytale-downloader-linux-amd64 -patchline "$PATCHLINE" 2>&1 | tee "$download_log"
  ) &
  download_pid=$!

  while kill -0 "$download_pid" 2>/dev/null; do
    if (( auth_reported == 0 )) && grep -q 'Please visit the following URL to authenticate:' "$download_log"; then
      auth_reported=1
      report_stage auth waiting "waiting for Hytale device authorization to complete"
    fi
    sleep 2
  done

  if ! wait "$download_pid"; then
    exit_code=$?
    rm -f "$download_log"
    if (( exit_code == 124 )); then
      fail "downloader timed out after ${DOWNLOAD_TIMEOUT_SECONDS}s"
    fi
    fail "downloader failed with exit code $exit_code"
  fi

  rm -f "$download_log"
  report_stage download completed "downloader finished"
}

local_server_version() {
  local version

  if [[ -f "$HTYALE_DIR/Server/HytaleServer.jar" ]]; then
    version="$(unzip -p "$HTYALE_DIR/Server/HytaleServer.jar" META-INF/MANIFEST.MF 2>/dev/null \
      | extract_server_version)"
    if [[ -n "$version" ]]; then
      printf '%s\n' "$version"
      return
    fi
  fi
}

server_version_pattern() {
  printf '%s' '20[0-9]{2}\.[0-9]{2}\.[0-9]{2}-[0-9A-Za-z][0-9A-Za-z.-]*|[0-9]+(\.[0-9]+){2}(-[0-9A-Za-z][0-9A-Za-z.-]*)?'
}

extract_server_version() {
  grep -Eo "$(server_version_pattern)" | tail -n 1 || true
}

latest_server_version() {
  local output
  local version

  cd "$HTYALE_DIR"
  output="$(timeout --foreground "${DOWNLOAD_TIMEOUT_SECONDS}s" \
    ./hytale-downloader-linux-amd64 -patchline "$PATCHLINE" -print-version)"
  version="$(extract_server_version <<<"$output")"
  if [[ -z "$version" ]]; then
    printf '%s\n' "$output" >&2
    fail "could not parse latest Hytale server version"
  fi
  printf '%s\n' "$version"
}

latest_server_zip() {
  find "$HTYALE_DIR" -maxdepth 1 -type f -name '*.zip' ! -name 'hytale-downloader.zip' -printf '%T@ %f\n' \
    | sort -nr \
    | awk 'NR==1 { print $2 }'
}

extract_latest_zip() {
  local zip_name
  zip_name="$(latest_server_zip)"

  if [[ -z "$zip_name" ]]; then
    fail "no downloaded server zip found in $HTYALE_DIR"
  fi

  report_stage extract running "extracting $zip_name"
  unzip -o "$HTYALE_DIR/$zip_name" -d "$HTYALE_DIR"
  ensure_executable "$HTYALE_DIR/start.sh"
  report_stage extract completed "extracted $zip_name"
}

restore_server_state() {
  if [[ -z "$BACKUP_ARCHIVE_PATH" || ! -f "$BACKUP_ARCHIVE_PATH" ]]; then
    report_stage restore completed "no mutable-state backup to restore"
    return
  fi

  report_stage restore running "restoring mutable server state from $(basename "$BACKUP_ARCHIVE_PATH")"
  tar -C "$HTYALE_DIR" -xzf "$BACKUP_ARCHIVE_PATH"
  ensure_executable "$HTYALE_DIR/start.sh"
  report_stage restore completed "mutable server state restored"
}

start_service_if_requested() {
  if [[ "$AUTO_START_SERVICE" != "1" ]]; then
    report_stage start skipped "auto service restart disabled"
    return
  fi

  require_cmd systemctl
  report_stage start running "starting $SERVICE_NAME via systemctl"
  sudo_cmd systemctl start "$SERVICE_NAME"
  if wait_for_active; then
    report_stage start completed "$SERVICE_NAME is active after systemctl start"
    return
  fi

  report_stage start failed "$SERVICE_NAME did not become active within ${START_TIMEOUT_SECONDS}s"
  systemctl status "$SERVICE_NAME" --no-pager || true
  sudo_cmd journalctl -u "$SERVICE_NAME" -n 100 --no-pager || true
  exit 1
}

print_next_steps() {
  if [[ "$AUTO_START_SERVICE" == "1" ]]; then
    cat <<EOF

Update complete.

Service restart:
  systemctl started "$SERVICE_NAME" automatically

Optional cleanup later:
  sudo rm -f "$AMD64_SOURCES_FILE"
  sudo apt update
EOF
    return
  fi

  cat <<EOF

Update complete.

Next steps:
  cd "$HTYALE_DIR"
  ./start.sh

Optional cleanup later:
  sudo rm -f "$AMD64_SOURCES_FILE"
  sudo apt update
EOF
}

check_update() {
  require_cmd dpkg
  require_cmd sudo

  mkdir -p "$HTYALE_DIR" "$STATE_DIR"
  : >"$STATE_FILE"
  : >"$LAST_STAGE_FILE"

  report_stage init running "preparing downloader check on $(uname -m)"
  ensure_downloader_runtime
  download_downloader

  local remote_version
  local installed_version
  local zip_name

  report_stage check-update running "checking latest Hytale game version for patchline $PATCHLINE"
  remote_version="$(latest_server_version)"
  installed_version="$(local_server_version)"

  printf 'Latest available server version: %s\n' "$remote_version"
  if [[ -n "$installed_version" ]]; then
    printf 'Installed server version: %s\n' "$installed_version"
    if [[ "$installed_version" == "$remote_version" ]]; then
      printf 'Server is up to date.\n'
    else
      printf 'Server update available: %s -> %s\n' "$installed_version" "$remote_version"
    fi
  else
    printf 'Installed server version: unknown; could not infer it from %s/Server/HytaleServer.jar.\n' "$HTYALE_DIR"
  fi

  zip_name="$(latest_server_zip || true)"
  if [[ -n "$zip_name" ]]; then
    printf 'Latest downloaded server zip: %s\n' "$zip_name"
  else
    printf 'No downloaded server zip found in %s.\n' "$HTYALE_DIR"
  fi
  report_stage check-update completed "Hytale server update check completed"
}

update() {
  require_cmd dpkg
  require_cmd tar
  require_cmd sudo

  mkdir -p "$HTYALE_DIR"
  mkdir -p "$STATE_DIR"
  : >"$STATE_FILE"
  : >"$LAST_STAGE_FILE"

  report_stage init running "preparing updater runtime on $(uname -m)"
  ensure_downloader_runtime
  download_downloader
  backup_existing_install
  run_downloader
  extract_latest_zip
  restore_server_state
  start_service_if_requested
  report_stage complete completed "update flow finished successfully"
  print_next_steps
}

main() {
  trap on_error EXIT

  case "${1:-update}" in
    check-update)
      check_update
      ;;
    update)
      update
      ;;
    *)
      printf 'Usage: %s {check-update|update}\n' "$0" >&2
      exit 2
      ;;
  esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
