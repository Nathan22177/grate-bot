# Maintainer Setup

This guide is for people running or deploying the bot. The main README is intentionally focused on Discord command usage.

## What You Need

Required for a normal bot server:

- A Linux host. The examples below assume Ubuntu with `systemd`.
- A Discord application with a bot token.
- Message Content Intent enabled in the Discord Developer Portal.
- A Discord bot invite using the `bot` and `applications.commands` scopes.
- Channel permissions for the bot in the test server.
- A release binary, or Rust installed if you want to build on the host.
- A runtime environment file containing `DISCORD_TOKEN`.
- A `systemd` service to keep the bot running after reboot.

For Hytale controls:

- A Hytale dedicated server managed by `systemd`.
- The repository checkout available on the bot host, so the bot can run `deploy/hytale-manage.sh`.
- A Discord role ID for trusted Hytale managers.
- Host permissions needed by the management scripts.

Feature behavior, commands, settings, and troubleshooting for Hytale controls live in [docs/HYTALE.md](docs/HYTALE.md).

## Discord Bot Setup

Create a Discord application and bot token in the Discord Developer Portal.

Enable the bot's Message Content Intent so it can read Grateic DM submissions.

Create an invite URL with both scopes:

- `bot`
- `applications.commands`

For the first launch test, grant the bot these channel permissions in the test server:

- View Channel
- Send Messages
- Use Slash Commands
- Embed Links
- Attach Files
- Read Message History

Grateic also needs all test players to allow DMs from the server so it can send canvas and round instructions.

Copy `.env.example` to `.env` if that file exists in your environment, or create `.env` with:

```sh
DISCORD_TOKEN=your_discord_bot_token
```

Slash commands are registered globally, so Discord can take a little while to show new or changed commands after the bot starts.

## Build

Build the bot:

```sh
cargo build --release
```

## Server Setup

Create a dedicated runtime user and install the binary:

```sh
sudo useradd --system --home /opt/grate-bot --shell /usr/sbin/nologin grate-bot
sudo mkdir -p /opt/grate-bot /etc/grate-bot
sudo cp target/release/grate-bot /opt/grate-bot/grate-bot
sudo chown -R grate-bot:grate-bot /opt/grate-bot
sudo chmod 0755 /opt/grate-bot/grate-bot
```

Create `/etc/grate-bot/grate-bot.env`:

```sh
DISCORD_TOKEN=your_discord_bot_token
```

Protect the environment file because it contains a secret:

```sh
sudo chown root:grate-bot /etc/grate-bot/grate-bot.env
sudo chmod 0640 /etc/grate-bot/grate-bot.env
```

Install the included `systemd` unit:

```sh
sudo cp deploy/grate-bot.service /etc/systemd/system/grate-bot.service
sudo systemctl daemon-reload
sudo systemctl enable --now grate-bot.service
```

`deploy/deploy-grate-bot.sh` generates the systemd unit from the deploy settings, installs it, runs `systemctl daemon-reload`, and enables the service by default. Set `SKIP_SERVICE_FILE=1` if you manage the systemd unit separately.

Useful operations:

```sh
sudo systemctl status grate-bot.service
sudo journalctl -u grate-bot.service -n 100 --no-pager
sudo systemctl restart grate-bot.service
```

## Hytale Management Setup

The Hytale commands assume the bot runs on the same Ubuntu host as the Hytale dedicated server. The bot calls `hytale-manage.sh` for every Hytale action, and the script manages the service through `systemd` as `hytale-server.service` by default. See [docs/HYTALE.md](docs/HYTALE.md) for the full feature explanation.

The deploy script points `HYTALE_MANAGE_SCRIPT` at the checked-out repository's `deploy/hytale-manage.sh` by default, so Hytale script changes follow the deployed repo revision. Make sure the repository path is executable by `BOT_USER`; set `SKIP_HYTALE_SCRIPT_CONFIG=1` if you manage `HYTALE_MANAGE_SCRIPT` yourself.

The repository scripts must be executable:

```sh
chmod +x deploy/hytale-manage.sh deploy/hytale-update.sh
```

Only users with the configured Discord role can run Hytale management commands. Set this to the Discord role ID for trusted Hytale managers:

```sh
HYTALE_MANAGER_ROLE_ID=123456789012345678
```

Hytale settings:

```sh
HYTALE_SERVICE_NAME=hytale-server.service
HYTALE_COMMAND_TIMEOUT_SECONDS=15
HYTALE_DOWNLOAD_TIMEOUT_SECONDS=1800
HYTALE_CHECK_UPDATE_COMMAND='your read-only update check command'
HYTALE_UPDATE_COMMAND='your update command'
```

By default, the bot looks for `hytale-manage.sh` in `~/hytale` for the user running the bot. Set `HYTALE_MANAGE_SCRIPT` only if the script lives somewhere else, and use an absolute path in service environment files.

When using `deploy/deploy-grate-bot.sh`, deploy sets `HYTALE_MANAGE_SCRIPT` in `ENV_FILE` to the repo script path automatically.

`HYTALE_COMMAND_TIMEOUT_SECONDS` is used for `/grate hytale status`, `logs`, `start`, `stop`, and `restart`. `HYTALE_DOWNLOAD_TIMEOUT_SECONDS` is used for `/grate hytale check-update` and `/grate hytale update` and is passed to the script as `DOWNLOAD_TIMEOUT_SECONDS`. `HYTALE_CHECK_UPDATE_COMMAND` and `HYTALE_UPDATE_COMMAND` are used by `hytale-update.sh`; keep the check command read-only.

The bot only calls the configured management script with one allowlisted action:

- `status`
- `logs`
- `start`
- `stop`
- `restart`
- `check-update`
- `update`

The deploy script installs these Hytale sudoers rules by default, using `BOT_USER`, `HYTALE_SERVICE_NAME`, and `HYTALE_SUDOERS_FILE`. Set `SKIP_HYTALE_SUDOERS=1` if you manage sudoers separately.

For manual setup or review, the scripts use `sudo -n`, so the bot's host user needs passwordless sudo for the commands the scripts run. Replace `BOT_USER` with the Linux user that runs the bot:

```sudoers
BOT_USER ALL=(root) NOPASSWD: /usr/bin/systemctl start hytale-server.service, /usr/bin/systemctl stop hytale-server.service, /usr/bin/systemctl restart hytale-server.service
BOT_USER ALL=(root) NOPASSWD: /usr/bin/systemctl status hytale-server.service --no-pager
BOT_USER ALL=(root) NOPASSWD: /usr/bin/apt, /usr/bin/dpkg, /usr/bin/tee, /usr/bin/test
```

Install sudoers entries with `visudo`, for example `sudo visudo -f /etc/sudoers.d/grate-bot-hytale`, and make sure the service name matches `HYTALE_SERVICE_NAME`. Verify the bot user can run the needed commands without an interactive password prompt:

```sh
sudo -u BOT_USER sudo -n systemctl status hytale-server.service --no-pager
sudo -u BOT_USER sudo -n systemctl restart hytale-server.service
```

The bot's host user also needs permission to read service logs with `journalctl`. On Ubuntu, that usually means adding the bot user to a journal-reading group such as `systemd-journal`.

## Run Locally

```sh
cargo run
```

## Test

```sh
cargo test
```
