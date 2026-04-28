use std::sync::Arc;

use ::serenity::all::{FullEvent, Interaction};
use funboy_cli::{FunboyEnv, get_funboy};
use funboy_core::database::Platform;
use funboy_discord::{
    Data, DiscordEnv, HttpKey,
    commands::{self},
    components::{CustomComponent, TrackComponent},
    get_discord_commands, grant_host_permissions, handle_message,
};
use poise::serenity_prelude as serenity;
use songbird::SerenityInit;

#[tokio::main]
async fn main() {
    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_MEMBERS
        | serenity::GatewayIntents::GUILD_VOICE_STATES;

    let funboy_env = FunboyEnv::new();
    let env = DiscordEnv::new(funboy_env);

    let funboy = Arc::new(get_funboy(&env.funboy_env, Platform::Discord).await);

    grant_host_permissions(&env, funboy.clone()).await;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: get_discord_commands(),
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("!".into()),
                mention_as_prefix: false,
                ..Default::default()
            },
            event_handler: |ctx, event, _framework_ctx, data| {
                Box::pin(async move {
                    match event {
                        FullEvent::Message { new_message } => {
                            let result = handle_message(ctx, data, new_message).await;
                            match result {
                                Ok(output) => {
                                    if let Some(output) = output {
                                        if let Err(e) = new_message.reply(&ctx.http, output).await {
                                            eprintln!("{e}");
                                        };
                                    }
                                }
                                Err(e) => {
                                    if let Err(e) = new_message.reply(&ctx.http, e).await {
                                        eprintln!("{e}");
                                    };
                                }
                            }
                        }
                        FullEvent::InteractionCreate {
                            interaction: Interaction::Component(component_interaction),
                        } => match CustomComponent::from(component_interaction) {
                            CustomComponent::TrackComponent => {
                                commands::sound::on_track_button_click(
                                    ctx,
                                    TrackComponent::new(component_interaction.clone()),
                                    data,
                                )
                                .await?;
                            }
                            CustomComponent::None => {}
                        },
                        _ => {}
                    }
                    Ok(())
                })
            },
            ..Default::default()
        })
        .setup(|_ctx, _ready, _framework| {
            Box::pin(async move {
                // poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data::new(funboy))
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(env.token, intents)
        .framework(framework)
        .register_songbird()
        .type_map_insert::<HttpKey>(reqwest::Client::new())
        .await;
    client.unwrap().start().await.unwrap();
}
