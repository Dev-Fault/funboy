use std::sync::Arc;

use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    types::{command::Executor, value::Value},
};
use funboy_cli::{ASK, ASK_RULES, DEFAULT_TIMEOUT_SECS, FunboyCtx, SAY, SAY_RULES};
use funboy_core::Funboy;
use matrix_sdk::{
    Room,
    ruma::{OwnedUserId, events::room::message::RoomMessageEventContent},
};
use tokio::sync::{Mutex, oneshot};

use crate::markdown_to_html;

#[derive(Clone)]
pub struct MatrixCtx {
    pub room: Room,
    pub pending_ask: Arc<Mutex<Option<(OwnedUserId, oneshot::Sender<String>)>>>,
    pub sender: OwnedUserId,
}

impl MatrixCtx {
    pub fn new(
        room: Room,
        pending_ask: Arc<Mutex<Option<(OwnedUserId, oneshot::Sender<String>)>>>,
        sender: OwnedUserId,
    ) -> Self {
        Self {
            room,
            pending_ask,
            sender,
        }
    }
}

#[derive(Clone)]
pub struct FslCtx {
    pub funboy_ctx: FunboyCtx,
    pub matrix_ctx: MatrixCtx,
    pub interpreter: Arc<Mutex<FslInterpreter>>,
}

impl FslCtx {
    pub fn new(funboy_ctx: FunboyCtx, matrix_ctx: MatrixCtx) -> Self {
        Self {
            funboy_ctx: funboy_ctx,
            matrix_ctx,
            interpreter: Arc::new(Mutex::new(FslInterpreter::new())),
        }
    }

    pub async fn generate_message(
        &self,
        message: &str,
    ) -> Result<String, fsl_interpreter::types::command::CommandError> {
        match self
            .funboy_ctx
            .funboy
            .generate(&message, self.interpreter.clone())
            .await
        {
            Ok(gen_msg) => Ok(gen_msg),
            Err(e) => {
                return Err(fsl_interpreter::types::command::CommandError::Custom(
                    e.to_string(),
                ));
            }
        }
    }
}

pub async fn create_interpreter(
    funboy_ctx: FunboyCtx,
    matrix_ctx: MatrixCtx,
) -> Arc<Mutex<FslInterpreter>> {
    let mut interpreter = FslInterpreter::new_unbounded();
    let fsl_ctx = FslCtx::new(funboy_ctx, matrix_ctx);
    interpreter.add_command(SAY, SAY_RULES, create_say_command(fsl_ctx.clone()));
    interpreter.add_command(ASK, ASK_RULES, create_ask_command(fsl_ctx.clone()));
    Arc::new(Mutex::new(interpreter))
}

pub fn create_ask_command(fsl_ctx: FslCtx) -> Executor {
    let ask_command = {
        move |command: fsl_interpreter::types::command::Command, data: Arc<InterpreterData>| {
            let fsl_ctx = fsl_ctx.clone();
            let room = fsl_ctx.matrix_ctx.room.clone();
            let sender = fsl_ctx.matrix_ctx.sender.clone();
            let pending_ask = fsl_ctx.matrix_ctx.pending_ask.clone();
            {
                async move {
                    let mut values = command.take_args();

                    let arg_0 = values.pop_front().unwrap().as_text(data.clone()).await?;
                    let arg_1 = values
                        .pop_front()
                        .unwrap_or(Value::Float(DEFAULT_TIMEOUT_SECS));

                    let question = format!("{}", arg_0);
                    let question = format!("{}\n{}", question, "(enter -STOP- to quit)");

                    let question = fsl_ctx.generate_message(&question).await;

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
                                return Ok(Value::Text(fsl_ctx.generate_message(&response).await?));
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

pub fn create_say_command(fsl_ctx: FslCtx) -> Executor {
    let say_command = {
        move |command: fsl_interpreter::types::command::Command, interpreter_data| {
            let fsl_ctx = fsl_ctx.clone();
            let room = fsl_ctx.matrix_ctx.room.clone();
            {
                async move {
                    let mut values = command.take_args();
                    let message = values
                        .pop_front()
                        .unwrap()
                        .as_text(interpreter_data)
                        .await?;

                    let message = fsl_ctx.generate_message(&message).await;

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
