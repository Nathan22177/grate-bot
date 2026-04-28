use super::{
    canvas::{CanvasPreset, generate_canvas_png, parse_hex_color},
    state::{
        Advance, CanvasConfig, Chain, ChainEntry, Game, GameError, GameKey, GamePhase,
        RoundAssignment, RoundKind, ShortDrawingAssignment, ShortShowcase, StartRound,
        SubmissionKind,
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

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum GameModeChoice {
    #[name = "short"]
    Short,
    #[name = "full"]
    Full,
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum HelpTopicChoice {
    #[name = "overview"]
    Overview,
    #[name = "commands"]
    Commands,
    #[name = "create settings"]
    CreateSettings,
    #[name = "modes"]
    Modes,
    #[name = "game flow examples"]
    GameFlowExamples,
    #[name = "canvas size rule"]
    CanvasSizeRule,
}

#[poise::command(
    slash_command,
    subcommands("help", "join", "ready", "start", "status", "cancel")
)]
pub async fn grateic(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command)]
pub async fn create(
    ctx: Context<'_>,
    #[description = "Game mode"] mode: GameModeChoice,
    #[description = "Canvas size preset"] preset: PresetChoice,
    #[description = "Background color preset"] background: BackgroundChoice,
    #[description = "Required only when background is custom hex, in #RRGGBB format"]
    custom_background: Option<String>,
    #[description = "Require drawing uploads to match the canvas size exactly; defaults to true"]
    require_canvas_size: Option<bool>,
) -> Result<(), Error> {
    let guild_id = guild_id_from_context(ctx)?;
    let key = game_key_from_context(ctx)?;
    let background_hex = background.hex(custom_background.as_deref())?;

    let canvas = CanvasConfig {
        preset: preset.into(),
        background_hex,
        require_canvas_size: require_canvas_size.unwrap_or(true),
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

        let game = match mode {
            GameModeChoice::Short => Game::new_short(key, ctx.author().id.get(), canvas.clone()),
            GameModeChoice::Full => Game::new(key, ctx.author().id.get(), canvas.clone()),
        };
        games.insert(key, game);
    }

    let content = match mode {
        GameModeChoice::Short => format!(
            "Short Grateic lobby created by <@{}>. Everyone will write one prompt, draw one prompt from another player, then the showcases reveal. Canvas: {} {}. {} Use `/grate grateic join` to play.",
            ctx.author().id.get(),
            canvas.preset.label(),
            canvas.background_hex,
            canvas_size_constraint_summary(&canvas)
        ),
        GameModeChoice::Full => format!(
            "Grateic lobby created by <@{}>. Canvas: {} {}. {} Use `/grate grateic join` to play. If I cannot DM someone on start, they can fix DMs and run `/grate grateic ready`.",
            ctx.author().id.get(),
            canvas.preset.label(),
            canvas.background_hex,
            canvas_size_constraint_summary(&canvas)
        ),
    };

    ctx.send(
        poise::CreateReply::default()
            .content(content)
            .components(server_control_components(key)),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn help(
    ctx: Context<'_>,
    #[description = "What to explain; defaults to overview"] topic: Option<HelpTopicChoice>,
) -> Result<(), Error> {
    ctx.say(grateic_help_text(
        topic.unwrap_or(HelpTopicChoice::Overview),
    ))
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
            Ok((pending_advance, None))
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
                        .map(|advance| (advance, None))
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

                    if let Err(error) = validate_canvas_size(&game.canvas, attachment.dimensions())
                    {
                        return message
                            .channel_id
                            .say(&ctx.http, error)
                            .await
                            .map(|_| ())
                            .map_err(Into::into);
                    }

                    let submission_note = canvas_size_constraint_submission_text(&game.canvas);

                    game.submit_drawing(
                        player_id,
                        attachment.url.clone(),
                        attachment.filename.clone(),
                    )
                    .map(|advance| (advance, Some(submission_note)))
                }
            }
        }
    };

    match action {
        Ok((Advance::Waiting, submission_note)) => {
            message
                .channel_id
                .say(
                    &ctx.http,
                    accepted_submission_message(submission_note.as_deref()),
                )
                .await?;
        }
        Ok((
            Advance::NextRound {
                next_round,
                assignments,
            },
            submission_note,
        )) => {
            message
                .channel_id
                .say(
                    &ctx.http,
                    format!(
                        "{} Next round is starting.",
                        accepted_submission_message(submission_note.as_deref())
                    ),
                )
                .await?;

            let canvas = {
                let games = data.grateic.games.read().await;
                games
                    .get(key)
                    .ok_or(GameError::GameNotFound)?
                    .canvas
                    .clone()
            };

            if let Err(error) = send_assignments(ctx, *key, &canvas, &assignments).await {
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
        Ok((Advance::ShortDrawingRound { assignments }, submission_note)) => {
            message
                .channel_id
                .say(
                    &ctx.http,
                    format!(
                        "{} Drawing round is starting.",
                        accepted_submission_message(submission_note.as_deref())
                    ),
                )
                .await?;

            let canvas = {
                let games = data.grateic.games.read().await;
                games
                    .get(key)
                    .ok_or(GameError::GameNotFound)?
                    .canvas
                    .clone()
            };

            if let Err(error) =
                send_short_drawing_assignments(ctx, *key, &canvas, &assignments).await
            {
                message
                    .channel_id
                    .say(
                        &ctx.http,
                        format!(
                            "I could not deliver every short-game drawing DM, so the game has not advanced yet. Ask players to check DMs, then have any player DM me again to retry delivery. ({error})"
                        ),
                    )
                    .await?;
                return Ok(());
            }

            {
                let mut games = data.grateic.games.write().await;
                let game = games.get_mut(key).ok_or(GameError::GameNotFound)?;
                game.commit_next_round(1)?;
            }
        }
        Ok((Advance::Finished { chains }, submission_note)) => {
            let removed_game = {
                let mut games = data.grateic.games.write().await;
                games.remove(key)
            };

            message
                .channel_id
                .say(
                    &ctx.http,
                    format!(
                        "{} Game complete, revealing now.",
                        accepted_submission_message(submission_note.as_deref())
                    ),
                )
                .await?;

            if let Some(game) = removed_game {
                reveal_game(ctx, &game, chains).await?;
            }
        }
        Ok((Advance::ShortFinished { showcases }, submission_note)) => {
            let removed_game = {
                let mut games = data.grateic.games.write().await;
                games.remove(key)
            };

            message
                .channel_id
                .say(
                    &ctx.http,
                    format!(
                        "{} Short game complete, revealing showcases now.",
                        accepted_submission_message(submission_note.as_deref())
                    ),
                )
                .await?;

            if let Some(game) = removed_game {
                reveal_short_game(ctx, &game, showcases).await?;
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
        require_canvas_size: canvas.require_canvas_size,
    };

    let start_round = {
        let mut games = data.grateic.games.write().await;
        let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
        game.start(requester_id)?
    };

    match &start_round {
        StartRound::Classic(assignments) => {
            for assignment in assignments {
                if let Err(error) =
                    send_classic_start_dm(ctx, assignment.player_id, &delivery, assignment).await
                {
                    mark_failed_start(data, key, assignment.player_id).await;
                    return Ok(format!(
                        "Could not start because I cannot DM <@{}>. I marked them unready; ask them to enable DMs and run `/grate grateic ready`, then try `/grate grateic start` again. ({error})",
                        assignment.player_id
                    ));
                }
            }
        }
        StartRound::ShortPrompt => {
            for player_id in &players {
                if let Err(error) = send_short_prompt_start_dm(ctx, *player_id, &delivery).await {
                    mark_failed_start(data, key, *player_id).await;
                    return Ok(format!(
                        "Could not start because I cannot DM <@{}>. I marked them unready; ask them to enable DMs and run `/grate grateic ready`, then try `/grate grateic start` again. ({error})",
                        player_id
                    ));
                }
            }
        }
    }

    let mode = match start_round {
        StartRound::Classic(_) => "Game",
        StartRound::ShortPrompt => "Short game",
    };

    Ok(format!(
        "{mode} started with {} players. Everyone has the canvas and first prompt instructions in DM.",
        players.len()
    ))
}

struct StartDmDelivery<'a> {
    key: GameKey,
    canvas_png: &'a [u8],
    width: u32,
    height: u32,
    background_hex: &'a str,
    require_canvas_size: bool,
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

async fn mark_failed_start(data: &Data, key: GameKey, player_id: u64) {
    let mut games = data.grateic.games.write().await;
    if let Some(game) = games.get_mut(&key) {
        game.reset_to_lobby_after_failed_start(player_id);
    }
}

async fn send_classic_start_dm(
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
                    "Your Grateic canvas is {}x{} with background {}. {}\n\n{}",
                    delivery.width,
                    delivery.height,
                    delivery.background_hex,
                    canvas_size_constraint_text(
                        delivery.require_canvas_size,
                        delivery.width,
                        delivery.height,
                    ),
                    assignment_message_content(
                        assignment,
                        delivery.require_canvas_size,
                        delivery.width,
                        delivery.height,
                    )
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

async fn send_short_prompt_start_dm(
    ctx: &serenity::Context,
    player_id: u64,
    delivery: &StartDmDelivery<'_>,
) -> Result<(), Error> {
    let user_id = UserId::new(player_id);
    let channel = user_id.create_dm_channel(&ctx.http).await?;
    channel
        .send_message(
            &ctx.http,
            CreateMessage::new()
                .content(format!(
                    "Your short Grateic canvas is {}x{} with background {}. {}\n\nRound 1: reply here with one prompt. After everyone submits, another player will draw it.",
                    delivery.width,
                    delivery.height,
                    delivery.background_hex,
                    canvas_size_constraint_text(
                        delivery.require_canvas_size,
                        delivery.width,
                        delivery.height,
                    )
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
    canvas: &CanvasConfig,
    assignments: &[RoundAssignment],
) -> Result<(), Error> {
    let (width, height) = canvas.preset.dimensions();

    for assignment in assignments {
        let user_id = UserId::new(assignment.player_id);
        let channel = user_id.create_dm_channel(&ctx.http).await?;

        channel
            .send_message(
                &ctx.http,
                CreateMessage::new()
                    .content(assignment_message_content(
                        assignment,
                        canvas.require_canvas_size,
                        width,
                        height,
                    ))
                    .components(status_button_components(key)),
            )
            .await?;
    }

    Ok(())
}

async fn send_short_drawing_assignments(
    ctx: &serenity::Context,
    key: GameKey,
    canvas: &CanvasConfig,
    assignments: &[ShortDrawingAssignment],
) -> Result<(), Error> {
    let (width, height) = canvas.preset.dimensions();

    for assignment in assignments {
        let user_id = UserId::new(assignment.player_id);
        let channel = user_id.create_dm_channel(&ctx.http).await?;

        channel
            .send_message(
                &ctx.http,
                CreateMessage::new()
                    .content(format!(
                        "Short Grateic drawing round: draw this prompt from another player, then upload your image here. {}\n\n<@{}> wrote:\n{}",
                        drawing_round_constraint_text(canvas.require_canvas_size, width, height),
                        assignment.prompt_author_id,
                        assignment.prompt
                    ))
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
        "Mode: {}\nHost: <@{}>\nPlayers: {}\nPhase: {:?}\nRound: {}/{}\nSubmitted this round: {}/{}\nWaiting for inputs: {}\nReady: {}/{}\nNeeds ready: {}\nCanvas: {} {}x{} {}\nDrawing size rule: {}",
        game.mode_label(),
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
        game.canvas.background_hex,
        canvas_size_constraint_summary(&game.canvas)
    )
}

fn validate_canvas_size(
    canvas: &CanvasConfig,
    attachment_dimensions: Option<(u32, u32)>,
) -> Result<(), String> {
    if !canvas.require_canvas_size {
        return Ok(());
    }

    let (expected_width, expected_height) = canvas.preset.dimensions();
    let Some((actual_width, actual_height)) = attachment_dimensions else {
        return Err(format!(
            "This drawing must be {expected_width}x{expected_height} to match the game canvas. I could not read this file's dimensions from Discord; please upload a normal image file whose dimensions Discord can detect."
        ));
    };

    if (actual_width, actual_height) == (expected_width, expected_height) {
        return Ok(());
    }

    Err(format!(
        "This drawing must be {expected_width}x{expected_height} to match the game canvas. Your file is {actual_width}x{actual_height}. Please upload a corrected image."
    ))
}

fn accepted_submission_message(submission_note: Option<&str>) -> String {
    match submission_note {
        Some(note) => format!("Submission accepted. {note}"),
        None => "Submission accepted.".to_owned(),
    }
}

fn canvas_size_constraint_summary(canvas: &CanvasConfig) -> String {
    let (width, height) = canvas.preset.dimensions();
    canvas_size_constraint_text(canvas.require_canvas_size, width, height)
}

fn canvas_size_constraint_text(require_canvas_size: bool, width: u32, height: u32) -> String {
    if require_canvas_size {
        format!("Drawing uploads must match the canvas size exactly ({width}x{height}).")
    } else {
        "Drawing uploads may use any image size.".to_owned()
    }
}

fn canvas_size_constraint_submission_text(canvas: &CanvasConfig) -> String {
    let (width, height) = canvas.preset.dimensions();
    if canvas.require_canvas_size {
        format!("Exact canvas size is enabled for this game ({width}x{height}).")
    } else {
        "Exact canvas size is disabled for this game; image size is not restricted.".to_owned()
    }
}

fn drawing_round_constraint_text(require_canvas_size: bool, width: u32, height: u32) -> String {
    if require_canvas_size {
        format!("Upload an image exactly {width}x{height}.")
    } else {
        "Image size is not restricted for this game.".to_owned()
    }
}

fn assignment_message_content(
    assignment: &RoundAssignment,
    require_canvas_size: bool,
    width: u32,
    height: u32,
) -> String {
    match (&assignment.round_kind, &assignment.previous_entry) {
        (RoundKind::Prompt, Some(previous)) => format!(
            "Prompt round: describe this drawing so the next player can draw from your words.\n\n{}",
            describe_entry(previous)
        ),
        (RoundKind::Prompt, _) => "Round 1: reply here with your initial prompt.".to_owned(),
        (RoundKind::Drawing, Some(previous)) => format!(
            "Drawing round: draw from the latest chain entry, then upload your image here. {}\n\n{}",
            drawing_round_constraint_text(require_canvas_size, width, height),
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

async fn reveal_short_game(
    ctx: &serenity::Context,
    game: &Game,
    showcases: Vec<ShortShowcase>,
) -> Result<(), Error> {
    let channel_id = serenity::ChannelId::new(game.key.channel_id);
    channel_id
        .say(
            &ctx.http,
            format!("Short Grateic showcase: {} drawings.", showcases.len()),
        )
        .await?;

    for (index, showcase) in showcases.iter().enumerate() {
        channel_id
            .send_message(
                &ctx.http,
                CreateMessage::new().embed(
                    CreateEmbed::new()
                        .title(format!("Showcase {}", index + 1))
                        .description(format!(
                            "<@{}> prompted:\n{}\n\n<@{}> drew:",
                            showcase.prompt_author_id, showcase.prompt, showcase.drawing_author_id
                        ))
                        .image(showcase.attachment_url.clone()),
                ),
            )
            .await?;

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

fn grateic_help_text(topic: HelpTopicChoice) -> &'static str {
    match topic {
        HelpTopicChoice::Overview => {
            "Grateic help: create a lobby with `/grate create`, then players use `/grate grateic join`, and the host uses `/grate grateic start`.\n\nUse the `topic` option on `/grate grateic help` for focused help: `commands`, `create settings`, `modes`, `game flow examples`, or `canvas size rule`.\n\nDefault behavior: `/grate create` requires a mode, preset, and background. `custom_background` is only needed for `custom hex`. `require_canvas_size` defaults to enabled, so drawing uploads must match the selected canvas size unless the host turns it off."
        }
        HelpTopicChoice::Commands => {
            "Grateic commands:\n`/grate create`: create a Grateic lobby. Choose `mode`, `preset`, and `background`.\n`/grate grateic join`: join the active lobby in this server.\n`/grate grateic ready`: retry the DM check if the bot could not DM you.\n`/grate grateic start`: host-only; starts the active lobby after at least 2 players join.\n`/grate grateic status`: show host, players, mode, round, readiness, canvas, and waiting count.\n`/grate grateic cancel`: host-only; cancel the active lobby.\n`/grate grateic help`: explain commands, settings, modes, and rules."
        }
        HelpTopicChoice::CreateSettings => {
            "`/grate create` settings:\n`mode`: `short` is one prompt plus one drawing. `full` is the full telephone-style chain game.\n`preset`: canvas size. `square` is 1024x1024, `portrait` is 1080x1920, `landscape` is 1920x1080.\n`background`: canvas background color preset. Choose `custom hex` only when you want your own color.\n`custom_background`: required only when `background` is `custom hex`; use `#RRGGBB`, like `#ff00aa`.\n`require_canvas_size`: optional. Defaults to `true`. When `false`, drawing uploads can use any image size."
        }
        HelpTopicChoice::Modes => {
            "Grateic modes:\n`short`: everyone submits one prompt, then each player gets one prompt from another player, uploads one drawing, and the bot reveals showcases.\n`full`: everyone submits an initial prompt, chains rotate through alternating drawing and prompt rounds, then each original author names the final drawing before reveal.\n\nBoth modes use DMs for submissions. Both modes send the selected blank canvas at start. Both modes use the same canvas size rule."
        }
        HelpTopicChoice::GameFlowExamples => {
            "Example short flow with 3 players:\n1. A, B, and C each submit one prompt.\n2. A draws C's prompt, B draws A's prompt, C draws B's prompt.\n3. After all drawings are uploaded, the bot posts each prompt with its drawing as showcases.\n\nExample full flow with 3 players:\n1. A, B, and C each submit an initial prompt.\n2. Next round, each player draws a different prompt.\n3. Next round, each player describes a drawing for the next player.\n4. Drawing and prompt rounds keep rotating.\n5. Final round, original authors name the final drawing from their chain.\n6. The bot reveals every chain."
        }
        HelpTopicChoice::CanvasSizeRule => {
            "Canvas size rule:\nBy default, `require_canvas_size` is enabled. Drawing uploads must exactly match the canvas preset chosen in `/grate create`.\n\nPreset sizes: `square` 1024x1024, `portrait` 1080x1920, `landscape` 1920x1080.\n\nIf a submitted image is the wrong size, the bot rejects it and tells the player the expected and actual dimensions. If Discord does not report dimensions, the bot asks for a normal image upload whose dimensions Discord can detect.\n\nTo disable this rule for a lobby, set `require_canvas_size:false` when creating it."
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas(require_canvas_size: bool) -> CanvasConfig {
        CanvasConfig {
            preset: CanvasPreset::Square,
            background_hex: "#ffffff".to_owned(),
            require_canvas_size,
        }
    }

    #[test]
    fn canvas_size_validation_accepts_exact_match_when_enabled() {
        assert_eq!(
            validate_canvas_size(&canvas(true), Some((1024, 1024))),
            Ok(())
        );
    }

    #[test]
    fn canvas_size_validation_rejects_wrong_size_when_enabled() {
        let error = validate_canvas_size(&canvas(true), Some((800, 800))).unwrap_err();

        assert!(error.contains("1024x1024"));
        assert!(error.contains("800x800"));
    }

    #[test]
    fn canvas_size_validation_rejects_missing_dimensions_when_enabled() {
        let error = validate_canvas_size(&canvas(true), None).unwrap_err();

        assert!(error.contains("could not read"));
        assert!(error.contains("1024x1024"));
    }

    #[test]
    fn canvas_size_validation_accepts_any_dimensions_when_disabled() {
        assert_eq!(
            validate_canvas_size(&canvas(false), Some((800, 800))),
            Ok(())
        );
        assert_eq!(validate_canvas_size(&canvas(false), None), Ok(()));
    }
}
