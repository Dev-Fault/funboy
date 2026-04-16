use std::{collections::HashMap, sync::Arc, time::Duration};

use fsl_interpreter::FslInterpreter;
use funboy_cli::{CommandError, CommandResult, Context, Permissions, interpret_bot_commands};
use funboy_core::{Funboy, Request, UserId};
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

use crate::{
    commands::interpret_matrix_commands,
    interpreter::{MatrixCtx, create_interpreter},
};

mod commands;
mod interpreter;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MatrixUser {
    room_id: OwnedRoomId,
    user_id: OwnedUserId,
}

impl MatrixUser {
    pub fn new(room_id: OwnedRoomId, user_id: OwnedUserId) -> Self {
        Self { room_id, user_id }
    }
}

impl UserId for MatrixUser {}

pub async fn on_stripped_state_member(
    room_member: StrippedRoomMemberEvent,
    client: Client,
    room: Room,
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
    let user_ctx = funboy.get_user_ctx(matrix_user.clone()).await;
    let mut pending_requests = user_ctx.pending_requests.lock().await;
    let interpreter = create_interpreter(
        funboy.clone(),
        MatrixCtx::new(room.clone(), pending_asks.clone(), matrix_user.clone()),
    )
    .await;

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
                            let msg = funboy.user_generate(user_id, &contents, interpreter).await;
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
        MatrixCtx::new(room.clone(), pending_asks, user_id.clone()),
    )
    .await;

    let result = interpret_bot_commands(
        MatrixUser::new(room.room_id().to_owned(), event.sender),
        &funboy,
        interpreter,
        &Permissions::power_user(),
        Context::Matrix,
        message,
    )
    .await;

    if let Ok(result) = result {
        handle_command_result(result, room).await;
    } else {
        let err = result.err().unwrap();
        match err {
            CommandError::UnhandledCommand(command) => {
                let result =
                    interpret_matrix_commands(&funboy, user_id.clone(), room.clone(), command)
                        .await;
                match result {
                    Ok(result) => {
                        handle_command_result(result, room).await;
                    }
                    Err(err) => {
                        handle_command_err(err, room).await;
                    }
                }
            }
            _ => {
                handle_command_err(err, room).await;
            }
        }
    }
}

async fn handle_command_result(result: CommandResult, room: Room) {
    match result {
        CommandResult::Text(message) => {
            if !message.is_empty() {
                let content = RoomMessageEventContent::text_markdown(&message);
                room.send(content).await.unwrap();
            }
        }
        CommandResult::Mode(_) => {
            let content = RoomMessageEventContent::text_plain(
                "Mode switching not available in matrix client.",
            );
            room.send(content).await.unwrap();
            return;
        }
        CommandResult::None => {}
        CommandResult::Exit => {}
    }
}

async fn handle_command_err(err: CommandError, room: Room) {
    let e = err.to_string();
    let content = RoomMessageEventContent::text_markdown(&e);
    room.send(content).await.unwrap();
    return;
}
