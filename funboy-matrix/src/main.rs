use std::{env, sync::Arc, time::Duration};

use fsl_interpreter::FslInterpreter;
use funboy_cli::{FunboyCtx, get_env, get_funboy};
use funboy_core::ollama::OllamaSettings;
use funboy_matrix::{on_room_message, on_stripped_state_member};
use matrix_sdk::{
    Client, Room,
    config::SyncSettings,
    ruma::{OwnedUserId, events::room::message::OriginalSyncRoomMessageEvent},
};
use tokio::sync::{Mutex, oneshot};

struct MatrixEnv {
    homeserver: String,
    username: String,
    password: String,
}

impl MatrixEnv {
    pub fn new() -> MatrixEnv {
        dotenvy::dotenv().expect("parent directory should have .env file");
        let homeserver = env::var("HOME_SERVER").expect(".env file should contain HOME_SERVER");
        let username = env::var("USERNAME").expect(".env file should contain USERNAME");
        let password = env::var("PASSWORD").expect(".env file should contain PASSWORD");

        Self {
            homeserver,
            username,
            password,
        }
    }
}

#[tokio::main]
async fn main() {
    let env = MatrixEnv::new();

    let client = Client::builder()
        .homeserver_url(&env.homeserver)
        .build()
        .await
        .expect("couldn't connect to home server");

    client
        .matrix_auth()
        .login_username(&env.username, &env.password)
        .await
        .expect("couldn't login");

    client.add_event_handler(on_stripped_state_member);

    let sync_token = client
        .sync_once(SyncSettings::default())
        .await
        .unwrap()
        .next_batch;

    let funboy = Arc::new(get_funboy(&get_env()).await);

    let funboy_ctx = FunboyCtx {
        funboy: funboy.clone(),
        ollama_settings: Arc::new(Mutex::new(OllamaSettings::default())),
    };

    let pending_ask: Arc<Mutex<Option<(OwnedUserId, oneshot::Sender<String>)>>> =
        Arc::new(Mutex::new(None));

    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        on_room_message(event, room, funboy_ctx, pending_ask)
    });

    let settings = SyncSettings::default().token(sync_token);

    client.sync(settings).await.expect("failed to sync client");
}
