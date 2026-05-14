use crate::{
    features::grateic::{self as grateic_feature, create, grateic},
    settings::{ChannelFamily, SettingsStore},
};
use anyhow::Context as AnyhowContext;
use poise::serenity_prelude as serenity;
use serenity::{ChannelId, FullEvent, GatewayIntents, GuildId};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, path::Path};

use crate::features::hytale::hytale;

type Error = anyhow::Error;
pub(crate) type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(Debug, Clone, Default)]
pub struct Data {
    pub(crate) grateic: grateic_feature::State,
    pub(crate) settings: SettingsStore,
}

pub async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let token = std::env::var("DISCORD_TOKEN")
        .context("DISCORD_TOKEN is missing. Add it to your environment or .env file.")?;

    let intents =
        GatewayIntents::GUILDS | GatewayIntents::DIRECT_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![grate()],
            event_handler: |ctx, event, _framework, data| {
                Box::pin(async move {
                    match event {
                        FullEvent::Message { new_message } => {
                            grateic_feature::handle_message(ctx, data, new_message).await?;
                        }
                        FullEvent::InteractionCreate { interaction } => {
                            grateic_feature::handle_interaction(ctx, data, interaction).await?;
                        }
                        _ => {}
                    }

                    Ok(())
                })
            },
            ..Default::default()
        })
        .setup(|ctx, ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                println!("{} is online", ready.user.name);

                Ok(Data {
                    grateic: grateic_feature::State::default(),
                    settings: SettingsStore::load_from_env().await?,
                })
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await?;

    client.start().await?;
    Ok(())
}

#[poise::command(
    slash_command,
    subcommands("help", "create", "grateic", "verify", "hytale")
)]
async fn grate(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command)]
async fn help(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(help_message()).await?;
    Ok(())
}

#[poise::command(slash_command)]
async fn verify(ctx: Context<'_>) -> Result<(), Error> {
    let exe_path = std::env::current_exe().context("could not locate the running executable")?;
    let checksum = sha256_file(&exe_path)
        .with_context(|| format!("could not checksum {}", exe_path.display()))?;

    ctx.say(format!(
        "Build verification\nRelease status: {}\nVersion: `{}`\nSource ref: `{}`\nCommit: `{}`\nBuild inputs: `{}`\nExecutable SHA-256: `{}`\nRelease: {}\nRelease checksum: {}",
        release_status(),
        env!("CARGO_PKG_VERSION"),
        option_env!("BUILD_SOURCE_REF").unwrap_or("unknown"),
        option_env!("BUILD_COMMIT").unwrap_or("unknown"),
        option_env!("BUILD_INPUT_STATE").unwrap_or("unknown"),
        checksum,
        release_url(),
        release_checksum_url()
    ))
    .await?;

    Ok(())
}

fn release_status() -> &'static str {
    let tag = option_env!("BUILD_RELEASE_TAG").unwrap_or("unknown");

    if tag == "unknown" {
        "⚠️ branch or local build"
    } else {
        "✅ official GitHub release"
    }
}

fn release_url() -> String {
    let repository = option_env!("BUILD_REPOSITORY").unwrap_or("unknown");
    let tag = option_env!("BUILD_RELEASE_TAG").unwrap_or("unknown");

    if repository == "unknown" || tag == "unknown" {
        "`unknown`".to_owned()
    } else {
        format!("https://github.com/{repository}/releases/tag/{tag}")
    }
}

fn release_checksum_url() -> String {
    let repository = option_env!("BUILD_REPOSITORY").unwrap_or("unknown");
    let tag = option_env!("BUILD_RELEASE_TAG").unwrap_or("unknown");
    let arch = option_env!("BUILD_RELEASE_ARCH").unwrap_or("unknown");

    if repository == "unknown" || tag == "unknown" || arch == "unknown" {
        "`unknown`".to_owned()
    } else {
        format!(
            "https://github.com/{repository}/releases/download/{tag}/grate-bot-{tag}-{arch}.sha256"
        )
    }
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)?;
    let digest = Sha256::digest(bytes);

    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hex, "{byte:02x}")?;
    }

    Ok(hex)
}

pub(crate) async fn ensure_command_channel(
    ctx: Context<'_>,
    family: ChannelFamily,
) -> Result<bool, Error> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(true);
    };
    let Some(configured_channel_id) = ctx.data().settings.channel(guild_id.get(), family).await
    else {
        return Ok(true);
    };

    let current_channel_id = ctx.channel_id().get();
    if current_channel_id == configured_channel_id {
        return Ok(true);
    }

    let lookup = channel_lookup_for_gate(ctx, guild_id, configured_channel_id).await;
    match channel_gate_action(configured_channel_id, current_channel_id, lookup) {
        ChannelGateAction::ClearAndAllow => {
            ctx.data()
                .settings
                .clear_channel(guild_id.get(), family)
                .await?;
            ctx.send(
                poise::CreateReply::default()
                    .content(format!(
                        "The configured {} channel no longer exists, so I cleared that setting. {} commands are allowed everywhere until someone sets a new channel.",
                        family.label(),
                        family.label()
                    ))
                    .ephemeral(true),
            )
            .await?;
            Ok(true)
        }
        ChannelGateAction::Block => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!(
                        "{} commands only work in <#{}>.",
                        family.label(),
                        configured_channel_id
                    ))
                    .ephemeral(true),
            )
            .await?;
            Ok(false)
        }
        ChannelGateAction::Allow => Ok(true),
    }
}

fn channel_gate_action(
    configured_channel_id: u64,
    current_channel_id: u64,
    lookup: ChannelLookup,
) -> ChannelGateAction {
    if current_channel_id == configured_channel_id {
        return ChannelGateAction::Allow;
    }

    match lookup {
        ChannelLookup::Missing => ChannelGateAction::ClearAndAllow,
        ChannelLookup::ExistsInGuild | ChannelLookup::AmbiguousFailure => ChannelGateAction::Block,
    }
}

async fn channel_lookup_for_gate(
    ctx: Context<'_>,
    guild_id: GuildId,
    channel_id: u64,
) -> ChannelLookup {
    match ctx
        .serenity_context()
        .http
        .get_channel(ChannelId::new(channel_id))
        .await
    {
        Ok(serenity::Channel::Guild(channel)) if channel.guild_id == guild_id => {
            ChannelLookup::ExistsInGuild
        }
        Ok(_) => ChannelLookup::Missing,
        Err(error) if is_unknown_channel_error(&error) => ChannelLookup::Missing,
        Err(_) => ChannelLookup::AmbiguousFailure,
    }
}

fn is_unknown_channel_error(error: &serenity::Error) -> bool {
    matches!(
        error,
        serenity::Error::Http(serenity::http::HttpError::UnsuccessfulRequest(response))
            if response.status_code.as_u16() == 404 && response.error.code == 10003
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelLookup {
    ExistsInGuild,
    Missing,
    AmbiguousFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelGateAction {
    Allow,
    Block,
    ClearAndAllow,
}

fn help_message() -> String {
    "Grate Boss help\n\
\n\
Broadly, I can help with:\n\
    - Grateic Phone: host a Discord drawing-and-prompt game. Create a short or full lobby, let players join, start rounds, collect text and drawing submissions in DMs, track status, cancel a lobby, and reveal the finished game.\n\
- Build verification: show the running bot version, source ref, commit, build input state, and executable SHA-256 so you can compare the live bot against a release.\n\
\n\
Useful commands:\n\
	- `/grate create` starts a Grateic Phone lobby with mode, canvas size, background, and canvas-size-rule options.\n\
	- `/grate grateic help`, `/grate grateic join`, `/grate grateic ready`, `/grate grateic start`, `/grate grateic status`, `/grate grateic cancel`, and `/grate grateic force_cancel` explain or manage a Grateic Phone game.\n\
- `/grate verify` reports what build is currently running.\n\
- `/grate hytale help`, `/grate hytale join`, `/grate hytale status`, `/grate hytale logs`, `/grate hytale start`, `/grate hytale stop`, `/grate hytale restart`, `/grate hytale check-update`, `/grate hytale update`, and Hytale settings commands manage or share the Hytale server.\n\
\n\
Notes: Grateic Phone game state is kept in memory, so active games reset if I restart."
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_message_mentions_major_features_and_commands() {
        let message = help_message();

        assert!(message.contains("Grateic Phone"));
        assert!(message.contains("Build verification"));
        assert!(message.contains("/grate verify"));
        assert!(message.contains("/grate hytale status"));
        assert!(message.contains("/grate hytale check-update"));
        assert!(message.contains("/grate hytale update"));
    }

    #[test]
    fn channel_gate_allows_matching_channel() {
        assert_eq!(
            channel_gate_action(10, 10, ChannelLookup::AmbiguousFailure),
            ChannelGateAction::Allow
        );
    }

    #[test]
    fn channel_gate_blocks_wrong_channel_when_configured_channel_exists() {
        assert_eq!(
            channel_gate_action(10, 11, ChannelLookup::ExistsInGuild),
            ChannelGateAction::Block
        );
    }

    #[test]
    fn channel_gate_clears_only_definitive_missing_channel() {
        assert_eq!(
            channel_gate_action(10, 11, ChannelLookup::Missing),
            ChannelGateAction::ClearAndAllow
        );
        assert_eq!(
            channel_gate_action(10, 11, ChannelLookup::AmbiguousFailure),
            ChannelGateAction::Block
        );
    }
}
