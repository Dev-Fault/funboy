use std::{env, sync::Arc};

use funboy_cli::{FunboyEnv, get_funboy};
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
    pub fn new(funboy_env: &FunboyEnv) -> MatrixEnv {
        dotenvy::dotenv().expect("parent directory should have .env file");
        let homeserver = env::var("HOME_SERVER").expect(".env file should contain HOME_SERVER");

        let (username, password) = if funboy_env.debug_mode {
            (
                env::var("DEBUG_USERNAME").expect(".env file should contain DEBUG_USERNAME"),
                env::var("DEBUG_PASSWORD").expect(".env file should contain DEBUG_PASSWORD"),
            )
        } else {
            (
                env::var("USERNAME").expect(".env file should contain USERNAME"),
                env::var("PASSWORD").expect(".env file should contain PASSWORD"),
            )
        };

        Self {
            homeserver,
            username,
            password,
        }
    }
}

#[tokio::main]
async fn main() {
    let funboy_env = FunboyEnv::new();
    let env = MatrixEnv::new(&funboy_env);

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

    let funboy = Arc::new(get_funboy(&funboy_env).await);

    let pending_ask: Arc<Mutex<Option<(OwnedUserId, oneshot::Sender<String>)>>> =
        Arc::new(Mutex::new(None));

    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        on_room_message(event, room, funboy, pending_ask)
    });

    let settings = SyncSettings::default().token(sync_token);

    client.sync(settings).await.expect("failed to sync client");
}
