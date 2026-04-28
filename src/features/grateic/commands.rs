use super::{
    canvas::{CanvasPreset, generate_canvas_png, parse_hex_color},
    state::{
        Advance, CanvasConfig, Chain, ChainEntry, Game, GameError, GameKey, GamePhase,
        RoundAssignment, RoundKind, SubmissionKind,
    },
};
use crate::bot::{Context, Data};
use anyhow::anyhow;
use poise::serenity_prelude as serenity;
use serenity::{
    ButtonStyle, CreateActionRow, CreateAttachment, CreateButton, CreateEmbed,
    CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage,
    EditInteractionResponse, Interaction, Message, UserId,
};
use std::time::Duration;

type Error = anyhow::Error;
const BUTTON_PREFIX: &str = "grateic:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonAction {
    Join,
    Status,
    Start,
}

impl ButtonAction {
    fn slug(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::Status => "status",
            Self::Start => "start",
        }
    }

    fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "join" => Some(Self::Join),
            "status" => Some(Self::Status),
            "start" => Some(Self::Start),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum PresetChoice {
    #[name = "square (1024x1024)"]
    Square,
    #[name = "portrait (1080x1920)"]
    Portrait,
    #[name = "landscape (1920x1080)"]
    Landscape,
}

impl From<PresetChoice> for CanvasPreset {
    fn from(value: PresetChoice) -> Self {
        match value {
            PresetChoice::Square => Self::Square,
            PresetChoice::Portrait => Self::Portrait,
            PresetChoice::Landscape => Self::Landscape,
        }
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum BackgroundChoice {
    #[name = "white (#ffffff)"]
    White,
    #[name = "black (#000000)"]
    Black,
    #[name = "warm paper (#f8f1df)"]
    WarmPaper,
    #[name = "pale blue (#dbeafe)"]
    PaleBlue,
    #[name = "pale green (#dcfce7)"]
    PaleGreen,
    #[name = "pale pink (#fce7f3)"]
    PalePink,
    #[name = "custom hex"]
    Custom,
}

impl BackgroundChoice {
    fn hex(self, custom_background: Option<&str>) -> Result<String, Error> {
        let hex = match self {
            Self::White => "#ffffff",
            Self::Black => "#000000",
            Self::WarmPaper => "#f8f1df",
            Self::PaleBlue => "#dbeafe",
            Self::PaleGreen => "#dcfce7",
            Self::PalePink => "#fce7f3",
            Self::Custom => custom_background.ok_or_else(|| {
                anyhow!("custom_background is required when background is custom hex")
            })?,
        };

        parse_hex_color(hex).map_err(|error| anyhow!(error))?;
        Ok(hex.to_owned())
    }
}

#[poise::command(
    slash_command,
    subcommands("create", "join", "ready", "start", "status", "cancel")
)]
pub async fn grateic(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command)]
async fn create(
    ctx: Context<'_>,
    #[description = "Canvas size preset"] preset: PresetChoice,
    #[description = "Background color preset"] background: BackgroundChoice,
    #[description = "Required only when background is custom hex, in #RRGGBB format"]
    custom_background: Option<String>,
) -> Result<(), Error> {
    let guild_id = guild_id_from_context(ctx)?;
    let key = game_key_from_context(ctx)?;
    let background_hex = background.hex(custom_background.as_deref())?;

    let canvas = CanvasConfig {
        preset: preset.into(),
        background_hex,
    };
    {
        let mut games = ctx.data().grateic.games.write().await;
        if games.keys().any(|game_key| game_key.guild_id == guild_id) {
            return Err(GameError::GameAlreadyExists.into());
        }

        let host_id = ctx.author().id.get();
        if games.values().any(|game| game.players.contains(&host_id)) {
            return Err(GameError::AlreadyInAnotherGame.into());
        }

        let game = Game::new(key, ctx.author().id.get(), canvas.clone());
        games.insert(key, game);
    }

    let content = format!(
        "Grateic lobby created by <@{}>. Canvas: {} {}. Use `/grate grateic join` to play. If I cannot DM someone on start, they can fix DMs and run `/grate grateic ready`.",
        ctx.author().id.get(),
        canvas.preset.label(),
        canvas.background_hex
    );

    ctx.send(
        poise::CreateReply::default()
            .content(content)
            .components(server_control_components(key)),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn join(ctx: Context<'_>) -> Result<(), Error> {
    let key = active_game_key_from_context(ctx).await?;
    let player_id = ctx.author().id.get();

    ctx.send(
        poise::CreateReply::default()
            .content(join_game(ctx.data(), key, player_id).await?)
            .components(server_control_components(key)),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn ready(ctx: Context<'_>) -> Result<(), Error> {
    let key = active_game_key_from_context(ctx).await?;
    let player_id = ctx.author().id.get();

    {
        let games = ctx.data().grateic.games.read().await;
        let game = games.get(&key).ok_or(GameError::GameNotFound)?;

        if game.phase != GamePhase::Lobby {
            return Err(GameError::NotInLobby.into());
        }

        if !game.players.contains(&player_id) {
            return Err(GameError::NotAPlayer.into());
        }
    }

    if let Err(error) = send_ready_dm(ctx.serenity_context(), player_id).await {
        ctx.say(format!(
            "I still cannot DM <@{player_id}>. Enable DMs from this server, then run `/grate grateic ready` again. ({error})"
        ))
        .await?;
        return Ok(());
    }

    let (ready_count, player_count) = {
        let mut games = ctx.data().grateic.games.write().await;
        let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
        game.mark_ready(player_id)?;
        (game.ready_count(), game.players.len())
    };

    ctx.send(
        poise::CreateReply::default()
            .content(format!(
                "<@{player_id}> is ready. Ready players: {ready_count}/{player_count}."
            ))
            .components(server_control_components(key)),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn start(ctx: Context<'_>) -> Result<(), Error> {
    let key = active_game_key_from_context(ctx).await?;
    let requester_id = ctx.author().id.get();

    ctx.send(
        poise::CreateReply::default()
            .content(start_game(ctx.serenity_context(), ctx.data(), key, requester_id).await?)
            .components(server_status_component(key)),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let key = active_game_key_from_context(ctx).await?;

    let response = {
        let games = ctx.data().grateic.games.read().await;
        let game = games.get(&key).ok_or(GameError::GameNotFound)?;
        format_status(game)
    };

    ctx.send(
        poise::CreateReply::default()
            .content(response)
            .components(server_control_components(key)),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
async fn cancel(ctx: Context<'_>) -> Result<(), Error> {
    let key = active_game_key_from_context(ctx).await?;
    let requester_id = ctx.author().id.get();

    {
        let mut games = ctx.data().grateic.games.write().await;
        let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
        game.cancel(requester_id)?;
        games.remove(&key);
    }

    ctx.say("Grateic game cancelled.").await?;
    Ok(())
}

pub async fn handle_message(
    ctx: &serenity::Context,
    data: &Data,
    message: &Message,
) -> Result<(), Error> {
    if message.author.bot || message.guild_id.is_some() {
        return Ok(());
    }

    let player_id = message.author.id.get();
    let matching_games = {
        let games = data.grateic.games.read().await;
        games
            .iter()
            .filter_map(|(key, game)| {
                (game.phase == GamePhase::InProgress && game.players.contains(&player_id))
                    .then_some(*key)
            })
            .collect::<Vec<_>>()
    };

    let [key] = matching_games.as_slice() else {
        if matching_games.len() > 1 {
            message
                .channel_id
                .say(
                    &ctx.http,
                    "You are in more than one active game, so I cannot tell where this submission belongs yet.",
                )
                .await?;
        }
        return Ok(());
    };

    let action = {
        let mut games = data.grateic.games.write().await;
        let game = games.get_mut(key).ok_or(GameError::GameNotFound)?;

        if let Some(pending_advance) = game.pending_next_round() {
            Ok(pending_advance)
        } else {
            match game.round_kind() {
                RoundKind::Prompt | RoundKind::Naming => {
                    let text = message.content.trim();
                    if text.is_empty() {
                        return message
                            .channel_id
                            .say(&ctx.http, "This round needs a text reply.")
                            .await
                            .map(|_| ())
                            .map_err(Into::into);
                    }

                    game.submit_text(player_id, text.to_owned())
                }
                RoundKind::Drawing => {
                    let Some(attachment) = message.attachments.iter().find(|attachment| {
                        attachment
                            .content_type
                            .as_deref()
                            .is_some_and(|content_type| content_type.starts_with("image/"))
                            || attachment.filename.to_ascii_lowercase().ends_with(".png")
                            || attachment.filename.to_ascii_lowercase().ends_with(".jpg")
                            || attachment.filename.to_ascii_lowercase().ends_with(".jpeg")
                            || attachment.filename.to_ascii_lowercase().ends_with(".webp")
                    }) else {
                        return message
                            .channel_id
                            .say(&ctx.http, "This round needs an image attachment.")
                            .await
                            .map(|_| ())
                            .map_err(Into::into);
                    };

                    game.submit_drawing(
                        player_id,
                        attachment.url.clone(),
                        attachment.filename.clone(),
                    )
                }
            }
        }
    };

    match action {
        Ok(Advance::Waiting) => {
            message
                .channel_id
                .say(&ctx.http, "Submission accepted.")
                .await?;
        }
        Ok(Advance::NextRound {
            next_round,
            assignments,
        }) => {
            message
                .channel_id
                .say(&ctx.http, "Submission accepted. Next round is starting.")
                .await?;

            if let Err(error) = send_assignments(ctx, *key, &assignments).await {
                message
                    .channel_id
                    .say(
                        &ctx.http,
                        format!(
                            "I could not deliver every next-round DM, so the game has not advanced yet. Ask players to check DMs, then have any player DM me again to retry delivery. ({error})"
                        ),
                    )
                    .await?;
                return Ok(());
            }

            {
                let mut games = data.grateic.games.write().await;
                let game = games.get_mut(key).ok_or(GameError::GameNotFound)?;
                game.commit_next_round(next_round)?;
            }
        }
        Ok(Advance::Finished { chains }) => {
            let removed_game = {
                let mut games = data.grateic.games.write().await;
                games.remove(key)
            };

            message
                .channel_id
                .say(
                    &ctx.http,
                    "Submission accepted. Game complete, revealing now.",
                )
                .await?;

            if let Some(game) = removed_game {
                reveal_game(ctx, &game, chains).await?;
            }
        }
        Err(error) => {
            message.channel_id.say(&ctx.http, error.to_string()).await?;
        }
    }

    Ok(())
}

pub async fn handle_interaction(
    ctx: &serenity::Context,
    data: &Data,
    interaction: &Interaction,
) -> Result<(), Error> {
    let Interaction::Component(component) = interaction else {
        return Ok(());
    };

    let Some((action, key)) = parse_button_custom_id(&component.data.custom_id) else {
        return Ok(());
    };

    let user_id = component.user.id.get();
    if action == ButtonAction::Start {
        component
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Defer(
                    CreateInteractionResponseMessage::new().ephemeral(true),
                ),
            )
            .await?;

        let content = match start_game(ctx, data, key, user_id).await {
            Ok(content) => content,
            Err(error) => error.to_string(),
        };

        component
            .edit_response(&ctx.http, EditInteractionResponse::new().content(content))
            .await?;

        return Ok(());
    }

    let content = match action {
        ButtonAction::Join => match join_game(data, key, user_id).await {
            Ok(content) => content,
            Err(error) => error.to_string(),
        },
        ButtonAction::Start => unreachable!("start buttons are deferred before dispatch"),
        ButtonAction::Status => {
            let games = data.grateic.games.read().await;
            if let Some(game) = games.get(&key) {
                if game.players.contains(&user_id) {
                    format_status(game)
                } else {
                    "This status button belongs to a Grateic game you are not in.".to_owned()
                }
            } else {
                "That Grateic game is no longer active.".to_owned()
            }
        }
    };

    component
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(content)
                    .ephemeral(true),
            ),
        )
        .await?;

    Ok(())
}

async fn join_game(data: &Data, key: GameKey, player_id: u64) -> Result<String, Error> {
    let player_count = {
        let mut games = data.grateic.games.write().await;
        if games
            .iter()
            .any(|(game_key, game)| *game_key != key && game.players.contains(&player_id))
        {
            return Err(GameError::AlreadyInAnotherGame.into());
        }

        let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
        game.join(player_id)?;
        game.players.len()
    };

    Ok(format!(
        "<@{player_id}> joined the Grateic lobby. Players: {player_count}."
    ))
}

async fn start_game(
    ctx: &serenity::Context,
    data: &Data,
    key: GameKey,
    requester_id: u64,
) -> Result<String, Error> {
    let (canvas, players) = {
        let games = data.grateic.games.read().await;
        let game = games.get(&key).ok_or(GameError::GameNotFound)?;

        if requester_id != game.host_id {
            return Err(GameError::NotHost.into());
        }

        if game.players.len() < 2 {
            return Err(GameError::NotEnoughPlayers.into());
        }

        if game.phase != GamePhase::Lobby {
            return Err(GameError::AlreadyStarted.into());
        }

        let unready_players = game.unready_players();
        if !unready_players.is_empty() {
            return Ok(format!(
                "Cannot start yet. I could not DM these players before. They need to enable DMs from this server and run `/grate grateic ready`: {}",
                mention_list(&unready_players)
            ));
        }

        (game.canvas.clone(), game.players.clone())
    };

    let background = parse_hex_color(&canvas.background_hex).map_err(|error| anyhow!(error))?;
    let canvas_png = generate_canvas_png(canvas.preset, background)?;
    let (width, height) = canvas.preset.dimensions();
    let delivery = StartDmDelivery {
        key,
        canvas_png: &canvas_png,
        width,
        height,
        background_hex: &canvas.background_hex,
    };

    let assignments = {
        let mut games = data.grateic.games.write().await;
        let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
        game.start(requester_id)?
    };

    for assignment in &assignments {
        if let Err(error) = send_start_dm(ctx, assignment.player_id, &delivery, assignment).await {
            {
                let mut games = data.grateic.games.write().await;
                if let Some(game) = games.get_mut(&key) {
                    game.reset_to_lobby_after_failed_start(assignment.player_id);
                }
            }

            return Ok(format!(
                "Could not start because I cannot DM <@{}>. I marked them unready; ask them to enable DMs and run `/grate grateic ready`, then try `/grate grateic start` again. ({error})",
                assignment.player_id
            ));
        }
    }

    Ok(format!(
        "Game started with {} players. Everyone has the canvas and first prompt instructions in DM.",
        players.len()
    ))
}

struct StartDmDelivery<'a> {
    key: GameKey,
    canvas_png: &'a [u8],
    width: u32,
    height: u32,
    background_hex: &'a str,
}

async fn send_ready_dm(ctx: &serenity::Context, player_id: u64) -> Result<(), Error> {
    let user_id = UserId::new(player_id);
    let channel = user_id.create_dm_channel(&ctx.http).await?;
    channel
        .say(
            &ctx.http,
            "DM check passed. You are ready for the Grateic game.",
        )
        .await?;

    Ok(())
}

async fn send_start_dm(
    ctx: &serenity::Context,
    player_id: u64,
    delivery: &StartDmDelivery<'_>,
    assignment: &RoundAssignment,
) -> Result<(), Error> {
    let user_id = UserId::new(player_id);
    let channel = user_id.create_dm_channel(&ctx.http).await?;
    channel
        .send_message(
            &ctx.http,
            CreateMessage::new()
                .content(format!(
                    "Your Grateic canvas is {}x{} with background {}.\n\n{}",
                    delivery.width,
                    delivery.height,
                    delivery.background_hex,
                    assignment_message_content(assignment)
                ))
                .add_file(CreateAttachment::bytes(
                    delivery.canvas_png.to_vec(),
                    "grateic-canvas.png",
                ))
                .components(status_button_components(delivery.key)),
        )
        .await?;

    Ok(())
}

async fn send_assignments(
    ctx: &serenity::Context,
    key: GameKey,
    assignments: &[RoundAssignment],
) -> Result<(), Error> {
    for assignment in assignments {
        let user_id = UserId::new(assignment.player_id);
        let channel = user_id.create_dm_channel(&ctx.http).await?;

        channel
            .send_message(
                &ctx.http,
                CreateMessage::new()
                    .content(assignment_message_content(assignment))
                    .components(status_button_components(key)),
            )
            .await?;
    }

    Ok(())
}

fn status_button_components(key: GameKey) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![status_button(key)])]
}

fn server_status_component(key: GameKey) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![status_button(key)])]
}

fn server_control_components(key: GameKey) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(button_custom_id(ButtonAction::Join, key))
            .label("Join")
            .style(ButtonStyle::Success),
        status_button(key),
        CreateButton::new(button_custom_id(ButtonAction::Start, key))
            .label("Start")
            .style(ButtonStyle::Primary),
    ])]
}

fn status_button(key: GameKey) -> CreateButton {
    CreateButton::new(button_custom_id(ButtonAction::Status, key))
        .label("Status")
        .style(ButtonStyle::Secondary)
}

fn button_custom_id(action: ButtonAction, key: GameKey) -> String {
    format!(
        "{BUTTON_PREFIX}{}:{}:{}",
        action.slug(),
        key.guild_id,
        key.channel_id
    )
}

fn parse_button_custom_id(custom_id: &str) -> Option<(ButtonAction, GameKey)> {
    let payload = custom_id.strip_prefix(BUTTON_PREFIX)?;
    let mut parts = payload.split(':');
    let action = ButtonAction::from_slug(parts.next()?)?;
    let guild_id = parts.next()?.parse().ok()?;
    let channel_id = parts.next()?.parse().ok()?;

    if parts.next().is_some() {
        return None;
    }

    Some((
        action,
        GameKey {
            guild_id,
            channel_id,
        },
    ))
}

fn format_status(game: &Game) -> String {
    let (width, height) = game.canvas.preset.dimensions();
    let waiting_count = game.players.len().saturating_sub(game.submitted_count());

    format!(
        "Host: <@{}>\nPlayers: {}\nPhase: {:?}\nRound: {}/{}\nSubmitted this round: {}/{}\nWaiting for inputs: {}\nReady: {}/{}\nNeeds ready: {}\nCanvas: {} {}x{} {}",
        game.host_id,
        game.players
            .iter()
            .map(|player_id| format!("<@{player_id}>"))
            .collect::<Vec<_>>()
            .join(", "),
        game.phase,
        if game.phase == GamePhase::Lobby {
            0
        } else {
            game.current_round + 1
        },
        game.total_rounds(),
        game.submitted_count(),
        game.players.len(),
        if game.phase == GamePhase::InProgress {
            waiting_count.to_string()
        } else {
            "not in progress".to_owned()
        },
        game.ready_count(),
        game.players.len(),
        mention_list(&game.unready_players()),
        game.canvas.preset.label(),
        width,
        height,
        game.canvas.background_hex
    )
}

fn assignment_message_content(assignment: &RoundAssignment) -> String {
    match (&assignment.round_kind, &assignment.previous_entry) {
        (RoundKind::Prompt, Some(previous)) => format!(
            "Prompt round: describe this drawing so the next player can draw from your words.\n\n{}",
            describe_entry(previous)
        ),
        (RoundKind::Prompt, _) => "Round 1: reply here with your initial prompt.".to_owned(),
        (RoundKind::Drawing, Some(previous)) => format!(
            "Drawing round: draw from the latest chain entry, then upload your image here.\n\n{}",
            describe_entry(previous)
        ),
        (RoundKind::Naming, Some(previous)) => format!(
            "Final naming round: this is what your original prompt became. Reply with a name/title for it.\n\n{}",
            describe_entry(previous)
        ),
        (_, None) => "Your next turn is ready. Reply here with your submission.".to_owned(),
    }
}

async fn reveal_game(
    ctx: &serenity::Context,
    game: &Game,
    chains: Vec<Chain>,
) -> Result<(), Error> {
    let channel_id = serenity::ChannelId::new(game.key.channel_id);
    channel_id
        .say(
            &ctx.http,
            format!(
                "Grateic reveal: {} players, {} rounds.",
                game.players.len(),
                game.total_rounds()
            ),
        )
        .await?;

    for (chain_index, chain) in chains.iter().enumerate() {
        channel_id
            .send_message(
                &ctx.http,
                CreateMessage::new().embed(
                    CreateEmbed::new()
                        .title(format!("Chain {}", chain_index + 1))
                        .description(format!("Started by <@{}>", chain.original_player_id)),
                ),
            )
            .await?;

        for (entry_index, entry) in chain.entries.iter().enumerate() {
            match &entry.kind {
                SubmissionKind::Prompt(text) => {
                    channel_id
                        .send_message(
                            &ctx.http,
                            CreateMessage::new().embed(
                                CreateEmbed::new()
                                    .title(format!(
                                        "{}. <@{}> wrote",
                                        entry_index + 1,
                                        entry.author_id
                                    ))
                                    .description(text),
                            ),
                        )
                        .await?;
                }
                SubmissionKind::Name(text) => {
                    channel_id
                        .send_message(
                            &ctx.http,
                            CreateMessage::new().embed(
                                CreateEmbed::new()
                                    .title(format!(
                                        "{}. <@{}> named it",
                                        entry_index + 1,
                                        entry.author_id
                                    ))
                                    .description(text),
                            ),
                        )
                        .await?;
                }
                SubmissionKind::Drawing { attachment_url, .. } => {
                    channel_id
                        .say(
                            &ctx.http,
                            format!(
                                "{}. <@{}> drew:\n{}",
                                entry_index + 1,
                                entry.author_id,
                                attachment_url
                            ),
                        )
                        .await?;
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(750)).await;
    }

    Ok(())
}

fn describe_entry(entry: &ChainEntry) -> String {
    match &entry.kind {
        SubmissionKind::Prompt(text) => format!("<@{}> wrote:\n{}", entry.author_id, text),
        SubmissionKind::Name(text) => format!("<@{}> named it:\n{}", entry.author_id, text),
        SubmissionKind::Drawing { attachment_url, .. } => {
            format!("<@{}> drew:\n{}", entry.author_id, attachment_url)
        }
    }
}

fn mention_list(user_ids: &[u64]) -> String {
    if user_ids.is_empty() {
        return "nobody".to_owned();
    }

    user_ids
        .iter()
        .map(|user_id| format!("<@{user_id}>"))
        .collect::<Vec<_>>()
        .join(", ")
}

async fn active_game_key_from_context(ctx: Context<'_>) -> Result<GameKey, Error> {
    let guild_id = guild_id_from_context(ctx)?;
    active_game_key_for_guild(ctx.data(), guild_id).await
}

async fn active_game_key_for_guild(data: &Data, guild_id: u64) -> Result<GameKey, Error> {
    let games = data.grateic.games.read().await;
    games
        .keys()
        .find(|key| key.guild_id == guild_id)
        .copied()
        .ok_or_else(|| GameError::GameNotFound.into())
}

fn guild_id_from_context(ctx: Context<'_>) -> Result<u64, Error> {
    Ok(ctx.guild_id().ok_or(GameError::MissingGuild)?.get())
}

fn game_key_from_context(ctx: Context<'_>) -> Result<GameKey, Error> {
    Ok(GameKey {
        guild_id: guild_id_from_context(ctx)?,
        channel_id: ctx.channel_id().get(),
    })
}
