use std::{collections::HashMap, sync::Arc};

use ::serenity::all::{FullEvent, Interaction};
use dotenvy::dotenv;
use funboy_core::{Funboy, template_database::TemplateDatabase};
use poise::serenity_prelude as serenity;
use reqwest::Client as HttpClient;
use songbird::{SerenityInit, typemap::TypeMapKey};
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::{
    commands::sound::TrackList,
    components::{CustomComponent, TrackComponent},
};

mod commands;
mod components;
mod io_format;

struct Data {
    pub funboy: Funboy,
    pub track_list: Arc<Mutex<TrackList>>,
    pub track_player_lock: Arc<Mutex<()>>,

    yt_dlp_cookies_path: Option<String>,
} // User data, which is stored and accessible in all command invocations
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

impl Data {
    pub fn get_funboy(&self) -> &Funboy {
        &self.funboy
    }

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

    let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    let intents = serenity::GatewayIntents::non_privileged();

    let db_url = std::env::var("DATABASE_URL").expect("missing DATABASE_URL");

    let pool = Arc::new(
        PgPool::connect(&db_url)
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
                commands::templates::delete_template(),
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
                commands::utility::fsl_help(),
                commands::utility::move_bot_pins(),
                commands::utility::age(),
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
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                //poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    funboy: Funboy::new(TemplateDatabase::new(pool.clone())),
                    track_list: Mutex::new(TrackList::new()).into(),
                    track_player_lock: Arc::new(Mutex::new(())),
                    yt_dlp_cookies_path: None,
                })
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
