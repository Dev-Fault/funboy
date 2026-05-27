use std::{str::FromStr, sync::Arc};

use dotenvy::dotenv;
use funboy_cli::FunboyEnv;
use funboy_core::{Funboy, interpreter::InterpreterLimits, user::FunboyUserId};
use serenity::{
    all::{Message, UserId},
    prelude::TypeMapKey,
};
use tokio::sync::Mutex;

use crate::{
    commands::{
        prefix_commands::{handle_prefix_command, handle_request},
        sound::TrackList,
    },
    interpreter::interpreter_from_serenity,
};

pub mod commands;
pub mod components;
pub mod context_extension;
pub mod interpreter;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiscordUserId(pub UserId);
impl FunboyUserId for DiscordUserId {}

impl ToString for DiscordUserId {
    fn to_string(&self) -> String {
        self.0.to_string()
    }
}

pub struct Data {
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

pub struct HttpKey;

impl TypeMapKey for HttpKey {
    type Value = reqwest::Client;
}

#[poise::command(prefix_command)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

pub struct DiscordEnv {
    pub funboy_env: FunboyEnv,
    pub token: String,
    pub host_ids: Vec<String>,
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

pub fn get_discord_commands()
-> Vec<poise::Command<Data, Box<dyn std::error::Error + std::marker::Send + Sync + 'static>>> {
    vec![
        register(),
        commands::templates::generate(),
        commands::templates::generate_ollama(),
        commands::templates::generate_file(),
        commands::templates::rename_template(),
        commands::templates::add(),
        commands::templates::add_file(),
        commands::templates::copy(),
        commands::templates::replace(),
        commands::templates::delete(),
        commands::templates::delete_templates(),
        commands::templates::list_templates(),
        commands::templates::list_subs(),
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

pub async fn handle_message(
    ctx: &serenity::prelude::Context,
    data: &Data,
    message: &Message,
) -> Result<Option<String>, String> {
    let user_id = DiscordUserId(message.author.id);
    let user_ctx = data.funboy.users.get_or_insert(user_id.clone()).await;
    if let Ok(user_ctx) = user_ctx {
        let requests = user_ctx.pending_requests.clone();
        let mut requests = requests.lock().await;
        if let Some(request) = requests.pop() {
            let result = handle_request(request, ctx, data, message).await;
            return result.map(|r| r.into()).map_err(|e| e.to_string());
        }
    }

    let mentions_bot = message.mentions_me(ctx).await.is_ok_and(|is_true| is_true);
    let mentions_bot = mentions_bot && message.content.starts_with("<@");
    if mentions_bot {
        let user_id = DiscordUserId(message.author.id);
        let msg = message.content.to_owned();
        let interpreter = interpreter_from_serenity(ctx, data, message).await;
        let result = data.funboy.user_chat(user_id, msg, &interpreter).await;
        result.map(|r| Some(r)).map_err(|e| e.to_string())
    } else if message.content.starts_with("!") {
        let result = handle_prefix_command(&ctx, data, &message).await;
        result.map(|r| r.into()).map_err(|e| e.to_string())
    } else {
        Ok(None)
    }
}
