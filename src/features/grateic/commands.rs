use super::{
    canvas::{CanvasPreset, generate_canvas_png, parse_hex_color},
    state::{
        Advance, CanvasConfig, Chain, ChainEntry, Game, GameError, GameKey, GamePhase,
        RoundAssignment, RoundKind, ShortDrawingAssignment, ShortShowcase, StartRound,
        SubmissionKind,
    },
};
use crate::{
    bot::{Context, Data, ensure_command_channel},
    settings::ChannelFamily,
};
use anyhow::anyhow;
use poise::serenity_prelude as serenity;
use serenity::{
    ButtonStyle, ChannelId, CreateActionRow, CreateAllowedMentions, CreateAttachment, CreateButton,
    CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage,
    EditInteractionResponse, EditMessage, Interaction, Message, MessageId, UserId,
};
use std::time::Duration;

type Error = anyhow::Error;
const BUTTON_PREFIX: &str = "grateic:";
const PROMPT_CHARACTER_LIMIT: usize = 140;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonAction {
    Join,
    Leave,
    Status,
    Start,
    Cancel,
}

impl ButtonAction {
    fn slug(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::Leave => "leave",
            Self::Status => "status",
            Self::Start => "start",
            Self::Cancel => "cancel",
        }
    }

    fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "join" => Some(Self::Join),
            "leave" => Some(Self::Leave),
            "status" => Some(Self::Status),
            "start" => Some(Self::Start),
            "cancel" => Some(Self::Cancel),
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
    subcommands(
        "help",
        "join",
        "ready",
        "start",
        "status",
        "cancel",
        "force_cancel",
        "set"
    )
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
    if !ensure_command_channel(ctx, ChannelFamily::Grateic).await? {
        return Ok(());
    }

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
        if games.values().any(|game| game.has_player(host_id)) {
            return Err(GameError::AlreadyInAnotherGame.into());
        }

        let game = match mode {
            GameModeChoice::Short => Game::new_short(key, ctx.author().id.get(), canvas.clone()),
            GameModeChoice::Full => Game::new(key, ctx.author().id.get(), canvas.clone()),
        };
        games.insert(key, game);
    }

    let content = {
        let games = ctx.data().grateic.games.read().await;
        let game = games.get(&key).ok_or(GameError::GameNotFound)?;
        format_game_status(game)
    };

    let reply = ctx
        .send(
            poise::CreateReply::default()
                .content(content)
                .allowed_mentions(no_ping_mentions())
                .components(lobby_control_components(key)),
        )
        .await?;
    let message = reply.into_message().await?;
    {
        let mut games = ctx.data().grateic.games.write().await;
        let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
        game.set_lobby_message_id(message.id.get());
    }

    Ok(())
}

#[poise::command(slash_command)]
async fn help(
    ctx: Context<'_>,
    #[description = "What to explain; defaults to overview"] topic: Option<HelpTopicChoice>,
) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Grateic).await? {
        return Ok(());
    }

    ctx.say(grateic_help_text(
        topic.unwrap_or(HelpTopicChoice::Overview),
    ))
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn join(ctx: Context<'_>) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Grateic).await? {
        return Ok(());
    }

    let key = active_game_key_from_context(ctx).await?;
    let player_id = ctx.author().id.get();

    let content = join_game(ctx.data(), key, player_id).await?;
    update_lobby_message(ctx.serenity_context(), ctx.data(), key).await?;

    ctx.send(
        poise::CreateReply::default()
            .content(content)
            .allowed_mentions(no_ping_mentions())
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn ready(ctx: Context<'_>) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Grateic).await? {
        return Ok(());
    }

    let key = active_game_key_from_context(ctx).await?;
    let player_id = ctx.author().id.get();

    {
        let games = ctx.data().grateic.games.read().await;
        let game = games.get(&key).ok_or(GameError::GameNotFound)?;

        if game.phase != GamePhase::Lobby {
            return Err(GameError::NotInLobby.into());
        }

        if !game.has_player(player_id) {
            return Err(GameError::NotAPlayer.into());
        }
    }

    if let Err(error) = send_ready_dm(ctx.serenity_context(), player_id).await {
        ctx.send(
            poise::CreateReply::default()
                .content(format!(
                    "I still cannot DM <@{player_id}>. Enable DMs from this server, then run `/grate grateic ready` again. ({error})"
                ))
                .allowed_mentions(no_ping_mentions())
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let (ready_count, player_count) = {
        let mut games = ctx.data().grateic.games.write().await;
        let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
        game.mark_ready(player_id)?;
        (game.ready_count(), game.players.len())
    };
    update_lobby_message(ctx.serenity_context(), ctx.data(), key).await?;

    ctx.send(
        poise::CreateReply::default()
            .content(format!(
                "<@{player_id}> is ready. Ready players: {ready_count}/{player_count}."
            ))
            .allowed_mentions(no_ping_mentions())
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn start(ctx: Context<'_>) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Grateic).await? {
        return Ok(());
    }

    let key = active_game_key_from_context(ctx).await?;
    let requester_id = ctx.author().id.get();
    let content = start_game(ctx.serenity_context(), ctx.data(), key, requester_id).await?;
    update_lobby_message(ctx.serenity_context(), ctx.data(), key).await?;

    ctx.send(
        poise::CreateReply::default()
            .content(content)
            .allowed_mentions(no_ping_mentions())
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn status(ctx: Context<'_>) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Grateic).await? {
        return Ok(());
    }

    let key = active_game_key_from_context(ctx).await?;

    let response = {
        let games = ctx.data().grateic.games.read().await;
        let game = games.get(&key).ok_or(GameError::GameNotFound)?;
        if game.phase == GamePhase::Lobby {
            "Lobby status refreshed in the original lobby message.".to_owned()
        } else if game.has_player(ctx.author().id.get()) {
            format_dm_status(game, ctx.author().id.get())
        } else {
            return Err(GameError::NotAPlayer.into());
        }
    };
    update_lobby_message(ctx.serenity_context(), ctx.data(), key).await?;

    ctx.send(
        poise::CreateReply::default()
            .content(response)
            .allowed_mentions(no_ping_mentions())
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
async fn cancel(ctx: Context<'_>) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Grateic).await? {
        return Ok(());
    }

    let key = active_game_key_from_context(ctx).await?;
    let requester_id = ctx.author().id.get();

    let cancelled_game = {
        let mut games = ctx.data().grateic.games.write().await;
        let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
        game.cancel(requester_id)?;
        games.remove(&key).ok_or(GameError::GameNotFound)?
    };

    edit_lobby_message_for_game(
        ctx.serenity_context(),
        &cancelled_game,
        "Grateic Phone game cancelled.",
        Vec::new(),
    )
    .await?;
    ctx.send(
        poise::CreateReply::default()
            .content("Grateic Phone game cancelled.")
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
async fn force_cancel(ctx: Context<'_>) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Grateic).await? {
        return Ok(());
    }

    let key = active_game_key_from_context(ctx).await?;
    let requester_id = ctx.author().id.get();

    let cancelled_game = force_cancel_game(ctx.data(), key, requester_id).await?;

    edit_lobby_message_for_game(
        ctx.serenity_context(),
        &cancelled_game,
        "Grateic Phone game force-cancelled.",
        Vec::new(),
    )
    .await?;
    ctx.send(
        poise::CreateReply::default()
            .content("Grateic Phone game force-cancelled.")
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command, subcommands("set_channel"))]
async fn set(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "channel",
    description_localized("en-US", "Set the only channel where Grateic commands work")
)]
async fn set_channel(
    ctx: Context<'_>,
    #[description = "Channel where Grateic commands should work"] channel: serenity::GuildChannel,
) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Grateic).await? {
        return Ok(());
    }

    let guild_id = guild_id_from_context(ctx)?;
    ctx.data()
        .settings
        .set_channel(guild_id, ChannelFamily::Grateic, channel.id.get())
        .await?;
    ctx.send(
        poise::CreateReply::default()
            .content(format!(
                "Grateic commands now only work in <#{}>.",
                channel.id.get()
            ))
            .ephemeral(true),
    )
    .await?;
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
                (game.phase == GamePhase::InProgress && game.has_player(player_id)).then_some(*key)
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
        submit_message_to_game(game, message, player_id)
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

fn submit_message_to_game(
    game: &mut Game,
    message: &Message,
    player_id: u64,
) -> Result<(Advance, Option<String>), Error> {
    if let Some(pending_advance) = game.pending_next_round() {
        return Ok((pending_advance, None));
    }

    match game.round_kind() {
        RoundKind::Prompt | RoundKind::Naming => {
            let text = validate_text_message(message)?;

            game.submit_text(player_id, text.to_owned())
                .map(|advance| (advance, None))
                .map_err(Into::into)
        }
        RoundKind::Drawing => {
            let attachment = message
                .attachments
                .iter()
                .find(|attachment| is_image_attachment(attachment))
                .ok_or_else(|| anyhow!("This round needs an image attachment."))?;

            validate_canvas_size(&game.canvas, attachment.dimensions())
                .map_err(|error| anyhow!(error))?;
            let submission_note = canvas_size_constraint_submission_text(&game.canvas);

            game.submit_drawing(
                player_id,
                attachment.url.clone(),
                attachment.filename.clone(),
            )
            .map(|advance| (advance, Some(submission_note)))
            .map_err(Into::into)
        }
    }
}

fn validate_text_message(message: &Message) -> Result<String, Error> {
    validate_text_submission_content(&message.content, !message.sticker_items.is_empty())
}

fn validate_text_submission_content(content: &str, has_sticker: bool) -> Result<String, Error> {
    if has_sticker {
        return Err(anyhow!("Discord stickers cannot be used as prompts."));
    }

    let text = content.trim();
    if text.is_empty() {
        return Err(anyhow!("This round needs a text reply."));
    }

    if text.chars().count() > PROMPT_CHARACTER_LIMIT {
        return Err(anyhow!(
            "Text submissions must be {PROMPT_CHARACTER_LIMIT} characters or fewer."
        ));
    }

    Ok(text.to_owned())
}

fn is_image_attachment(attachment: &serenity::Attachment) -> bool {
    attachment
        .content_type
        .as_deref()
        .is_some_and(|content_type| content_type.starts_with("image/"))
        || matches!(
            attachment
                .filename
                .rsplit_once('.')
                .map(|(_, extension)| extension.to_ascii_lowercase()),
            Some(extension) if matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp")
        )
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
            Ok(content) => {
                let _ = update_lobby_message(ctx, data, key).await;
                content
            }
            Err(error) => error.to_string(),
        };

        component
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new()
                    .content(content)
                    .allowed_mentions(no_ping_mentions()),
            )
            .await?;

        return Ok(());
    }

    let content = match action {
        ButtonAction::Join => match toggle_lobby_membership(data, key, user_id).await {
            Ok(content) => {
                let _ = update_lobby_message(ctx, data, key).await;
                content
            }
            Err(error) => error.to_string(),
        },
        ButtonAction::Leave => match leave_game(data, key, user_id).await {
            Ok(content) => {
                let _ = update_lobby_message(ctx, data, key).await;
                content
            }
            Err(error) => error.to_string(),
        },
        ButtonAction::Start => unreachable!("start buttons are deferred before dispatch"),
        ButtonAction::Cancel => match cancel_game(data, key, user_id).await {
            Ok(game) => {
                let _ = edit_lobby_message_for_game(
                    ctx,
                    &game,
                    "Grateic Phone game cancelled.",
                    Vec::new(),
                )
                .await;
                "Grateic Phone game cancelled.".to_owned()
            }
            Err(error) => error.to_string(),
        },
        ButtonAction::Status => {
            let games = data.grateic.games.read().await;
            if let Some(game) = games.get(&key) {
                if game.has_player(user_id) {
                    if game.phase == GamePhase::Lobby {
                        drop(games);
                        let _ = update_lobby_message(ctx, data, key).await;
                        "Lobby status refreshed in the original lobby message.".to_owned()
                    } else {
                        format_dm_status(game, user_id)
                    }
                } else {
                    "This status button belongs to a Grateic Phone game you are not in.".to_owned()
                }
            } else {
                "That Grateic Phone game is no longer active.".to_owned()
            }
        }
    };

    component
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(content)
                    .allowed_mentions(no_ping_mentions())
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
            .any(|(game_key, game)| *game_key != key && game.has_player(player_id))
        {
            return Err(GameError::AlreadyInAnotherGame.into());
        }

        let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
        game.join(player_id)?;
        game.players.len()
    };

    Ok(format!(
        "<@{player_id}> joined the Grateic Phone lobby. Players: {player_count}."
    ))
}

async fn leave_game(data: &Data, key: GameKey, player_id: u64) -> Result<String, Error> {
    let player_count = {
        let mut games = data.grateic.games.write().await;
        let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
        game.leave(player_id)?;
        game.players.len()
    };

    Ok(format!(
        "<@{player_id}> left the Grateic Phone lobby. Players: {player_count}."
    ))
}

async fn toggle_lobby_membership(
    data: &Data,
    key: GameKey,
    player_id: u64,
) -> Result<String, Error> {
    let mut games = data.grateic.games.write().await;

    if games
        .iter()
        .any(|(game_key, game)| *game_key != key && game.has_player(player_id))
    {
        return Err(GameError::AlreadyInAnotherGame.into());
    }

    let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
    if game.has_player(player_id) {
        game.leave(player_id)?;
        Ok(format!(
            "<@{player_id}> left the Grateic Phone lobby. Players: {}.",
            game.players.len()
        ))
    } else {
        game.join(player_id)?;
        Ok(format!(
            "<@{player_id}> joined the Grateic Phone lobby. Players: {}.",
            game.players.len()
        ))
    }
}

async fn cancel_game(data: &Data, key: GameKey, requester_id: u64) -> Result<Game, Error> {
    let mut games = data.grateic.games.write().await;
    let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
    game.cancel(requester_id)?;
    games
        .remove(&key)
        .ok_or_else(|| GameError::GameNotFound.into())
}

async fn force_cancel_game(data: &Data, key: GameKey, requester_id: u64) -> Result<Game, Error> {
    let mut games = data.grateic.games.write().await;
    let game = games.get_mut(&key).ok_or(GameError::GameNotFound)?;
    game.force_cancel(requester_id)?;
    games
        .remove(&key)
        .ok_or_else(|| GameError::GameNotFound.into())
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
            "DM check passed. You are ready for the Grateic Phone game.",
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

async fn update_lobby_message(
    ctx: &serenity::Context,
    data: &Data,
    key: GameKey,
) -> Result<(), Error> {
    let game = {
        let games = data.grateic.games.read().await;
        games.get(&key).cloned()
    };

    let Some(game) = game else {
        return Ok(());
    };

    let components = if game.phase == GamePhase::Lobby {
        lobby_control_components(key)
    } else {
        Vec::new()
    };
    edit_lobby_message_for_game(ctx, &game, &format_game_status(&game), components).await
}

async fn edit_lobby_message_for_game(
    ctx: &serenity::Context,
    game: &Game,
    content: &str,
    components: Vec<CreateActionRow>,
) -> Result<(), Error> {
    let Some(message_id) = game.lobby_message_id else {
        return Ok(());
    };

    ChannelId::new(game.key.channel_id)
        .edit_message(
            &ctx.http,
            MessageId::new(message_id),
            EditMessage::new()
                .content(content)
                .allowed_mentions(no_ping_mentions())
                .components(components),
        )
        .await
        .ok();

    Ok(())
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
                    "Your Grateic Phone canvas is {}x{} with background {}. {}\n\n{}",
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
                .allowed_mentions(no_ping_mentions())
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
                    "Your short Grateic Phone canvas is {}x{} with background {}. {}\n\nRound 1: reply here with one prompt. After everyone submits, another player will draw it.",
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
                .allowed_mentions(no_ping_mentions())
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
                    .allowed_mentions(no_ping_mentions())
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
                        "Short Grateic Phone drawing round: draw this prompt from another player, then upload your image here. {}\n\n{}",
                        drawing_round_constraint_text(canvas.require_canvas_size, width, height),
                        assignment.prompt
                    ))
                    .allowed_mentions(no_ping_mentions())
                    .components(status_button_components(key)),
            )
            .await?;
    }

    Ok(())
}

fn status_button_components(key: GameKey) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![status_button(key)])]
}

fn lobby_control_components(key: GameKey) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(button_custom_id(ButtonAction::Join, key))
            .label("Join / Leave")
            .style(ButtonStyle::Success),
        status_button(key),
        CreateButton::new(button_custom_id(ButtonAction::Start, key))
            .label("Start")
            .style(ButtonStyle::Primary),
        CreateButton::new(button_custom_id(ButtonAction::Cancel, key))
            .label("Cancel")
            .style(ButtonStyle::Danger),
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

fn no_ping_mentions() -> CreateAllowedMentions {
    CreateAllowedMentions::new()
}

fn format_game_status(game: &Game) -> String {
    let (width, height) = game.canvas.preset.dimensions();
    let waiting_count = game.players.len().saturating_sub(game.submitted_count());
    let start_hint = if game.phase == GamePhase::Lobby {
        if game.players.len() < 2 {
            "Start: waiting for at least 2 players.".to_owned()
        } else if game.unready_players().is_empty() {
            "Start: ready when the host presses Start.".to_owned()
        } else {
            format!(
                "Start: waiting for ready checks from {}.",
                mention_list(&game.unready_players())
            )
        }
    } else {
        "Start: game has already started.".to_owned()
    };

    format!(
        "Grateic Phone lobby\nMode: {}\nHost: <@{}>\nPlayers joined ({}/{} minimum): {}\nPhase: {:?}\nRound: {}/{}\nSubmitted this round: {}/{}\nWaiting for inputs: {}\nReady: {}/{}\nNeeds ready: {}\nCanvas: {} {}x{} {}\nDrawing size rule: {}\n{}",
        game.mode_label(),
        game.host_id,
        game.players.len(),
        2,
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
        canvas_size_constraint_summary(&game.canvas),
        start_hint
    )
}

fn format_dm_status(game: &Game, requester_id: u64) -> String {
    let waiting_count = game.players.len().saturating_sub(game.submitted_count());
    let requester_status = if game.phase == GamePhase::InProgress {
        if game.has_submitted(requester_id) {
            "You have submitted this round."
        } else {
            "You have not submitted this round yet."
        }
    } else {
        "This game is not currently collecting submissions."
    };

    format!(
        "Grateic Phone status\nMode: {}\nPhase: {:?}\nRound: {}/{}\nSubmitted this round: {}/{}\nWaiting for inputs: {}\n{}",
        game.mode_label(),
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
        requester_status
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
            describe_entry_for_drawing(previous)
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
                "Grateic Phone reveal: {} players, {} rounds.",
                game.players.len(),
                game.total_rounds()
            ),
        )
        .await?;

    for (chain_index, chain) in chains.iter().enumerate() {
        channel_id
            .send_message(
                &ctx.http,
                CreateMessage::new()
                    .allowed_mentions(no_ping_mentions())
                    .embed(
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
                            CreateMessage::new()
                                .allowed_mentions(no_ping_mentions())
                                .embed(
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
                            CreateMessage::new()
                                .allowed_mentions(no_ping_mentions())
                                .embed(
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
                        .send_message(
                            &ctx.http,
                            CreateMessage::new()
                                .content(format!(
                                    "{}. <@{}> drew:\n{}",
                                    entry_index + 1,
                                    entry.author_id,
                                    attachment_url
                                ))
                                .allowed_mentions(no_ping_mentions()),
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
            format!(
                "Short Grateic Phone showcase: {} drawings.",
                showcases.len()
            ),
        )
        .await?;

    for (index, showcase) in showcases.iter().enumerate() {
        channel_id
            .send_message(
                &ctx.http,
                CreateMessage::new()
                    .allowed_mentions(no_ping_mentions())
                    .embed(
                        CreateEmbed::new()
                            .title(format!("Showcase {}", index + 1))
                            .description(format!(
                                "<@{}> prompted:\n{}\n\n<@{}> drew:",
                                showcase.prompt_author_id,
                                showcase.prompt,
                                showcase.drawing_author_id
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

fn describe_entry_for_drawing(entry: &ChainEntry) -> String {
    match &entry.kind {
        SubmissionKind::Prompt(text) => format!("Prompt:\n{text}"),
        SubmissionKind::Name(text) => format!("Name:\n{text}"),
        SubmissionKind::Drawing { attachment_url, .. } => format!("Drawing:\n{attachment_url}"),
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
            "Grateic Phone help: create a lobby with `/grate create`, then players use `/grate grateic join`, and the host uses `/grate grateic start`.\n\nUse the `topic` option on `/grate grateic help` for focused help: `commands`, `create settings`, `modes`, `game flow examples`, or `canvas size rule`.\n\nDefault behavior: `/grate create` requires a mode, preset, and background. `custom_background` is only needed for `custom hex`. `require_canvas_size` defaults to enabled, so drawing uploads must match the selected canvas size unless the host turns it off."
        }
        HelpTopicChoice::Commands => {
            "Grateic Phone commands:\n`/grate create`: create a Grateic Phone lobby. Choose `mode`, `preset`, and `background`.\n`/grate grateic join`: join the active lobby in this server.\n`/grate grateic ready`: retry the DM check if the bot could not DM you.\n`/grate grateic start`: host-only; starts the active lobby after at least 2 players join.\n`/grate grateic status`: refresh lobby status before start, or privately show in-progress round status.\n`/grate grateic cancel`: host-only; cancel the active lobby before it starts.\n`/grate grateic force_cancel`: host-only; force-cancel a stuck active game.\n`/grate grateic set channel`: set the only channel where Grateic commands work.\n`/grate grateic help`: explain commands, settings, modes, and rules."
        }
        HelpTopicChoice::CreateSettings => {
            "`/grate create` settings:\n`mode`: `short` is one prompt plus one drawing. `full` is the full telephone-style chain game.\n`preset`: canvas size. `square` is 1024x1024, `portrait` is 1080x1920, `landscape` is 1920x1080.\n`background`: canvas background color preset. Choose `custom hex` only when you want your own color.\n`custom_background`: required only when `background` is `custom hex`; use `#RRGGBB`, like `#ff00aa`.\n`require_canvas_size`: optional. Defaults to `true`. When `false`, drawing uploads can use any image size."
        }
        HelpTopicChoice::Modes => {
            "Grateic Phone modes:\n`short`: everyone submits one prompt, then each player gets one prompt from another player, uploads one drawing, and the bot reveals showcases.\n`full`: everyone submits an initial prompt, chains rotate through alternating drawing and prompt rounds, then each original author names the final drawing before reveal.\n\nBoth modes use DMs for submissions. Both modes send the selected blank canvas at start. Both modes use the same canvas size rule."
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

    fn game_with_players(count: u64) -> Game {
        let mut game = Game::new(test_key(), 1, canvas(true));

        for player_id in 2..=count {
            game.join(player_id).unwrap();
        }

        game
    }

    fn test_key() -> GameKey {
        GameKey {
            guild_id: 1,
            channel_id: 10,
        }
    }

    async fn insert_game(data: &Data, game: Game) {
        data.grateic.games.write().await.insert(game.key, game);
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

    #[test]
    fn text_submission_validation_rejects_stickers() {
        let error = validate_text_submission_content("draw a house", true).unwrap_err();

        assert!(error.to_string().contains("stickers"));
    }

    #[test]
    fn text_submission_validation_caps_at_140_characters_after_trimming() {
        assert_eq!(
            validate_text_submission_content(&format!("  {}  ", "a".repeat(140)), false).unwrap(),
            "a".repeat(140)
        );

        let error = validate_text_submission_content(&"a".repeat(141), false).unwrap_err();
        assert!(error.to_string().contains("140"));
    }

    #[test]
    fn live_lobby_status_lists_joined_players() {
        let game = game_with_players(3);
        let status = format_game_status(&game);

        assert!(status.contains("Grateic Phone lobby"));
        assert!(status.contains("Players joined (3/2 minimum): <@1>, <@2>, <@3>"));
        assert!(status.contains("Ready: 3/3"));
    }

    #[test]
    fn dm_status_reports_requester_submission_state() {
        let mut game = game_with_players(2);
        game.start(1).unwrap();
        game.submit_text(1, "tiny castle".to_owned()).unwrap();

        let submitted_status = format_dm_status(&game, 1);
        let waiting_status = format_dm_status(&game, 2);

        assert!(submitted_status.contains("You have submitted this round."));
        assert!(waiting_status.contains("You have not submitted this round yet."));
    }

    #[test]
    fn drawing_assignment_omits_prompt_author() {
        let assignment = RoundAssignment {
            player_id: 2,
            chain_index: 0,
            previous_entry: Some(ChainEntry {
                author_id: 1,
                kind: SubmissionKind::Prompt("tiny castle".to_owned()),
            }),
            round_kind: RoundKind::Drawing,
        };

        let content = assignment_message_content(&assignment, true, 1024, 1024);

        assert!(content.contains("tiny castle"));
        assert!(!content.contains("<@1>"));
        assert!(!content.contains("wrote"));
    }

    #[tokio::test]
    async fn lobby_button_toggles_membership() {
        let data = Data::default();
        insert_game(&data, game_with_players(1)).await;

        let joined = toggle_lobby_membership(&data, test_key(), 2).await.unwrap();
        assert!(joined.contains("joined"));

        let left = toggle_lobby_membership(&data, test_key(), 2).await.unwrap();
        assert!(left.contains("left"));

        let games = data.grateic.games.read().await;
        let game = games.get(&test_key()).unwrap();
        assert_eq!(game.players, vec![1]);
    }

    #[tokio::test]
    async fn normal_cancel_is_lobby_only_and_force_cancel_handles_started_game() {
        let data = Data::default();
        let mut game = game_with_players(2);
        game.start(1).unwrap();
        insert_game(&data, game).await;

        let error = cancel_game(&data, test_key(), 1).await.unwrap_err();
        assert!(error.to_string().contains("already started"));

        force_cancel_game(&data, test_key(), 1).await.unwrap();
        assert!(data.grateic.games.read().await.get(&test_key()).is_none());
    }
}
