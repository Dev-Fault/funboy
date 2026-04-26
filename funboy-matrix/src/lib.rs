use std::{collections::HashMap, env, str::FromStr, sync::Arc, time::Duration};

use fsl_interpreter::FslInterpreter;
use funboy_cli::FunboyEnv;
use funboy_core::{
    Funboy, Request,
    commands::{CommandError, CommandResult},
    database::Platform,
    permissions::Permissions,
    user::FunboyUserId,
};
use matrix_sdk::{
    Client, Room, RoomState,
    ruma::{
        OwnedRoomId, OwnedUserId,
        events::room::{
            member::StrippedRoomMemberEvent,
            message::{MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent},
        },
    },
};
use tokio::{
    sync::{Mutex, oneshot},
    time::sleep,
};
use url::Url;

use crate::{
    commands::{attach_url, interpret_matrix_commands},
    interpreter::{MatrixCtx, create_interpreter},
};

mod commands;
mod interpreter;

#[derive(Clone)]
pub struct MatrixEnv {
    pub homeserver: String,
    pub username: String,
    pub password: String,
    pub recovery_key: String,
    pub host_ids: Vec<String>,
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

        let recovery_key = env::var("RECOVERY_KEY").expect(".env file should contain recovery key");

        let host_ids: Vec<String> = env::var("HOSTS")
            .unwrap_or_default()
            .split(",")
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        Self {
            homeserver,
            username,
            password,
            recovery_key,
            host_ids,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MatrixUser {
    room_id: OwnedRoomId,
    user_id: OwnedUserId,
}

impl MatrixUser {
    pub fn new(room_id: OwnedRoomId, user_id: OwnedUserId) -> Self {
        Self { room_id, user_id }
    }
}

impl FunboyUserId for MatrixUser {}

impl ToString for MatrixUser {
    fn to_string(&self) -> String {
        self.user_id.to_string()
    }
}

pub async fn grant_host_permissions(
    env: &MatrixEnv,
    funboy: Arc<Funboy<MatrixUser>>,
    room_id: OwnedRoomId,
) {
    for host_id in &env.host_ids {
        let user_id = match OwnedUserId::from_str(&host_id) {
            Ok(user_id) => user_id,
            Err(e) => {
                eprintln!("{}", e.to_string());
                continue;
            }
        };
        let user = MatrixUser::new(room_id.clone(), user_id);
        let users = funboy.users.clone();
        if let Err(e) = users.grant_all_permissions(user).await {
            eprintln!("{e}");
        };
    }
}

pub async fn on_stripped_state_member(
    room_member: StrippedRoomMemberEvent,
    client: Client,
    room: Room,
    env: MatrixEnv,
    funboy: Arc<Funboy<MatrixUser>>,
) {
    if room_member.state_key != client.user_id().unwrap() {
        return;
    }

    tokio::spawn(async move {
        println!("Autojoining room {}", room.room_id());
        let mut delay = 2;

        while let Err(err) = room.join().await {
            eprintln!(
                "Failed to join room {} ({err:?}), retrying in {delay}s",
                room.room_id()
            );

            sleep(Duration::from_secs(delay)).await;
            delay *= 2;

            if delay > 3600 {
                eprintln!("Can't join room {} ({err:?})", room.room_id());
                break;
            }
        }

        grant_host_permissions(&env, funboy, room.room_id().to_owned()).await;

        println!("Successfully joined room {}", room.room_id());
    });
}

pub async fn on_room_message(
    client: Client,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    funboy: Arc<Funboy<MatrixUser>>,
    pending_asks: Arc<Mutex<HashMap<MatrixUser, oneshot::Sender<String>>>>,
) {
    // First, we need to unpack the message: We only want messages from rooms we are
    // still in and that are regular text messages - ignoring everything else.
    if room.state() != RoomState::Joined {
        return;
    }

    let matrix_user = MatrixUser::new(room.room_id().to_owned(), event.sender.clone());
    let user_ctx = match funboy.users.get_or_insert(matrix_user.clone()).await {
        Ok(user_ctx) => user_ctx,
        Err(e) => {
            room.send(RoomMessageEventContent::text_plain(e.to_string()))
                .await
                .unwrap();
            return;
        }
    };

    let mut pending_requests = user_ctx.pending_requests.lock().await;
    let interpreter = create_interpreter(
        funboy.clone(),
        MatrixCtx::new(
            funboy.clone(),
            room.clone(),
            pending_asks.clone(),
            matrix_user.clone(),
        ),
    )
    .await;

    let bot_user_id = client.user_id().unwrap().localpart().to_owned();

    if let Some(request) = pending_requests.pop() {
        tokio::spawn(async move {
            handle_request(request, client, event, room, funboy, interpreter).await;
        });
    } else {
        let MessageType::Text(text_content) = event.content.msgtype.clone() else {
            return;
        };

        {
            let mut pending = pending_asks.lock().await;
            if let Some(tx) = pending.remove(&matrix_user) {
                let _ = tx.send(text_content.body);
                return;
            }
        }

        tokio::spawn(async move {
            if text_content.body.starts_with("!") {
                handle_bot_command(
                    event,
                    room,
                    funboy,
                    pending_asks,
                    text_content.body.trim_start_matches("!"),
                )
                .await;
            } else if text_content.body.starts_with(&bot_user_id) {
                let input = text_content.body.trim_start_matches(&bot_user_id);
                let result = funboy.user_chat(matrix_user, input, interpreter).await;
                match result {
                    Ok(response) => {
                        room.send(RoomMessageEventContent::text_markdown(response))
                            .await
                            .unwrap();
                    }
                    Err(e) => {
                        room.send(RoomMessageEventContent::text_plain(e.to_string()))
                            .await
                            .unwrap();
                    }
                }
            }
        });
    }
}

pub async fn handle_request(
    request: Request,
    client: Client,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    funboy: Arc<Funboy<MatrixUser>>,
    interpreter: Arc<Mutex<FslInterpreter>>,
) {
    let user_id = MatrixUser::new(room.room_id().to_owned(), event.sender.clone());
    match request {
        Request::GenerateFile => {
            if let MessageType::File(file) = &event.content.msgtype {
                match client.media().get_file(file, false).await {
                    Ok(file_data) => {
                        let file_data = file_data.unwrap_or_default();
                        let contents = String::from_utf8(file_data);
                        if let Ok(contents) = contents {
                            let msg = funboy.user_generate(user_id, contents, interpreter).await;
                            let msg = match msg {
                                Ok(msg) => RoomMessageEventContent::text_markdown(msg),
                                Err(e) => RoomMessageEventContent::text_markdown(e.to_string()),
                            };
                            room.send(msg).await.unwrap();
                        } else {
                            let content = RoomMessageEventContent::text_plain(
                                "Only text files are allowed (must be valid UTF-8).",
                            );
                            room.send(content).await.unwrap();
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", e.to_string());
                        let content =
                            RoomMessageEventContent::text_plain("Failed to download file.");
                        room.send(content).await.unwrap();
                    }
                }
            } else {
                let content = RoomMessageEventContent::text_plain(
                    "Only text files are allowed (must be valid UTF-8).",
                );
                room.send(content).await.unwrap();
            }
        }
        Request::UploadSub(template) => {
            if let MessageType::File(file) = &event.content.msgtype {
                match client.media().get_file(file, false).await {
                    Ok(file_data) => {
                        let file_data = file_data.unwrap_or_default();
                        let contents = String::from_utf8(file_data);
                        if let Ok(contents) = contents {
                            let result = funboy.add_substitutes(&template, &[&contents]).await;
                            let msg = match result {
                                Ok(_) => RoomMessageEventContent::text_markdown(format!(
                                    "added {} to {}",
                                    file.filename(),
                                    template
                                )),
                                Err(e) => RoomMessageEventContent::text_markdown(e.to_string()),
                            };
                            room.send(msg).await.unwrap();
                        } else {
                            let content = RoomMessageEventContent::text_plain(
                                "Only text files are allowed (must be valid UTF-8).",
                            );
                            room.send(content).await.unwrap();
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", e.to_string());
                        let content =
                            RoomMessageEventContent::text_plain("Failed to download file.");
                        room.send(content).await.unwrap();
                    }
                }
            } else {
                let content = RoomMessageEventContent::text_plain(
                    "Only text files are allowed (must be valid UTF-8).",
                );
                room.send(content).await.unwrap();
            }
        }
        Request::DeleteTemplate(template) => {
            if let MessageType::Text(response) = event.content.msgtype
                && response.body.to_lowercase() == "yes"
            {
                let result = funboy
                    .delete_command(
                        user_id.clone(),
                        Platform::Matrix,
                        template,
                        String::new(),
                        false,
                        false,
                    )
                    .await;
                match result {
                    Ok(result) => handle_command_result(result, room).await,
                    Err(e) => handle_command_err(e, room).await,
                };
            }
        }
    }
}

pub async fn handle_bot_command(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    funboy: Arc<Funboy<MatrixUser>>,
    pending_asks: Arc<Mutex<HashMap<MatrixUser, oneshot::Sender<String>>>>,
    message: &str,
) {
    let user_id = MatrixUser::new(room.room_id().to_owned(), event.sender.clone());
    let interpreter = create_interpreter(
        funboy.clone(),
        MatrixCtx::new(funboy.clone(), room.clone(), pending_asks, user_id.clone()),
    )
    .await;

    let result = interpret_matrix_commands(
        &funboy,
        interpreter,
        MatrixUser::new(room.room_id().to_owned(), event.sender),
        room.clone(),
        &message,
    )
    .await;

    match result {
        Ok(result) => handle_command_result(result, room).await,
        Err(e) => {
            handle_command_err(e, room).await;
        }
    }
}

pub async fn send_msg_with_mixed_content(input: &str, user_permissions: &Permissions, room: Room) {
    let mut buf = String::with_capacity(input.len());
    for item in input.split_inclusive(' ') {
        if let Ok(url) = Url::parse(item)
            && !url.cannot_be_a_base()
        {
            if !buf.trim().is_empty() {
                room.send(RoomMessageEventContent::text_markdown(&buf))
                    .await
                    .unwrap();
                buf.clear();
            }
            if let Err(_) = attach_url(url.as_str(), &user_permissions, room.clone()).await {
                buf.push_str(item);
            };
        } else {
            buf.push_str(item);
        }
    }

    if !buf.trim().is_empty() {
        room.send(RoomMessageEventContent::text_markdown(&buf))
            .await
            .unwrap();
    }
}

async fn handle_command_result(result: CommandResult, room: Room) {
    match result {
        CommandResult::Text(message) => {
            if !message.trim().is_empty() {
                let content = RoomMessageEventContent::text_markdown(&message);
                room.send(content).await.unwrap();
            }
        }
        CommandResult::None => {}
    }
}

async fn handle_command_err(err: CommandError, room: Room) {
    let e = err.to_string();
    let content = RoomMessageEventContent::text_plain(&e);
    room.send(content).await.unwrap();
    return;
}
