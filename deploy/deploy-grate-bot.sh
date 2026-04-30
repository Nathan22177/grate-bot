#!/usr/bin/env bash

set -euo pipefail

SERVICE_NAME="${SERVICE_NAME:-grate-bot.service}"
BOT_USER="${BOT_USER:-grate-bot}"
BOT_GROUP="${BOT_GROUP:-$BOT_USER}"
INSTALL_DIR="${INSTALL_DIR:-/opt/grate-bot}"
BIN_NAME="${BIN_NAME:-grate-bot}"
ENV_FILE="${ENV_FILE:-/etc/grate-bot/grate-bot.env}"
BRANCH="${BRANCH:-}"
REMOTE="${REMOTE:-origin}"
HYTALE_SCRIPT_SOURCE_DIR="${HYTALE_SCRIPT_SOURCE_DIR:-}"
HYTALE_SCRIPT_INSTALL_DIR="${HYTALE_SCRIPT_INSTALL_DIR:-}"
SKIP_GIT_PULL="${SKIP_GIT_PULL:-0}"
SKIP_BUILD="${SKIP_BUILD:-0}"
SKIP_HYTALE_SCRIPTS="${SKIP_HYTALE_SCRIPTS:-0}"

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
  REMOTE                       default: origin
  BRANCH                       branch to deploy; prompts when unset, default: main
  HYTALE_SCRIPT_SOURCE_DIR     directory containing hytale-manage.sh and hytale-update.sh
  HYTALE_SCRIPT_INSTALL_DIR    default: BOT_USER's home/hytale
  SKIP_GIT_PULL=1              do not git checkout/pull
  SKIP_BUILD=1                 do not run cargo build --release
  SKIP_HYTALE_SCRIPTS=1        do not install Hytale scripts

Examples:
  deploy/deploy-grate-bot.sh
  HYTALE_SCRIPT_SOURCE_DIR=/tmp/hytale-scripts deploy/deploy-grate-bot.sh
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

log "Stopping $SERVICE_NAME"
sudo systemctl stop "$SERVICE_NAME" || true

log "Installing bot binary to $INSTALL_DIR/$BIN_NAME"
sudo install -d -o "$BOT_USER" -g "$BOT_GROUP" -m 0755 "$INSTALL_DIR"
sudo install -o "$BOT_USER" -g "$BOT_GROUP" -m 0755 \
  "target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

if [[ ! -f "$ENV_FILE" ]]; then
  log "Environment file $ENV_FILE does not exist yet; create it before starting the service"
else
  log "Environment file exists at $ENV_FILE"
fi

if [[ "$SKIP_HYTALE_SCRIPTS" != "1" ]]; then
  if [[ -z "$HYTALE_SCRIPT_INSTALL_DIR" ]]; then
    bot_home="$(getent passwd "$BOT_USER" | cut -d: -f6)"
    [[ -n "$bot_home" ]] || fail "could not determine home directory for $BOT_USER"
    HYTALE_SCRIPT_INSTALL_DIR="$bot_home/hytale"
  fi

  if [[ -z "$HYTALE_SCRIPT_SOURCE_DIR" ]]; then
    if [[ -f hytale-manage.sh && -f hytale-update.sh ]]; then
      HYTALE_SCRIPT_SOURCE_DIR="$PWD"
    elif [[ -f deploy/hytale-manage.sh && -f deploy/hytale-update.sh ]]; then
      HYTALE_SCRIPT_SOURCE_DIR="$PWD/deploy"
    elif [[ -f "$HYTALE_SCRIPT_INSTALL_DIR/hytale-manage.sh" && -f "$HYTALE_SCRIPT_INSTALL_DIR/hytale-update.sh" ]]; then
      HYTALE_SCRIPT_SOURCE_DIR=""
    else
      log "No Hytale script source directory found; set HYTALE_SCRIPT_SOURCE_DIR to install them"
    fi
  fi

  log "Ensuring Hytale script directory exists at $HYTALE_SCRIPT_INSTALL_DIR"
  sudo install -d -o "$BOT_USER" -g "$BOT_GROUP" -m 0755 "$HYTALE_SCRIPT_INSTALL_DIR"

  if [[ -n "$HYTALE_SCRIPT_SOURCE_DIR" ]]; then
    [[ -f "$HYTALE_SCRIPT_SOURCE_DIR/hytale-manage.sh" ]] || fail "missing $HYTALE_SCRIPT_SOURCE_DIR/hytale-manage.sh"
    [[ -f "$HYTALE_SCRIPT_SOURCE_DIR/hytale-update.sh" ]] || fail "missing $HYTALE_SCRIPT_SOURCE_DIR/hytale-update.sh"

    log "Installing Hytale scripts from $HYTALE_SCRIPT_SOURCE_DIR"
    sudo install -o "$BOT_USER" -g "$BOT_GROUP" -m 0755 \
      "$HYTALE_SCRIPT_SOURCE_DIR/hytale-manage.sh" \
      "$HYTALE_SCRIPT_INSTALL_DIR/hytale-manage.sh"
    sudo install -o "$BOT_USER" -g "$BOT_GROUP" -m 0755 \
      "$HYTALE_SCRIPT_SOURCE_DIR/hytale-update.sh" \
      "$HYTALE_SCRIPT_INSTALL_DIR/hytale-update.sh"
  fi
else
  log "Skipping Hytale script install"
fi

log "Starting $SERVICE_NAME"
sudo systemctl start "$SERVICE_NAME"

log "Service status"
sudo systemctl status "$SERVICE_NAME" --no-pager

log "Recent logs"
sudo journalctl -u "$SERVICE_NAME" -n 80 --no-pager

log "Deployment complete"
