use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use ::serenity::all::{FullEvent, Interaction, UserId};
use dotenvy::dotenv;
use funboy_core::{
    Funboy,
    ollama::{OllamaGenerator, OllamaSettings},
    template_database::TemplateDatabase,
};
use poise::serenity_prelude as serenity;
use reqwest::Client as HttpClient;
use songbird::{SerenityInit, typemap::TypeMapKey};
use sqlx::{PgPool, postgres::PgPoolOptions};
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

pub type OllamaUserSettingsMap = HashMap<UserId, OllamaSettings>;

struct OllamaData {
    pub users: Mutex<HashSet<UserId>>,
    pub generator: Mutex<OllamaGenerator>,
    pub user_settings: Arc<Mutex<OllamaUserSettingsMap>>,
}

impl Default for OllamaData {
    fn default() -> Self {
        Self {
            users: Default::default(),
            generator: Default::default(),
            user_settings: Default::default(),
        }
    }
}

struct Data {
    pub funboy: Arc<Funboy>,
    pub track_list: Arc<Mutex<TrackList>>,
    pub track_player_lock: Arc<Mutex<()>>,
    pub ollama_data: OllamaData,
    pub interpreter_rate_limit: Arc<Mutex<RateLimit>>,
    yt_dlp_cookies_path: Option<String>,
} // User data, which is stored and accessible in all command invocations

impl Data {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self {
            funboy: Arc::new(Funboy::new(TemplateDatabase::new(pool.clone()))),
            track_list: Mutex::new(TrackList::new()).into(),
            track_player_lock: Default::default(),
            ollama_data: OllamaData::default(),
            interpreter_rate_limit: Arc::new(Mutex::new(RateLimit::new(15, 20, 3, 10))),
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

#[tokio::main]
async fn main() {
    dotenv().ok();

    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::MESSAGE_CONTENT
        | serenity::GatewayIntents::GUILD_MEMBERS;

    let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    let debug_mode = std::env::var("DEBUG_MODE")
        .unwrap_or("false".to_string())
        .parse::<bool>()
        .expect("DEBUG_MODE must be of type bool");
    let db_url = if debug_mode == false {
        println!("Launching in release mode.");
        std::env::var("DATABASE_URL").expect("missing DATABASE_URL")
    } else {
        println!("Launching in debug mode.");
        std::env::var("DEBUG_DATABASE_URL").expect("missing DATABASE_URL")
    };

    let pool = Arc::new(
        PgPoolOptions::new()
            .max_connections(15)
            .min_connections(2)
            .acquire_timeout(Duration::from_secs(5))
            .idle_timeout(Duration::from_secs(60 * 10))
            .max_lifetime(Duration::from_secs(60 * 30))
            .connect(&db_url)
            .await
            .expect("failed to connect to database"),
    );

    TemplateDatabase::migrate(&pool)
        .await
        .expect("sqlx migration failed");

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                register(),
                commands::templates::generate(),
                commands::templates::rename_template(),
                commands::templates::add_subs(),
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
            ],
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
                Ok(Data::new(pool))
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .register_songbird()
        .type_map_insert::<HttpKey>(HttpClient::new())
        .await;
    client.unwrap().start().await.unwrap();
}
