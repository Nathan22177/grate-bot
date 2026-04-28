# grate-bot

Bot for various Grate shenanigans.

## Features

### Grateic

Grateic is a small Grateic Phone-style drawing-and-prompt game.

Commands:

- `/grate grateic create preset:<square|portrait|landscape> background:<color-preset> custom_background:<#RRGGBB?>`
- `/grate grateic join`
- `/grate grateic ready`
- `/grate grateic start`
- `/grate grateic status`
- `/grate grateic cancel`

Arguments for `/grate grateic create`:

- `preset`: canvas size preset.
- `background`: background color preset.
- `custom_background`: required only when `background` is `custom hex`; must use `#RRGGBB`.

Canvas size presets:

- `square`: `1024x1024`
- `portrait`: `1080x1920`
- `landscape`: `1920x1080`

Background choices:

- `white (#ffffff)`
- `black (#000000)`
- `warm paper (#f8f1df)`
- `pale blue (#dbeafe)`
- `pale green (#dcfce7)`
- `pale pink (#fce7f3)`
- `custom hex`

### Build Verification

Build verification lets users inspect the running bot build and compare it against a published release artifact.

Commands:

- `/grate verify`

`/grate verify` reports the running bot's Cargo package version, source ref, build commit, build input state, and SHA-256 checksum of the executable. Users can compare the checksum against the binary you publish for a release.

## Server Buttons

Lobby and status messages in the server include `Join`, `Status`, and `Start` buttons, so players do not need to type every command. The game reveal is posted in the channel where `/grate grateic create` was run.

DM assignment messages include a `Status` button for checking the game without returning to the server channel.

## Game Rules

Games are stored in memory and reset when the bot restarts. Only one Grateic game can be active per server, and a Discord user can only be enrolled in one active Grateic game across the bot.

Player submissions happen in DMs. Players are treated as ready when they join. If the bot cannot DM someone on start, it rolls the game back to the lobby and marks that player unready. After they enable DMs from the server, they can run `/grate grateic ready`; then the host can try `/grate grateic start` again.

On start, the bot generates a blank PNG canvas from the chosen size preset and background color, sends it to every player, and asks every player for an initial prompt.

Each chain starts with one player's prompt, then rotates through the players as alternating drawing and prompt rounds:

1. A player who receives text draws it and uploads an image.
2. A player who receives an image describes it for the next player to draw.
3. The chain keeps rotating until the original prompt author receives the final drawing.
4. The original prompt author gives that final drawing a name/title.

For `N` players, the game runs `2N + 1` rounds. After every chain is titled, the bot reveals how each starting prompt transformed.

If the bot cannot DM a next-round assignment, the game does not advance. Players can fix DMs and have any player DM the bot again to retry assignment delivery.

## Validation

The bot rejects duplicate submissions, submissions from non-players, text when an image is required, image-less messages when a drawing is required, invalid custom hex colors, and submissions after the game has ended.

## Setup

1. Create a Discord application and bot token in the Discord Developer Portal.
2. Enable the bot's Message Content Intent so it can read DM submissions.
3. Copy `.env.example` to `.env` and set `DISCORD_TOKEN`.
4. Run the bot:

```sh
cargo run
```

Slash commands are registered globally, so Discord can take a little while to show new or changed commands.

## Testing

```sh
cargo test
```
