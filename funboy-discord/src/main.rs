use std::sync::Arc;

use ::serenity::all::{FullEvent, Interaction, UserId};
use dotenvy::dotenv;
use funboy_cli::{FunboyEnv, get_funboy};
use funboy_core::Funboy;
use poise::serenity_prelude as serenity;
use reqwest::Client as HttpClient;
use songbird::{SerenityInit, typemap::TypeMapKey};
use tokio::sync::Mutex;

use crate::{
    commands::sound::TrackList,
    components::{CustomComponent, TrackComponent},
    rate_limiter::RateLimit,
};

mod commands;
mod components;
mod interpreter;
mod io_format;
mod rate_limiter;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct DiscordUserId(UserId);
impl funboy_core::UserId for DiscordUserId {}

struct Data {
    pub funboy: Arc<Funboy<DiscordUserId>>,
    pub track_list: Arc<Mutex<TrackList>>,
    pub track_player_lock: Arc<Mutex<()>>,
    pub interpreter_rate_limit: Arc<Mutex<RateLimit>>,
    yt_dlp_cookies_path: Option<String>,
} // User data, which is stored and accessible in all command invocations

impl Data {
    pub fn new(funboy: Arc<Funboy<DiscordUserId>>) -> Self {
        Self {
            funboy,
            track_list: Mutex::new(TrackList::new()).into(),
            track_player_lock: Default::default(),
            interpreter_rate_limit: Arc::new(Mutex::new(
                RateLimit::new(25, 15).with_timeout(60, 10),
            )),
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
}

impl DiscordEnv {
    pub fn new(funboy_env: FunboyEnv) -> DiscordEnv {
        dotenv().ok();

        let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");

        DiscordEnv { funboy_env, token }
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
        commands::ollama::list_ollama_models(),
        commands::ollama::set_ollama_model(),
        commands::ollama::list_ollama_settings(),
        commands::ollama::set_ollama_word_limit(),
        commands::ollama::set_ollama_parameters(),
        commands::ollama::set_ollama_system_prompt(),
        commands::ollama::reset_ollama_system_prompt(),
        commands::ollama::set_ollama_template(),
        commands::ollama::reset_ollama_template(),
        commands::ollama::reset_ollama_parameters(),
        commands::ollama::generate_ollama(),
    ]
}

#[tokio::main]
async fn main() {
    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_MEMBERS;

    let funboy_env = FunboyEnv::new();
    let env = DiscordEnv::new(funboy_env);
    let funboy = Arc::new(get_funboy(&env.funboy_env).await);

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: get_discord_commands(),
            event_handler: |ctx, event, _framework_ctx, data| {
                Box::pin(async move {
                    match event {
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
