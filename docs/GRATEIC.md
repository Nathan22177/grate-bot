# Grateic Phone

Grateic Phone is a Discord drawing-and-prompt game. Players join from a server channel, receive assignments in DMs, submit prompts and drawings, and reveal the finished chains back in the original channel.

## Commands

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

Create a lobby with:

```text
/grate create mode:<short|full> preset:<square|portrait|landscape> background:<color-preset> custom_background:<#RRGGBB?> require_canvas_size:<true|false?>
```

## Lobby Settings

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

## Rules

Grateic Phone is played in Discord DMs after players join from a server channel. Games are stored in memory, reset when the bot restarts, and are limited to one active Grateic Phone game per server. A Discord user can only be enrolled in one active Grateic Phone game across the bot.

Players are treated as ready when they join. If the bot cannot DM someone when the host starts the game, it rolls the game back to the lobby and marks that player unready. After they enable DMs from the server, they can run `/grate grateic ready`; then the host can run `/grate grateic start` again.

The original lobby message is continuously updated with lobby status, joined players, readiness, canvas settings, and start requirements. Lobby controls include `Join / Leave`, `Status`, `Start`, and `Cancel`; `Start` and `Cancel` remain host-only. DM assignment messages include a `Status` button for checking in-progress round completion without returning to the server channel. Reveals are posted in the channel where `/grate create` was run.

## Short Mode

Short mode is the fast showcase flow:

1. Every player submits one prompt.
2. Each player receives one prompt from another player.
3. Each player uploads one drawing.
4. The bot reveals each prompt with its drawing.

Example with 3 players:

1. A, B, and C submit prompts.
2. A draws C's prompt, B draws A's prompt, and C draws B's prompt.
3. The bot posts the three showcases.

## Full Mode

Full mode is the telephone-style chain flow:

1. Every player submits an initial prompt.
2. Players draw prompts.
3. Players describe drawings.
4. Drawing and prompt rounds alternate as chains rotate.
5. The original prompt author receives the final drawing and gives it a name.
6. The bot reveals every chain.

For `N` players, full mode runs `2N + 1` rounds.

## Canvas Size Rule

By default, `require_canvas_size` is enabled. During drawing rounds, uploaded images must exactly match the selected canvas preset:

- `square`: `1024x1024`
- `portrait`: `1080x1920`
- `landscape`: `1920x1080`

If an upload is the wrong size, the bot rejects it and tells the player the expected and actual dimensions. If Discord does not report image dimensions, the bot asks for a normal image upload whose dimensions Discord can detect.

Set `require_canvas_size:false` in `/grate create` to allow any image size for that lobby.

## Validation

The bot rejects:

- Duplicate submissions
- Submissions from non-players
- Text when an image is required
- Image-less messages when a drawing is required
- Text submissions longer than 140 characters
- Discord stickers used as text prompts
- Wrong-size drawings when `require_canvas_size` is enabled
- Invalid custom hex colors
- Submissions after the game has ended

If the bot cannot DM next-round assignments, the game does not advance. Players can fix DMs and have any player DM the bot again to retry assignment delivery.
