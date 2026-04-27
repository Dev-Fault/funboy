use std::{str::FromStr, sync::Arc};

use ::serenity::all::{FullEvent, Interaction, UserId};
use dotenvy::dotenv;
use fsl_interpreter::FslInterpreter;
use funboy_cli::{FunboyEnv, get_funboy};
use funboy_core::{Funboy, database::Platform, interpreter::InterpreterLimits, user::FunboyUserId};
use poise::serenity_prelude as serenity;
use reqwest::Client as HttpClient;
use songbird::{SerenityInit, typemap::TypeMapKey};
use tokio::sync::Mutex;

use crate::{
    commands::sound::TrackList,
    components::{CustomComponent, TrackComponent},
};

mod commands;
mod components;
mod context_extension;
mod interpreter;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiscordUserId(UserId);
impl FunboyUserId for DiscordUserId {}

impl ToString for DiscordUserId {
    fn to_string(&self) -> String {
        self.0.to_string()
    }
}

struct Data {
    pub funboy: Arc<Funboy<DiscordUserId>>,
    pub track_list: Arc<Mutex<TrackList>>,
    pub track_player_lock: Arc<Mutex<()>>,
    pub interpreter_limits: InterpreterLimits<DiscordUserId>,
    yt_dlp_cookies_path: Option<String>,
} // User data, which is stored and accessible in all command invocations

impl Data {
    pub fn new(funboy: Arc<Funboy<DiscordUserId>>) -> Self {
        Self {
            funboy,
            track_list: Mutex::new(TrackList::new()).into(),
            track_player_lock: Default::default(),
            interpreter_limits: InterpreterLimits::default(),
            yt_dlp_cookies_path: None,
        }
    }

    #[allow(dead_code)]
    pub fn get_yt_dlp_cookies_path(&self) -> Option<&str> {
        match &self.yt_dlp_cookies_path {
            Some(path) => Some(path),
            None => None,
        }
    }
}

struct HttpKey;

impl TypeMapKey for HttpKey {
    type Value = HttpClient;
}

#[poise::command(prefix_command)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

pub struct DiscordEnv {
    funboy_env: FunboyEnv,
    token: String,
    host_ids: Vec<String>,
}

impl DiscordEnv {
    pub fn new(funboy_env: FunboyEnv) -> DiscordEnv {
        dotenv().ok();

        let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");

        let host_ids: Vec<String> = std::env::var("HOSTS")
            .unwrap_or_default()
            .split(",")
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        DiscordEnv {
            funboy_env,
            token,
            host_ids,
        }
    }
}

fn get_discord_commands()
-> Vec<poise::Command<Data, Box<dyn std::error::Error + std::marker::Send + Sync + 'static>>> {
    vec![
        register(),
        commands::templates::generate(),
        commands::templates::generate_file(),
        commands::templates::rename_template(),
        commands::templates::add_subs(),
        commands::templates::upload_sub(),
        commands::templates::copy_subs(),
        commands::templates::replace_sub(),
        commands::templates::delete_subs(),
        commands::templates::delete_templates(),
        commands::templates::list_subs(),
        commands::templates::list_templates(),
        commands::random::random_number(),
        commands::random::random_entry(),
        commands::sound::join_voice(),
        commands::sound::leave_voice(),
        commands::sound::play_track(),
        commands::sound::stop_tracks(),
        commands::sound::list_tracks(),
        commands::utility::help(),
        commands::utility::help_command(),
        commands::utility::move_bot_pins(),
        commands::utility::age(),
        commands::utility::set_role(),
        commands::utility::grant(),
        commands::utility::revoke(),
        commands::utility::cancel(),
        commands::ollama::ollama(),
        commands::ollama::generate_ollama(),
    ]
}

pub async fn grant_host_permissions(env: &DiscordEnv, funboy: Arc<Funboy<DiscordUserId>>) {
    for host_id in &env.host_ids {
        let user_id = match UserId::from_str(host_id.as_str()) {
            Ok(user_id) => user_id,
            Err(e) => {
                eprintln!("{e}");
                continue;
            }
        };
        let user_id = DiscordUserId(user_id);
        let users = funboy.users.clone();
        if let Err(e) = users.grant_all_permissions(user_id).await {
            eprintln!("{e}");
        }
    }
}

#[tokio::main]
async fn main() {
    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_MEMBERS;

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
                            if new_message
                                .mentions_me(ctx)
                                .await
                                .is_ok_and(|is_true| is_true)
                            {
                                let user_id = DiscordUserId(new_message.author.id);
                                let msg = new_message.content.to_owned();
                                let interpreter = Arc::new(Mutex::new(FslInterpreter::new()));
                                let result = data.funboy.user_chat(user_id, msg, interpreter).await;

                                match result {
                                    Ok(response) => {
                                        let _ = new_message.reply(&ctx.http, response).await;
                                    }
                                    Err(e) => {
                                        let _ = new_message.reply(&ctx.http, e.to_string()).await;
                                    }
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
        .type_map_insert::<HttpKey>(HttpClient::new())
        .await;
    client.unwrap().start().await.unwrap();
}
