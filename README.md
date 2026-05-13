# grate-bot

Discord bot for Grate server tools:

- Host Grateic Phone, a drawing-and-prompt game
- Manage the Hytale server with trusted helpers
- Verify the running bot build

## Commands

### Verification And Help

| Command | Purpose |
| --- | --- |
| `/grate verify` | Report whether the bot is running an official release, plus version, source ref, build commit, build input state, executable SHA-256 checksum, and official release checksum link. |
| `/grate help` | Show the top-level command summary. |

Use `/grate verify` to compare the running executable SHA-256 against the checksum asset attached to the official GitHub release.

### Grateic Phone

| Command | Purpose |
| --- | --- |
| `/grate create` | Create a Grateic Phone lobby. |
| `/grate grateic help` | Explain Grateic Phone commands, settings, modes, and examples. |
| `/grate grateic join` | Join the active Grateic Phone lobby in this server. |
| `/grate grateic ready` | Retry the DM readiness check after fixing DMs. |
| `/grate grateic start` | Start the active lobby. Host only. |
| `/grate grateic status` | Refresh lobby status before start, or privately show in-progress round status. |
| `/grate grateic cancel` | Cancel the active lobby before it starts. Host only. |
| `/grate grateic force_cancel` | Force-cancel a stuck active game. Host only. |

Full Grateic Phone rules, setup options, modes, canvas settings, and validation details live in [docs/GRATEIC.md](docs/GRATEIC.md).

### Hytale

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

Hytale management commands require the configured Hytale manager role. The feature overview, runtime settings, script contract, and operations flow live in [docs/HYTALE.md](docs/HYTALE.md).

## Maintainers

Deployment, bot token setup, systemd, and host permissions are covered in [MAINTAINER_SETUP.md](MAINTAINER_SETUP.md).
