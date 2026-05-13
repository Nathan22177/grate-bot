#!/usr/bin/env bash

set -euo pipefail

SERVICE_NAME="${SERVICE_NAME:-grate-bot.service}"
BOT_USER="${BOT_USER:-grate-bot}"
BOT_GROUP="${BOT_GROUP:-$BOT_USER}"
INSTALL_DIR="${INSTALL_DIR:-/opt/grate-bot}"
BIN_NAME="${BIN_NAME:-grate-bot}"
ENV_FILE="${ENV_FILE:-/etc/grate-bot/grate-bot.env}"
SERVICE_FILE="${SERVICE_FILE:-/etc/systemd/system/$SERVICE_NAME}"
BRANCH="${BRANCH:-}"
REMOTE="${REMOTE:-origin}"
HYTALE_MANAGE_SCRIPT="${HYTALE_MANAGE_SCRIPT:-}"
HYTALE_SERVICE_NAME="${HYTALE_SERVICE_NAME:-}"
HYTALE_SUDOERS_FILE="${HYTALE_SUDOERS_FILE:-/etc/sudoers.d/grate-bot-hytale}"
SKIP_GIT_PULL="${SKIP_GIT_PULL:-0}"
SKIP_BUILD="${SKIP_BUILD:-0}"
SKIP_SERVICE_FILE="${SKIP_SERVICE_FILE:-0}"
SKIP_HYTALE_SCRIPT_CONFIG="${SKIP_HYTALE_SCRIPT_CONFIG:-0}"
SKIP_HYTALE_SUDOERS="${SKIP_HYTALE_SUDOERS:-0}"

log() {
  printf '[deploy-grate-bot] %s\n' "$*"
}

fail() {
  printf '[deploy-grate-bot] ERROR: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

env_file_simple_value() {
  local key="$1"
  local line value

  sudo test -f "$ENV_FILE" || return 1
  line="$(sudo grep -E "^[[:space:]]*(export[[:space:]]+)?${key}=" "$ENV_FILE" | tail -n 1 || true)"
  [[ -n "$line" ]] || return 1
  value="${line#*=}"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"

  case "$value" in
    \"*\") value="${value#\"}"; value="${value%\"}" ;;
    \'*\') value="${value#\'}"; value="${value%\'}" ;;
  esac

  [[ -n "$value" ]] || return 1
  printf '%s\n' "$value"
}

validate_env_file_value() {
  local key="$1"
  local value="$2"

  [[ "$value" != *$'\n'* && "$value" != *$'\r'* ]] || fail "$key cannot contain newlines"
  [[ "$value" != *[[:space:]]* ]] || fail "$key cannot contain whitespace for systemd EnvironmentFile"
  [[ "$value" != *\"* && "$value" != *\'* ]] || fail "$key cannot contain quotes for systemd EnvironmentFile"
  [[ "$value" == /* ]] || fail "$key must be an absolute path"
}

validate_manage_script_path() {
  local path="$1"

  [[ "$path" == /* ]] || fail "HYTALE_MANAGE_SCRIPT must be an absolute path: $path"
  case "$path" in
    /tmp/*|/var/tmp/*|/private/tmp/*)
      fail "HYTALE_MANAGE_SCRIPT points at a temporary path: $path"
      ;;
  esac

  case "$path" in
    /opt/*|/srv/*|/usr/local/*) ;;
    *)
      log "Warning: HYTALE_MANAGE_SCRIPT points outside a typical service path: $path"
      ;;
  esac
}

grant_bot_script_access() {
  local script_path="$1"
  local dir

  [[ -e "$script_path" ]] || fail "script path does not exist: $script_path"

  if sudo -u "$BOT_USER" test -x "$script_path"; then
    return 0
  fi

  if ! command -v setfacl >/dev/null 2>&1; then
    log "$BOT_USER cannot execute $script_path and setfacl is missing; installing acl"
    if command -v apt-get >/dev/null 2>&1; then
      sudo apt-get update
      sudo apt-get install -y acl
    elif command -v apt >/dev/null 2>&1; then
      sudo apt update
      sudo apt install -y acl
    else
      fail "$BOT_USER cannot execute $script_path and setfacl is missing. Install acl/setfacl or move the repo to /srv/grate-bot or /opt/grate-bot so $BOT_USER can traverse the script path."
    fi

    command -v setfacl >/dev/null 2>&1 || fail "acl install completed but setfacl is still unavailable"
  fi

  log "Granting $BOT_USER execute access to $script_path"
  sudo setfacl -m "u:$BOT_USER:rx" "$script_path"

  dir="$(dirname "$script_path")"
  while [[ "$dir" != "/" ]]; do
    sudo setfacl -m "u:$BOT_USER:--x" "$dir"
    dir="$(dirname "$dir")"
  done

  sudo -u "$BOT_USER" test -x "$script_path" || fail "$BOT_USER still cannot execute $script_path after applying ACLs"
}

upsert_env_file_value() {
  local key="$1"
  local value="$2"
  local tmp output assignment replaced owner group mode

  tmp="$(mktemp)"
  output="$(mktemp)"
  cleanup_env_tmp() {
    rm -f "$tmp" "$output"
  }
  trap cleanup_env_tmp EXIT

  if sudo test -f "$ENV_FILE"; then
    sudo cp "$ENV_FILE" "$tmp"
    owner="$(sudo stat -c '%U' "$ENV_FILE")"
    group="$(sudo stat -c '%G' "$ENV_FILE")"
    mode="$(sudo stat -c '%a' "$ENV_FILE")"
  else
    : >"$tmp"
    getent group "$BOT_GROUP" >/dev/null || fail "group does not exist for new env file: $BOT_GROUP"
    owner="root"
    group="$BOT_GROUP"
    mode="640"
  fi

  validate_env_file_value "$key" "$value"
  assignment="$key=$value"
  replaced=0
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" =~ ^[[:space:]]*(export[[:space:]]+)?${key}= ]]; then
      printf '%s\n' "$assignment" >>"$output"
      replaced=1
    else
      printf '%s\n' "$line" >>"$output"
    fi
  done <"$tmp"

  if [[ "$replaced" != "1" ]]; then
    printf '%s\n' "$assignment" >>"$output"
  fi

  sudo install -d -o root -g "$group" -m 0750 "$(dirname "$ENV_FILE")"
  sudo install -o "$owner" -g "$group" -m "$mode" "$output" "$ENV_FILE"
  cleanup_env_tmp
  trap - EXIT
}

usage() {
  cat <<'EOF'
Usage:
  deploy/deploy-grate-bot.sh

Run from the grate-bot repository on the server.

Environment overrides:
  SERVICE_NAME                 default: grate-bot.service
  BOT_USER                     default: grate-bot
  BOT_GROUP                    default: same as BOT_USER
  INSTALL_DIR                  default: /opt/grate-bot
  BIN_NAME                     default: grate-bot
  ENV_FILE                     default: /etc/grate-bot/grate-bot.env
  SERVICE_FILE                 default: /etc/systemd/system/SERVICE_NAME
  REMOTE                       default: origin
  BRANCH                       branch to deploy; prompts when unset, default: main
  HYTALE_MANAGE_SCRIPT         default: this repo's deploy/hytale-manage.sh
  HYTALE_SERVICE_NAME          default: hytale-server.service
  HYTALE_SUDOERS_FILE          default: /etc/sudoers.d/grate-bot-hytale
  SKIP_GIT_PULL=1              do not git checkout/pull
  SKIP_BUILD=1                 do not run cargo build --release
  SKIP_SERVICE_FILE=1          do not install or enable the systemd service file
  SKIP_HYTALE_SCRIPT_CONFIG=1  do not update HYTALE_MANAGE_SCRIPT in ENV_FILE
  SKIP_HYTALE_SUDOERS=1        do not install Hytale sudoers rules

Examples:
  deploy/deploy-grate-bot.sh
  HYTALE_MANAGE_SCRIPT=/srv/grate-bot/deploy/hytale-manage.sh deploy/deploy-grate-bot.sh
  SKIP_GIT_PULL=1 deploy/deploy-grate-bot.sh
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

need_cmd cargo
need_cmd git
need_cmd install
need_cmd sudo
need_cmd systemctl
if [[ "$SKIP_HYTALE_SCRIPT_CONFIG" != "1" ]]; then
  need_cmd getent
fi
if [[ "$SKIP_HYTALE_SUDOERS" != "1" ]]; then
  need_cmd visudo
fi

if [[ ! -f Cargo.toml ]]; then
  fail "run this script from the grate-bot repository root"
fi

if [[ -z "$BRANCH" ]]; then
  read -r -p "Branch to deploy [main]: " BRANCH
  BRANCH="${BRANCH:-main}"
fi

if [[ "$SKIP_GIT_PULL" != "1" ]]; then
  log "Checking out $BRANCH and pulling latest from $REMOTE"
  git fetch "$REMOTE" "$BRANCH"
  git checkout "$BRANCH"
  git pull --ff-only "$REMOTE" "$BRANCH"
else
  log "Skipping git pull"
fi

if [[ "$SKIP_BUILD" != "1" ]]; then
  log "Building release binary"
  cargo build --release
else
  log "Skipping build"
fi

if [[ ! -x "target/release/$BIN_NAME" ]]; then
  fail "release binary target/release/$BIN_NAME does not exist or is not executable"
fi

if [[ ! -f "$ENV_FILE" ]]; then
  log "Environment file $ENV_FILE does not exist yet; create it before starting the service"
else
  log "Environment file exists at $ENV_FILE"
fi

if [[ "$SKIP_HYTALE_SCRIPT_CONFIG" != "1" ]]; then
  if [[ -z "$HYTALE_MANAGE_SCRIPT" ]]; then
    HYTALE_MANAGE_SCRIPT="$PWD/deploy/hytale-manage.sh"
  fi

  validate_manage_script_path "$HYTALE_MANAGE_SCRIPT"
  [[ -x "$HYTALE_MANAGE_SCRIPT" ]] || fail "Hytale manage script is not executable: $HYTALE_MANAGE_SCRIPT"
  grant_bot_script_access "$HYTALE_MANAGE_SCRIPT"

  HYTALE_UPDATE_SCRIPT="$(env_file_simple_value HYTALE_UPDATE_SCRIPT || true)"
  HYTALE_UPDATE_SCRIPT="${HYTALE_UPDATE_SCRIPT:-$(dirname "$HYTALE_MANAGE_SCRIPT")/hytale-update.sh}"
  [[ -x "$HYTALE_UPDATE_SCRIPT" ]] || fail "Hytale update script is not executable: $HYTALE_UPDATE_SCRIPT"
  grant_bot_script_access "$HYTALE_UPDATE_SCRIPT"

  log "Pointing HYTALE_MANAGE_SCRIPT at $HYTALE_MANAGE_SCRIPT"
  upsert_env_file_value HYTALE_MANAGE_SCRIPT "$HYTALE_MANAGE_SCRIPT"
else
  log "Skipping Hytale manage script env config"
fi

if [[ -z "$HYTALE_SERVICE_NAME" ]]; then
  HYTALE_SERVICE_NAME="$(env_file_simple_value HYTALE_SERVICE_NAME || true)"
fi
HYTALE_SERVICE_NAME="${HYTALE_SERVICE_NAME:-hytale-server.service}"
[[ "$HYTALE_SERVICE_NAME" =~ ^[A-Za-z0-9_.@-]+$ ]] || fail "HYTALE_SERVICE_NAME has unsupported characters: $HYTALE_SERVICE_NAME"

if [[ "$SKIP_HYTALE_SUDOERS" != "1" ]]; then
  systemctl_path="$(type -P systemctl)"
  apt_path="$(type -P apt || true)"
  dpkg_path="$(type -P dpkg || true)"
  tee_path="$(type -P tee || true)"
  test_path="$(type -P test || true)"
  sudoers_tmp="$(mktemp)"
  cleanup_sudoers_tmp() {
    rm -f "$sudoers_tmp"
  }
  trap cleanup_sudoers_tmp EXIT

  {
    printf '%s ALL=(root) NOPASSWD: %s start %s, %s stop %s, %s restart %s\n' \
      "$BOT_USER" \
      "$systemctl_path" "$HYTALE_SERVICE_NAME" \
      "$systemctl_path" "$HYTALE_SERVICE_NAME" \
      "$systemctl_path" "$HYTALE_SERVICE_NAME"
    printf '%s ALL=(root) NOPASSWD: %s status %s --no-pager\n' \
      "$BOT_USER" "$systemctl_path" "$HYTALE_SERVICE_NAME"

    sudo_commands=()
    [[ -n "$apt_path" ]] && sudo_commands+=("$apt_path")
    [[ -n "$dpkg_path" ]] && sudo_commands+=("$dpkg_path")
    [[ -n "$tee_path" ]] && sudo_commands+=("$tee_path")
    [[ -n "$test_path" ]] && sudo_commands+=("$test_path")
    if [[ "${#sudo_commands[@]}" -gt 0 ]]; then
      printf '%s ALL=(root) NOPASSWD: ' "$BOT_USER"
      separator=""
      for sudo_command in "${sudo_commands[@]}"; do
        printf '%s%s' "$separator" "$sudo_command"
        separator=", "
      done
      printf '\n'
    fi
  } >"$sudoers_tmp"

  log "Validating Hytale sudoers rules for $BOT_USER and $HYTALE_SERVICE_NAME"
  sudo visudo -cf "$sudoers_tmp"
  log "Installing Hytale sudoers rules to $HYTALE_SUDOERS_FILE"
  sudo install -o root -g root -m 0440 "$sudoers_tmp" "$HYTALE_SUDOERS_FILE"
  cleanup_sudoers_tmp
  trap - EXIT
else
  log "Skipping Hytale sudoers install"
fi

if [[ "$SKIP_SERVICE_FILE" != "1" ]]; then
  service_tmp="$(mktemp)"
  cleanup_service_tmp() {
    rm -f "$service_tmp"
  }
  trap cleanup_service_tmp EXIT

  cat >"$service_tmp" <<EOF
[Unit]
Description=Grate Discord Bot
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$BOT_USER
Group=$BOT_GROUP
WorkingDirectory=$INSTALL_DIR
EnvironmentFile=$ENV_FILE
ExecStart=$INSTALL_DIR/$BIN_NAME
Restart=on-failure
RestartSec=10
NoNewPrivileges=false
PrivateTmp=true

[Install]
WantedBy=multi-user.target
EOF

  log "Installing systemd service file to $SERVICE_FILE"
  sudo install -o root -g root -m 0644 "$service_tmp" "$SERVICE_FILE"
  cleanup_service_tmp
  trap - EXIT
  log "Reloading systemd"
  sudo systemctl daemon-reload
  log "Enabling $SERVICE_NAME"
  sudo systemctl enable "$SERVICE_NAME"
else
  log "Skipping service file install"
fi

log "Stopping $SERVICE_NAME"
sudo systemctl stop "$SERVICE_NAME" || true

log "Installing bot binary to $INSTALL_DIR/$BIN_NAME"
sudo install -d -o "$BOT_USER" -g "$BOT_GROUP" -m 0755 "$INSTALL_DIR"
sudo install -o "$BOT_USER" -g "$BOT_GROUP" -m 0755 \
  "target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"


log "Starting $SERVICE_NAME"
sudo systemctl start "$SERVICE_NAME"

log "Service status"
sudo systemctl status "$SERVICE_NAME" --no-pager

log "Recent logs"
sudo journalctl -u "$SERVICE_NAME" -n 80 --no-pager

log "Deployment complete"
