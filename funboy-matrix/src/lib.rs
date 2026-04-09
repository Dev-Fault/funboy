use std::{sync::Arc, time::Duration};

use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    types::{command::Executor, value::Value},
};
use funboy_cli::{
    ASK, ASK_RULES, BotData, DEFAULT_TIMEOUT_SECS, Permissions, SAY, SAY_RULES,
    interpret_bot_commands,
};
use funboy_core::Funboy;
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

pub fn create_ask_command(
    funboy: Arc<Funboy>,
    room: Room,
    pending_ask: Arc<Mutex<Option<(OwnedUserId, oneshot::Sender<String>)>>>,
    sender: OwnedUserId,
) -> Executor {
    let ask_command = {
        move |command: fsl_interpreter::types::command::Command, data: Arc<InterpreterData>| {
            let room = room.clone();
            let pending_ask = pending_ask.clone();
            let sender = sender.clone();
            {
                let funboy = funboy.clone();
                async move {
                    let mut values = command.take_args();

                    let arg_0 = values.pop_front().unwrap().as_text(data.clone()).await?;
                    let arg_1 = values
                        .pop_front()
                        .unwrap_or(Value::Float(DEFAULT_TIMEOUT_SECS));

                    let question = format!("{}", arg_0);
                    let question = format!("{}\n{}", question, "(enter -STOP- to quit)");

                    let question = funboy
                        .generate(&question, Arc::new(Mutex::new(FslInterpreter::new())))
                        .await;

                    let question = match question {
                        Ok(question) => question,
                        Err(e) => {
                            return Err(fsl_interpreter::types::command::CommandError::Custom(
                                format!("{}", e.to_string()),
                            ));
                        }
                    };

                    let html = markdown_to_html(&question);
                    let question = RoomMessageEventContent::text_html(&question, html);
                    room.send(question).await.unwrap();

                    let (tx, rx) = oneshot::channel::<String>();
                    *pending_ask.lock().await = Some((sender, tx));

                    match rx.await {
                        Ok(response) => {
                            if response == "-STOP-" {
                                return Err(fsl_interpreter::types::command::CommandError::Custom(
                                    format!("{}", "user quit the program"),
                                ));
                            } else {
                                return Ok(Value::Text(response));
                            }
                        }
                        Err(e) => {
                            return Err(fsl_interpreter::types::command::CommandError::Custom(
                                format!("{}", e.to_string()),
                            ));
                        }
                    }
                }
            }
        }
    };
    Some(Arc::new(ask_command))
}

pub fn create_say_command(funboy: Arc<Funboy>, room: Room) -> Executor {
    let say_command = {
        move |command: fsl_interpreter::types::command::Command, interpreter_data| {
            let room = room.clone();
            {
                let funboy = funboy.clone();
                async move {
                    let mut values = command.take_args();
                    let message = values
                        .pop_front()
                        .unwrap()
                        .as_text(interpreter_data)
                        .await?;

                    let message = funboy
                        .generate(&message, Arc::new(Mutex::new(FslInterpreter::new())))
                        .await;

                    match message {
                        Ok(output) => {
                            if !output.is_empty() {
                                let html = markdown_to_html(&output);
                                let content = RoomMessageEventContent::text_html(&output, html);
                                room.send(content).await.unwrap();
                            }
                        }
                        Err(e) => {
                            return Err(fsl_interpreter::types::command::CommandError::Custom(
                                format!("{}", e.to_string()),
                            ));
                        }
                    }

                    Ok(Value::None)
                }
            }
        }
    };
    Some(Arc::new(say_command))
}

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
    bot_data: Arc<BotData>,
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
        let funboy = bot_data.funboy.clone();

        let mut interpreter = bot_data.interpreter.lock().await;
        interpreter.add_command(
            SAY,
            SAY_RULES,
            create_say_command(funboy.clone(), room.clone()),
        );
        interpreter.add_command(
            ASK,
            ASK_RULES,
            create_ask_command(funboy, room.clone(), pending_ask, event.sender),
        );
        drop(interpreter);

        if text_content.body.starts_with("!") {
            let result = interpret_bot_commands(
                &bot_data,
                &Permissions::power_user(),
                text_content.body.trim_start_matches("!"),
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
