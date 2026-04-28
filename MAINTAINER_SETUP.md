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

Optional for Hytale controls:

- A binary built with `--features hytale`.
- A Hytale dedicated server managed by `systemd`.
- A Discord role ID for trusted Hytale managers.
- Host permissions for unstable log reads and passwordless start/stop/restart of only the Hytale service.

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

Default build, without Hytale commands:

```sh
cargo build --release
```

Build with optional Hytale controls:

```sh
cargo build --release --features hytale
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

Useful operations:

```sh
sudo systemctl status grate-bot.service
sudo journalctl -u grate-bot.service -n 100 --no-pager
sudo systemctl restart grate-bot.service
```

## Hytale Management Setup

Hytale support is optional. Default builds do not include the `/grate hytale` command group. To enable it, build the bot with:

```sh
cargo build --release --features hytale
```

The Hytale commands assume the bot runs on the same Ubuntu host as the Hytale dedicated server and that the server is managed by `systemd` as `hytale-server.service`.

Operational Hytale commands are marked unstable for now. Only users with the configured Discord role can run Hytale commands. Set this to the Discord role ID for trusted Hytale managers:

```sh
HYTALE_MANAGER_ROLE_ID=123456789012345678
```

Optional Hytale settings:

```sh
HYTALE_SERVICE_NAME=hytale-server.service
HYTALE_LOG_LINES=40
HYTALE_COMMAND_TIMEOUT_SECONDS=15
```

If Hytale support is not enabled at build time, these variables are ignored and no Hytale slash commands are registered.

The bot only runs allowlisted local commands:

- `systemctl is-active`, `systemctl is-enabled`, and `systemctl status --no-pager` for unstable status
- `journalctl -u hytale-server.service -n 40 --no-pager` for unstable logs
- `sudo -n systemctl start|stop|restart hytale-server.service` for unstable server actions

The bot's host user needs passwordless sudo for only the Hytale service actions. Replace `BOT_USER` with the Linux user that runs the bot:

```sudoers
BOT_USER ALL=(root) NOPASSWD: /usr/bin/systemctl start hytale-server.service, /usr/bin/systemctl stop hytale-server.service, /usr/bin/systemctl restart hytale-server.service
```

The bot's host user also needs permission to read service logs with `journalctl`. On Ubuntu, that usually means adding the bot user to a journal-reading group such as `systemd-journal`.

## Run Locally

```sh
cargo run
```

Run locally with optional Hytale commands:

```sh
cargo run --features hytale
```

## Test

```sh
cargo test
```

Test the optional Hytale code too:

```sh
cargo test --all-features
```
