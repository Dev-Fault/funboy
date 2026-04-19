use std::{collections::HashMap, sync::Arc};

use funboy_cli::{FunboyEnv, get_funboy};
use funboy_core::database::Platform;
use funboy_matrix::{
    MatrixEnv, MatrixUser, grant_host_permissions, on_room_message, on_stripped_state_member,
};
use matrix_sdk::{
    Client, Room,
    config::SyncSettings,
    ruma::events::room::{member::StrippedRoomMemberEvent, message::OriginalSyncRoomMessageEvent},
};
use tokio::sync::{Mutex, oneshot};

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
