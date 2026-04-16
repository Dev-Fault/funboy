use std::{collections::HashMap, sync::Arc, time::Duration};

use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    types::{
        command::{CommandError, Executor},
        value::Value,
    },
};
use funboy_core::{
    Funboy,
    interpreter::{
        ASK, ASK_RULES, ASK_TO, ASK_TO_RULES, DEFAULT_TIMEOUT_SECS, SAY, SAY_RULES, SAY_TO,
        SAY_TO_RULES,
    },
};
use matrix_sdk::{
    Room,
    ruma::{
        OwnedUserId,
        events::{Mentions, room::message::RoomMessageEventContent},
    },
};
use tokio::{
    sync::{
        Mutex,
        oneshot::{self, Sender},
    },
    time::{Instant, timeout_at},
};

use crate::MatrixUser;

#[derive(Clone)]
pub struct MatrixCtx {
    pub room: Room,
    pub pending_asks: Arc<Mutex<HashMap<MatrixUser, oneshot::Sender<String>>>>,
    pub sender: MatrixUser,
}

impl MatrixCtx {
    pub fn new(
        room: Room,
        pending_asks: Arc<Mutex<HashMap<MatrixUser, oneshot::Sender<String>>>>,
        sender: MatrixUser,
    ) -> Self {
        Self {
            room,
            pending_asks,
            sender,
        }
    }
}

#[derive(Clone)]
pub struct FslCtx {
    pub funboy: Arc<Funboy<MatrixUser>>,
    pub matrix_ctx: MatrixCtx,
    pub interpreter: Arc<Mutex<FslInterpreter>>,
}

impl FslCtx {
    pub fn new(funboy: Arc<Funboy<MatrixUser>>, matrix_ctx: MatrixCtx) -> Self {
        Self {
            funboy,
            matrix_ctx,
            interpreter: Arc::new(Mutex::new(FslInterpreter::new())),
        }
    }

    pub async fn generate_message(
        &self,
        message: &str,
    ) -> Result<String, fsl_interpreter::types::command::CommandError> {
        match self
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
    funboy: Arc<Funboy<MatrixUser>>,
    matrix_ctx: MatrixCtx,
) -> Arc<Mutex<FslInterpreter>> {
    let mut interpreter = FslInterpreter::new();
    let fsl_ctx = FslCtx::new(funboy, matrix_ctx);
    interpreter.add_command(SAY, SAY_RULES, create_say_command(fsl_ctx.clone()));
    interpreter.add_command(SAY_TO, SAY_TO_RULES, create_say_to_command(fsl_ctx.clone()));
    interpreter.add_command(ASK, ASK_RULES, create_ask_command(fsl_ctx.clone()));
    interpreter.add_command(ASK_TO, ASK_TO_RULES, create_ask_to_command(fsl_ctx.clone()));
    Arc::new(Mutex::new(interpreter))
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

                    let message = fsl_ctx.generate_message(&message).await?;

                    if !message.is_empty() {
                        let message = RoomMessageEventContent::text_markdown(&message);
                        room.send(message).await.unwrap();
                    }

                    Ok(Value::None)
                }
            }
        }
    };
    Some(Arc::new(say_command))
}

async fn user_name_to_id(user_name: &str, room: Room) -> Result<OwnedUserId, CommandError> {
    let users = room.users_with_power_levels().await;

    let users: Vec<&OwnedUserId> = users
        .iter()
        .map(|u| u.0)
        .filter(|u| u.as_str() == user_name || u.localpart() == user_name)
        .collect();

    let user = match users.get(0) {
        Some(user) => user,
        None => {
            return Err(fsl_interpreter::types::command::CommandError::Custom(
                format!("no user named {} present in room", user_name),
            ));
        }
    };
    Ok((*user).clone())
}

pub fn create_say_to_command(fsl_ctx: FslCtx) -> Executor {
    let say_command = {
        move |command: fsl_interpreter::types::command::Command,
              interpreter_data: Arc<InterpreterData>| {
            let fsl_ctx = fsl_ctx.clone();
            let room = fsl_ctx.matrix_ctx.room.clone();
            {
                async move {
                    let mut values = command.take_args();
                    let user_name = values
                        .pop_front()
                        .unwrap()
                        .as_text(interpreter_data.clone())
                        .await?;
                    let message = values
                        .pop_front()
                        .unwrap()
                        .as_text(interpreter_data)
                        .await?;

                    let user = user_name_to_id(&user_name, room.clone()).await?;

                    let message = fsl_ctx.generate_message(&message).await?;

                    if !message.is_empty() {
                        let message = RoomMessageEventContent::text_markdown(format!(
                            "{}\n\n{}",
                            user, &message
                        ));
                        let message = message.add_mentions(Mentions::with_user_ids([user]));
                        room.send(message).await.unwrap();
                    }

                    Ok(Value::None)
                }
            }
        }
    };
    Some(Arc::new(say_command))
}

async fn await_response(
    fsl_ctx: FslCtx,
    sender: MatrixUser,
    pending_asks: Arc<Mutex<HashMap<MatrixUser, Sender<String>>>>,
    timeout: f64,
) -> Result<Value, CommandError> {
    let (tx, rx) = oneshot::channel::<String>();
    let mut asks = pending_asks.lock().await;
    asks.insert(sender, tx);
    drop(asks);

    match timeout_at(Instant::now() + Duration::from_secs_f64(timeout), rx).await {
        Ok(Ok(response)) => {
            if response == "-STOP-" {
                return Err(fsl_interpreter::types::command::CommandError::Custom(
                    format!("{}", "user quit the program"),
                ));
            } else {
                return Ok(Value::Text(fsl_ctx.generate_message(&response).await?));
            }
        }
        Ok(Err(e)) => {
            return Err(fsl_interpreter::types::command::CommandError::Custom(
                format!("{}", e.to_string()),
            ));
        }
        Err(_) => {
            return Err(fsl_interpreter::types::command::CommandError::Custom(
                format!("{}", "didn't receive message before timeout"),
            ));
        }
    }
}

pub fn create_ask_command(fsl_ctx: FslCtx) -> Executor {
    let ask_command = {
        move |command: fsl_interpreter::types::command::Command, data: Arc<InterpreterData>| {
            let fsl_ctx = fsl_ctx.clone();
            let room = fsl_ctx.matrix_ctx.room.clone();
            let sender = fsl_ctx.matrix_ctx.sender.clone();
            let pending_asks = fsl_ctx.matrix_ctx.pending_asks.clone();
            {
                async move {
                    let mut values = command.take_args();

                    let question = values.pop_front().unwrap().as_text(data.clone()).await?;
                    let timeout = values
                        .pop_front()
                        .unwrap_or(Value::Float(DEFAULT_TIMEOUT_SECS))
                        .as_float(data.clone())
                        .await?;

                    let question = format!("{}\n{}", question, "(enter -STOP- to quit)");

                    let question = fsl_ctx.generate_message(&question).await?;

                    let question = RoomMessageEventContent::text_markdown(&question);
                    room.send(question).await.unwrap();

                    await_response(fsl_ctx, sender, pending_asks, timeout).await
                }
            }
        }
    };
    Some(Arc::new(ask_command))
}

pub fn create_ask_to_command(fsl_ctx: FslCtx) -> Executor {
    let ask_command = {
        move |command: fsl_interpreter::types::command::Command, data: Arc<InterpreterData>| {
            let fsl_ctx = fsl_ctx.clone();
            let room = fsl_ctx.matrix_ctx.room.clone();
            let pending_asks = fsl_ctx.matrix_ctx.pending_asks.clone();
            {
                async move {
                    let mut values = command.take_args();

                    let user_name = values.pop_front().unwrap().as_text(data.clone()).await?;
                    let question = values.pop_front().unwrap().as_text(data.clone()).await?;
                    let timeout = values
                        .pop_front()
                        .unwrap_or(Value::Float(DEFAULT_TIMEOUT_SECS))
                        .as_float(data.clone())
                        .await?;

                    let user = user_name_to_id(&user_name, room.clone()).await?;

                    let question =
                        format!("{}\n\n{}\n{}", user, question, "(enter -STOP- to quit)");

                    let question = fsl_ctx.generate_message(&question).await?;

                    let question = RoomMessageEventContent::text_markdown(&question);
                    room.send(question).await.unwrap();

                    let receiver = MatrixUser::new(room.room_id().into(), user);
                    await_response(fsl_ctx, receiver, pending_asks, timeout).await
                }
            }
        }
    };
    Some(Arc::new(ask_command))
}
