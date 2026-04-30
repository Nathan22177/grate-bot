# grate-bot

Discord bot for Grate server utilities:

- Build verification for the running bot binary
- Help and command discovery
- Grateic drawing-and-prompt games
- Hytale server management for trusted helpers

## Commands

### Verification And Help

| Command | Purpose |
| --- | --- |
| `/grate verify` | Report the running bot version, source ref, build commit, build input state, and executable SHA-256 checksum. |
| `/grate help` | Show the top-level command summary. |

Use `/grate verify` to compare the running binary against a published release artifact.

### Grateic

| Command | Purpose |
| --- | --- |
| `/grate create` | Create a Grateic lobby. |
| `/grate grateic help` | Explain Grateic commands, settings, modes, and examples. |
| `/grate grateic join` | Join the active Grateic lobby in this server. |
| `/grate grateic ready` | Retry the DM readiness check after fixing DMs. |
| `/grate grateic start` | Start the active lobby. Host only. |
| `/grate grateic status` | Show players, mode, round, readiness, canvas, and waiting count. |
| `/grate grateic cancel` | Cancel the active lobby. Host only. |

Full Grateic rules, setup options, modes, canvas settings, and validation details live in [docs/GRATEIC.md](docs/GRATEIC.md).

### Hytale

| Command | Purpose |
| --- | --- |
| `/grate hytale help` | Explain Hytale commands, settings, permissions, operations flow, and troubleshooting. |
| `/grate hytale status` | Check the Hytale service status. |
| `/grate hytale logs` | Show recent service logs. |
| `/grate hytale start` | Start the Hytale service. |
| `/grate hytale stop` | Stop the Hytale service. |
| `/grate hytale restart` | Restart the Hytale service. |
| `/grate hytale update` | Update the Hytale server and restart it. |

Hytale management commands require the configured Hytale manager role. The feature overview, runtime settings, script contract, and operations flow live in [docs/HYTALE.md](docs/HYTALE.md).

## Maintainers

Deployment, bot token setup, systemd, and host permissions are covered in [MAINTAINER_SETUP.md](MAINTAINER_SETUP.md).
