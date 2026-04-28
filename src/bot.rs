use crate::features::grateic::{self as grateic_feature, grateic};
use anyhow::Context as AnyhowContext;
use poise::serenity_prelude as serenity;
use serenity::{FullEvent, GatewayIntents};

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

#[poise::command(slash_command, subcommands("grateic"))]
async fn grate(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}
