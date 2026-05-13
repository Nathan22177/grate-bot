# Hytale Management

Hytale management lets trusted Discord helpers check, manage, and update a co-hosted Hytale service from Discord. The bot does not run arbitrary shell commands; it calls one local management script with one allowlisted action.

## Commands

| Command | Purpose |
| --- | --- |
| `/grate hytale help` | Explain Hytale commands, settings, permissions, operations flow, and troubleshooting. |
| `/grate hytale status` | Check the Hytale service status. |
| `/grate hytale logs` | Show recent service logs. |
| `/grate hytale start` | Start the Hytale service. |
| `/grate hytale stop` | Stop the Hytale service. |
| `/grate hytale restart` | Restart the Hytale service. |
| `/grate hytale check-update` | Check whether a Hytale server update is available without applying it. |
| `/grate hytale update` | Update the Hytale server and restart it. |

All operational Hytale command responses are ephemeral and require the configured Hytale manager role. `/grate hytale help` is available without that role so people can discover the setup and permission requirements.

## Script Contract

The bot calls `hytale-manage.sh` for every Hytale action. By default, the script path is:

```text
~/hytale/hytale-manage.sh
```

The bot only passes one of these fixed action arguments:

- `status`
- `logs`
- `start`
- `stop`
- `restart`
- `check-update`
- `update`

The bot passes `SERVICE_NAME` to the script for every action. For `check-update` and `update`, it also passes `DOWNLOAD_TIMEOUT_SECONDS`.

The script is expected to write human-readable output and structured JSON progress lines. JSON progress lines should look like:

```json
{"timestamp":"2026-04-30T13:10:02Z","source":"hytale-manage","stage":"update","status":"running","message":"running updater"}
```

The bot streams JSON progress states back to Discord and includes trimmed non-JSON output in the final `status`, `logs`, and `check-update` responses. During `check-update` or `update`, if the updater prints an `http://` or `https://` authentication URL, the bot sends that link as an ephemeral Discord message. Failed commands include the latest parsed failure state and trimmed script output.

## Settings

| Setting | Default | Meaning |
| --- | --- | --- |
| `HYTALE_MANAGER_ROLE_ID` | Required | Discord role allowed to use Hytale management commands. |
| `HYTALE_MANAGE_SCRIPT` | `~/hytale/hytale-manage.sh` | Path to the management script. |
| `HYTALE_SERVICE_NAME` | `hytale-server.service` | Service name passed to the script as `SERVICE_NAME`. |
| `HYTALE_COMMAND_TIMEOUT_SECONDS` | `15` | Timeout for status, logs, start, stop, and restart; minimum `1`. |
| `HYTALE_DOWNLOAD_TIMEOUT_SECONDS` | `1800` | Timeout for `/grate hytale check-update` and `/grate hytale update`; also passed to the script as `DOWNLOAD_TIMEOUT_SECONDS`; minimum `1`. |

`HYTALE_COMMAND_TIMEOUT_SECONDS` intentionally stays short for normal management commands. Long update and download work uses `HYTALE_DOWNLOAD_TIMEOUT_SECONDS`.

## Operations Flow

1. Run `/grate hytale status`.
2. If players report issues, run `/grate hytale logs`.
3. Use `/grate hytale start` only when the service is stopped.
4. Use `/grate hytale restart` when status/logs suggest the service is wedged.
5. Use `/grate hytale stop` when intentionally taking the server offline.
6. Use `/grate hytale check-update` to see whether a new server build is available.
7. Use `/grate hytale update` when applying a new server build.
8. Re-check `/grate hytale status`.

## Check Update Behavior

`/grate hytale check-update` calls the configured management script:

```text
HYTALE_MANAGE_SCRIPT check-update
```

The manager script is responsible for checking update state without stopping, updating, or restarting the service. The migrated downloader updater runs the Hytale Downloader CLI with `-check-update` to check the downloader tool and `-print-version` to show the latest game version for the configured patchline, without extracting or applying server files.

## Update Behavior

`/grate hytale update` calls the configured management script:

```text
HYTALE_MANAGE_SCRIPT update
```

The manager script is responsible for stopping the service if needed, running the updater, forwarding updater progress, and starting the service again after the update.

The updater may require Hytale device authorization if its stored auth expires. When that happens, the bot reports the waiting state and sends the auth link if the updater printed one. The auth flow still needs to be completed by someone with access to the linked authorization page.

## Host Setup

When deployed with `deploy/deploy-grate-bot.sh`, the bot runs the scripts directly from the checked-out repository. Deploy writes `HYTALE_MANAGE_SCRIPT` in the bot environment file so it points at:

```text
<repo>/deploy/hytale-manage.sh
```

The repository scripts must be executable:

```sh
chmod +x deploy/hytale-manage.sh deploy/hytale-update.sh deploy/hytale-downloader-update.sh
```

If the repo is checked out under a private home directory such as `/home/ubuntu`, deploy installs `acl` when needed and grants narrow ACL access for the bot user. Alternatively, keep the checkout under a service path such as `/srv/grate-bot` or `/opt/grate-bot`.

`hytale-update.sh` delegates the Hytale-specific downloader work to configured commands. The deploy script seeds these commands automatically to call the migrated legacy downloader workflow in `deploy/hytale-downloader-update.sh`:

```sh
HYTALE_CHECK_UPDATE_COMMAND='<repo>/deploy/hytale-downloader-update.sh check-update'
HYTALE_UPDATE_COMMAND='<repo>/deploy/hytale-downloader-update.sh update'
```

Override these in `/etc/grate-bot/grate-bot.env` only if this host uses a different Hytale update tool. `HYTALE_CHECK_UPDATE_COMMAND` must not stop, update, extract server files, or restart the service.

The scripts use `sudo -n`, so the bot's host user needs passwordless sudo for the commands the scripts run. If Discord shows `sudo: a password is required`, the sudoers entry is missing or does not match the command path/service name. It also needs permission to read service logs with `journalctl`; on Ubuntu, that usually means membership in a journal-reading group such as `systemd-journal`.

Deployment and service setup details live in [../MAINTAINER_SETUP.md](../MAINTAINER_SETUP.md).

## Troubleshooting

- If commands say controls are not set up, set `HYTALE_MANAGER_ROLE_ID`.
- If you lack permission, ask for the configured Hytale manager role.
- If a command fails to start, check `HYTALE_MANAGE_SCRIPT` and make sure the script exists and is executable by the bot user. For repo paths under `/home/ubuntu`, deploy should install `acl` and grant access automatically; if apt is misconfigured or blocked, move the checkout under `/srv/grate-bot` or `/opt/grate-bot`.
- If a script action fails with `sudo: a password is required`, configure passwordless sudo for the bot host user and verify it with `sudo -u BOT_USER sudo -n systemctl status hytale-server.service --no-pager`.
- If a script action fails for another reason, check host sudoers, systemd permissions, journal access, and the script output shown in Discord.
- If update waits for auth, complete the Hytale downloader authorization flow on the host.
- If output is trimmed, use host access for deeper investigation.
