# grate-bot

Discord bot for Grate server utilities:

- Grateic drawing-and-prompt games
- Build verification for the running bot binary
- Optional Hytale server management for trusted helpers when the bot is built with Hytale support

## Commands

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

Create a lobby with:

```text
/grate create mode:<short|full> preset:<square|portrait|landscape> background:<color-preset> custom_background:<#RRGGBB?> require_canvas_size:<true|false?>
```

`/grate create` settings:

| Setting | Default | Meaning |
| --- | --- | --- |
| `mode` | Required | `short` for one prompt and one drawing, or `full` for the telephone-style chain game. |
| `preset` | Required | Canvas size: `square`, `portrait`, or `landscape`. |
| `background` | Required | Canvas background color preset. |
| `custom_background` | None | Required only when `background` is `custom hex`; must use `#RRGGBB`. |
| `require_canvas_size` | `true` | When enabled, drawing uploads must exactly match the selected canvas size. |

Canvas presets:

| Preset | Size |
| --- | --- |
| `square` | `1024x1024` |
| `portrait` | `1080x1920` |
| `landscape` | `1920x1080` |

Background choices:

- `white (#ffffff)`
- `black (#000000)`
- `warm paper (#f8f1df)`
- `pale blue (#dbeafe)`
- `pale green (#dcfce7)`
- `pale pink (#fce7f3)`
- `custom hex`

### Optional Hytale

| Command | Purpose |
| --- | --- |
| `/grate hytale help` | Explain Hytale commands, settings, permissions, operations flow, and troubleshooting. |
| `/grate hytale status` (unstable) | Check whether the Hytale service is running and enabled on boot. |
| `/grate hytale logs` (unstable) | Show recent service logs. |
| `/grate hytale start` (unstable) | Start the Hytale service. |
| `/grate hytale stop` (unstable) | Stop the Hytale service. |
| `/grate hytale restart` (unstable) | Restart the Hytale service. |

Hytale commands are optional and only exist in builds made with `--features hytale`. Operational Hytale commands are marked unstable for now. Management commands require the configured Hytale manager role. The help command is available without that role so people can discover the setup and permission requirements.

### Build Verification

| Command | Purpose |
| --- | --- |
| `/grate verify` | Report the running bot version, source ref, build commit, build input state, and executable SHA-256 checksum. |

Use `/grate verify` to compare the running binary against a published release artifact.

## Grateic Rules

Grateic is played in Discord DMs after players join from a server channel. Games are stored in memory, reset when the bot restarts, and are limited to one active Grateic game per server. A Discord user can only be enrolled in one active Grateic game across the bot.

Players are treated as ready when they join. If the bot cannot DM someone when the host starts the game, it rolls the game back to the lobby and marks that player unready. After they enable DMs from the server, they can run `/grate grateic ready`; then the host can run `/grate grateic start` again.

Lobby and status messages include `Join`, `Status`, and `Start` buttons. DM assignment messages include a `Status` button for checking the game without returning to the server channel. Reveals are posted in the channel where `/grate create` was run.

### Short Mode

Short mode is the fast showcase flow:

1. Every player submits one prompt.
2. Each player receives one prompt from another player.
3. Each player uploads one drawing.
4. The bot reveals each prompt with its drawing.

Example with 3 players:

1. A, B, and C submit prompts.
2. A draws C's prompt, B draws A's prompt, and C draws B's prompt.
3. The bot posts the three showcases.

### Full Mode

Full mode is the telephone-style chain flow:

1. Every player submits an initial prompt.
2. Players draw prompts.
3. Players describe drawings.
4. Drawing and prompt rounds alternate as chains rotate.
5. The original prompt author receives the final drawing and gives it a name.
6. The bot reveals every chain.

For `N` players, full mode runs `2N + 1` rounds.

### Canvas Size Rule

By default, `require_canvas_size` is enabled. During drawing rounds, uploaded images must exactly match the selected canvas preset:

- `square`: `1024x1024`
- `portrait`: `1080x1920`
- `landscape`: `1920x1080`

If an upload is the wrong size, the bot rejects it and tells the player the expected and actual dimensions. If Discord does not report image dimensions, the bot asks for a normal image upload whose dimensions Discord can detect.

Set `require_canvas_size:false` in `/grate create` to allow any image size for that lobby.

### Validation

The bot rejects:

- Duplicate submissions
- Submissions from non-players
- Text when an image is required
- Image-less messages when a drawing is required
- Wrong-size drawings when `require_canvas_size` is enabled
- Invalid custom hex colors
- Submissions after the game has ended

If the bot cannot DM next-round assignments, the game does not advance. Players can fix DMs and have any player DM the bot again to retry assignment delivery.

## Optional Hytale Operations

Hytale commands are for trusted helpers to check or nudge a co-hosted Hytale service. They are not part of the default bot build. Server maintainers can enable them by building with:

```sh
cargo build --release --features hytale
```

Default settings:

| Setting | Default | Meaning |
| --- | --- | --- |
| `HYTALE_MANAGER_ROLE_ID` | Required | Discord role allowed to use Hytale management commands. |
| `HYTALE_SERVICE_NAME` | `hytale-server.service` | systemd service name. |
| `HYTALE_LOG_LINES` | `40` | Number of log lines shown by `/grate hytale logs` (unstable); capped at `100`. |
| `HYTALE_COMMAND_TIMEOUT_SECONDS` | `15` | Timeout for local service commands; minimum `1`. |

Typical flow:

1. Run `/grate hytale status` (unstable).
2. If players report issues, run `/grate hytale logs` (unstable).
3. Use `/grate hytale start` (unstable) only when the service is stopped.
4. Use `/grate hytale restart` (unstable) when status/logs suggest the service is wedged.
5. Use `/grate hytale stop` (unstable) when intentionally taking the server offline.
6. Re-check `/grate hytale status` (unstable).

Server setup details, the bot `systemd` unit, Hytale build flags, and sudo requirements live in [MAINTAINER_SETUP.md](MAINTAINER_SETUP.md).
