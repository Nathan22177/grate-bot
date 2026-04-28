use crate::features::{
    grateic::{self as grateic_feature, grateic},
    hytale::hytale,
};
use anyhow::Context as AnyhowContext;
use poise::serenity_prelude as serenity;
use serenity::{FullEvent, GatewayIntents};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, path::Path};

type Error = anyhow::Error;
pub(crate) type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(Debug, Clone, Default)]
pub struct Data {
    pub(crate) grateic: grateic_feature::State,
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

                Ok(Data::default())
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await?;

    client.start().await?;
    Ok(())
}

#[poise::command(slash_command, subcommands("help", "grateic", "verify", "hytale"))]
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
        "Build verification\nVersion: `{}`\nSource ref: `{}`\nCommit: `{}`\nBuild inputs: `{}`\nExecutable SHA-256: `{}`",
        env!("CARGO_PKG_VERSION"),
        option_env!("BUILD_SOURCE_REF").unwrap_or("unknown"),
        option_env!("BUILD_COMMIT").unwrap_or("unknown"),
        option_env!("BUILD_INPUT_STATE").unwrap_or("unknown"),
        checksum
    ))
    .await?;

    Ok(())
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

fn help_message() -> &'static str {
    "Grate Boss help\n\
\n\
Broadly, I can help with:\n\
- Grateic: host a Discord drawing-and-prompt game. Create a lobby, let players join, start rounds, collect text and drawing submissions in DMs, track status, cancel a lobby, and reveal the finished chains.\n\
- Hytale management: for trusted server helpers, check the Hytale server status, read recent logs, and start, stop, or restart the service.\n\
- Build verification: show the running bot version, source ref, commit, build input state, and executable SHA-256 so you can compare the live bot against a release.\n\
\n\
Useful commands:\n\
- `/grate grateic create` starts a Grateic lobby with canvas size and background options.\n\
- `/grate grateic join`, `/grate grateic ready`, `/grate grateic start`, `/grate grateic status`, and `/grate grateic cancel` manage a Grateic game.\n\
- `/grate hytale status`, `/grate hytale logs`, `/grate hytale start`, `/grate hytale stop`, and `/grate hytale restart` manage the Hytale server if you have permission.\n\
- `/grate verify` reports what build is currently running.\n\
\n\
Notes: Grateic game state is kept in memory, so active games reset if I restart. Hytale controls only work for users with the configured manager role."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_message_mentions_major_features_and_commands() {
        let message = help_message();

        assert!(message.contains("Grateic"));
        assert!(message.contains("Hytale management"));
        assert!(message.contains("Build verification"));
        assert!(message.contains("/grate verify"));
        assert!(message.contains("/grate hytale status"));
    }
}
