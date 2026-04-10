use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use funboy_cli::{FunboyCtx, Permissions, interpret_bot_commands};
use funboy_core::{Funboy, UserId};
use matrix_sdk::{
    Client, Room, RoomState,
    ruma::{
        OwnedUserId,
        events::room::{
            member::StrippedRoomMemberEvent,
            message::{MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent},
        },
    },
};
use pulldown_cmark::{Options, Parser, html};
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
pub struct MatrixUserId(OwnedUserId);
impl UserId for MatrixUserId {}

fn markdown_to_html(input: &str) -> String {
    input
        .trim()
        .lines()
        .map(|line| {
            let parser = Parser::new_ext(line, Options::all());
            let mut html_output = String::new();
            html::push_html(&mut html_output, parser);
            html_output
                .replace("<p>", "")
                .replace("</p>", "")
                .trim()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("<br/>")
}

pub async fn on_room_message(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    funboy: Arc<Funboy<MatrixUserId>>,
    user_ctx: Arc<Mutex<HashMap<OwnedUserId, FunboyCtx<MatrixUserId>>>>,
    pending_ask: Arc<Mutex<Option<(OwnedUserId, oneshot::Sender<String>)>>>,
) {
    // First, we need to unpack the message: We only want messages from rooms we are
    // still in and that are regular text messages - ignoring everything else.
    if room.state() != RoomState::Joined {
        return;
    }

    let MessageType::Text(text_content) = event.content.msgtype else {
        return;
    };

    {
        let mut pending = pending_ask.lock().await;
        if let Some((expected_sender, tx)) = pending.take() {
            if expected_sender == event.sender {
                let _ = tx.send(text_content.body);
                return;
            } else {
                *pending = Some((expected_sender, tx))
            }
        }
    }

    tokio::spawn(async move {
        if text_content.body.starts_with("!") {
            let mut user_ctx = user_ctx.lock().await;
            let funboy_ctx = user_ctx
                .entry(event.sender.clone())
                .or_insert(FunboyCtx::new(funboy))
                .clone();
            drop(user_ctx);

            let user_text = text_content.body.trim_start_matches("!");
            let result = interpret_matrix_commands(&funboy_ctx, room.clone(), &user_text).await;
            if let Err(err) = result {
                match err {
                    funboy_cli::CommandError::ExecutionFailed(e) => {
                        let html = markdown_to_html(&e);
                        let content = RoomMessageEventContent::text_html(&e, html);
                        room.send(content).await.unwrap();
                        return;
                    }
                    funboy_cli::CommandError::LackingPermission(_) => {
                        let e = err.to_string();
                        let html = markdown_to_html(&e);
                        let content = RoomMessageEventContent::text_html(&e, html);
                        room.send(content).await.unwrap();
                        return;
                    }
                    funboy_cli::CommandError::UnknownCommand(_) => {}
                }
            } else {
                return;
            }

            let interpreter = create_interpreter(
                funboy_ctx.clone(),
                MatrixCtx::new(room.clone(), pending_ask, event.sender),
            )
            .await;

            if funboy_ctx.in_use.load(Ordering::Relaxed) {
                room.send(RoomMessageEventContent::text_plain(
                    "You're already using a command, wait until it's finished.",
                ))
                .await
                .unwrap();
                return;
            } else {
                funboy_ctx.in_use.store(true, Ordering::Relaxed);
            }

            let result = interpret_bot_commands(
                &funboy_ctx,
                interpreter,
                &Permissions::power_user(),
                user_text,
            )
            .await;

            match result {
                Ok(result) => match result {
                    funboy_cli::CommandResult::Text(message) => {
                        // send our message to the room we found the command in
                        if !message.is_empty() {
                            let html = markdown_to_html(&message);
                            let content = RoomMessageEventContent::text_html(&message, html);
                            room.send(content).await.unwrap();
                        }
                    }
                    funboy_cli::CommandResult::ContextSwitch(_) => {}
                    funboy_cli::CommandResult::Exit => {}
                    funboy_cli::CommandResult::None => {}
                },
                Err(e) => {
                    let e = e.to_string();
                    let html = markdown_to_html(&e);
                    let content = RoomMessageEventContent::text_html(&e, html);
                    room.send(content).await.unwrap();
                }
            }

            funboy_ctx.in_use.store(false, Ordering::Relaxed);
        }
    });
}

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
