use std::sync::Arc;

use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    types::{command::Executor, value::Value},
};
use funboy_cli::DEFAULT_TIMEOUT_SECS;
use funboy_core::Funboy;
use matrix_sdk::{
    Room,
    ruma::{OwnedUserId, events::room::message::RoomMessageEventContent},
};
use tokio::sync::{Mutex, oneshot};

use crate::markdown_to_html;

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
