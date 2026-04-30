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
- `update`

The bot passes `SERVICE_NAME` to the script for every action. For `update`, it also passes `DOWNLOAD_TIMEOUT_SECONDS`.

The script is expected to write human-readable output and structured JSON progress lines. JSON progress lines should look like:

```json
{"timestamp":"2026-04-30T13:10:02Z","source":"hytale-manage","stage":"update","status":"running","message":"running updater"}
```

The bot streams JSON progress states back to Discord and includes trimmed non-JSON output in the final `status` and `logs` responses. During `update`, if the updater prints an `http://` or `https://` authentication URL, the bot sends that link as an ephemeral Discord message. Failed commands include the latest parsed failure state and trimmed script output.

## Settings

| Setting | Default | Meaning |
| --- | --- | --- |
| `HYTALE_MANAGER_ROLE_ID` | Required | Discord role allowed to use Hytale management commands. |
| `HYTALE_MANAGE_SCRIPT` | `~/hytale/hytale-manage.sh` | Path to the management script. |
| `HYTALE_SERVICE_NAME` | `hytale-server.service` | Service name passed to the script as `SERVICE_NAME`. |
| `HYTALE_COMMAND_TIMEOUT_SECONDS` | `15` | Timeout for status, logs, start, stop, and restart; minimum `1`. |
| `HYTALE_DOWNLOAD_TIMEOUT_SECONDS` | `1800` | Timeout for `/grate hytale update`; also passed to the script as `DOWNLOAD_TIMEOUT_SECONDS`; minimum `1`. |

`HYTALE_COMMAND_TIMEOUT_SECONDS` intentionally stays short for normal management commands. Long update and download work uses `HYTALE_DOWNLOAD_TIMEOUT_SECONDS`.

## Operations Flow

1. Run `/grate hytale status`.
2. If players report issues, run `/grate hytale logs`.
3. Use `/grate hytale start` only when the service is stopped.
4. Use `/grate hytale restart` when status/logs suggest the service is wedged.
5. Use `/grate hytale stop` when intentionally taking the server offline.
6. Use `/grate hytale update` when applying a new server build.
7. Re-check `/grate hytale status`.

## Update Behavior

`/grate hytale update` calls:

```text
~/hytale/hytale-manage.sh update
```

The manager script is responsible for stopping the service if needed, running the updater, forwarding updater progress, and starting the service again after the update.

The updater may require Hytale device authorization if its stored auth expires. When that happens, the bot reports the waiting state and sends the auth link if the updater printed one. The auth flow still needs to be completed by someone with access to the linked authorization page.

## Host Setup

Install the scripts on the same host as the bot:

```sh
mkdir -p ~/hytale
cp hytale-manage.sh hytale-update.sh ~/hytale/
chmod +x ~/hytale/hytale-manage.sh ~/hytale/hytale-update.sh
```

The scripts use `sudo -n`, so the bot's host user needs passwordless sudo for the commands the scripts run. It also needs permission to read service logs with `journalctl`; on Ubuntu, that usually means membership in a journal-reading group such as `systemd-journal`.

Deployment and service setup details live in [../MAINTAINER_SETUP.md](../MAINTAINER_SETUP.md).

## Troubleshooting

- If commands say controls are not set up, set `HYTALE_MANAGER_ROLE_ID`.
- If you lack permission, ask for the configured Hytale manager role.
- If a command fails to start, check `HYTALE_MANAGE_SCRIPT` and make sure the script exists and is executable by the bot user.
- If a script action fails, check host sudoers, systemd permissions, journal access, and the script output shown in Discord.
- If update waits for auth, complete the Hytale downloader authorization flow on the host.
- If output is trimmed, use host access for deeper investigation.
