use std::{collections::HashMap, path::Path, sync::Arc};

use funboy_cli::{FunboyEnv, get_funboy};
use funboy_core::database::Platform;
use funboy_matrix::{
    MatrixEnv, MatrixUser, grant_host_permissions, on_room_message, on_stripped_state_member,
};
use matrix_sdk::{
    Client, Room,
    authentication::matrix::MatrixSession,
    config::SyncSettings,
    ruma::events::{
        key::verification::request::ToDeviceKeyVerificationRequestEvent,
        room::{member::StrippedRoomMemberEvent, message::OriginalSyncRoomMessageEvent},
    },
};
use tokio::sync::{Mutex, oneshot};

#[tokio::main]
async fn main() {
    let funboy_env = FunboyEnv::new();
    let env = MatrixEnv::new(&funboy_env);

    let client = Client::builder()
        .homeserver_url(&env.homeserver)
        .sqlite_store("./bot_state", None)
        .build()
        .await
        .expect("couldn't connect to home server");

    let session_file = Path::new("./bot_state/session.json");

    if session_file.exists() {
        let session_json = std::fs::read_to_string(session_file).unwrap();
        let session: MatrixSession = serde_json::from_str(&session_json).unwrap();
        client.restore_session(session).await.unwrap();
        println!(
            "Restored previous session, device ID: {}",
            client.session().expect("").meta().device_id
        )
    } else {
        client
            .matrix_auth()
            .login_username(&env.username, &env.password)
            .await
            .expect("couldn't login");

        let session = client.matrix_auth().session().unwrap();
        let session_json = serde_json::to_string(&session).unwrap();
        std::fs::write(session_file, session_json).unwrap();
        println!("New login, device ID: {}", client.device_id().unwrap());

        client
            .encryption()
            .recovery()
            .recover(&env.recovery_key)
            .await
            .expect("invalid recovery key");
    }

    let funboy = Arc::new(get_funboy(&funboy_env, Platform::Matrix).await);

    let funboy_clone = funboy.clone();
    let env_clone = env.clone();
    client.add_event_handler(
        move |room_member: StrippedRoomMemberEvent, client: Client, room: Room| {
            on_stripped_state_member(room_member, client, room, env_clone, funboy_clone)
        },
    );

    let sync_token = client
        .sync_once(SyncSettings::default())
        .await
        .unwrap()
        .next_batch;

    client.add_event_handler(
        |_ev: ToDeviceKeyVerificationRequestEvent, _client: Client| async move {
            // ignore verification requests, prefer bootstrap
        },
    );

    let pending_asks: Arc<Mutex<HashMap<MatrixUser, oneshot::Sender<String>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let client_clone = client.clone();
    let funboy_clone = funboy.clone();
    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        on_room_message(client_clone, event, room, funboy_clone, pending_asks)
    });

    let settings = SyncSettings::default().token(sync_token);

    for room in client.joined_rooms() {
        grant_host_permissions(&env, funboy.clone(), room.room_id().to_owned()).await;
    }

    client.sync(settings).await.expect("failed to sync client");
}
