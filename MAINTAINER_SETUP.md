# Maintainer Setup

This guide is for people running or deploying the bot. The main README is intentionally focused on Discord command usage.

## Discord Bot Setup

Create a Discord application and bot token in the Discord Developer Portal.

Enable the bot's Message Content Intent so it can read Grateic DM submissions.

Copy `.env.example` to `.env` if that file exists in your environment, or create `.env` with:

```sh
DISCORD_TOKEN=your_discord_bot_token
```

Slash commands are registered globally, so Discord can take a little while to show new or changed commands after the bot starts.

## Hytale Management Setup

The Hytale commands assume the bot runs on the same Ubuntu host as the Hytale dedicated server and that the server is managed by `systemd` as `hytale-server.service`.

Only users with the configured Discord role can run Hytale commands. Set this to the Discord role ID for trusted Hytale managers:

```sh
HYTALE_MANAGER_ROLE_ID=123456789012345678
```

Optional Hytale settings:

```sh
HYTALE_SERVICE_NAME=hytale-server.service
HYTALE_LOG_LINES=40
HYTALE_COMMAND_TIMEOUT_SECONDS=15
```

The bot only runs allowlisted local commands:

- `systemctl is-active`, `systemctl is-enabled`, and `systemctl status --no-pager` for status
- `journalctl -u hytale-server.service -n 40 --no-pager` for logs
- `sudo -n systemctl start|stop|restart hytale-server.service` for server actions

The bot's host user needs passwordless sudo for only the Hytale service actions. Replace `BOT_USER` with the Linux user that runs the bot:

```sudoers
BOT_USER ALL=(root) NOPASSWD: /usr/bin/systemctl start hytale-server.service, /usr/bin/systemctl stop hytale-server.service, /usr/bin/systemctl restart hytale-server.service
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
