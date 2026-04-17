use std::{collections::HashMap, sync::Arc, time::Duration};

use fsl_interpreter::{FslInterpreter, types::command::CommandError};
use funboy_core::{
    Funboy,
    interpreter::{
        ASK, ASK_RULES, ASK_TO, ASK_TO_RULES, CommunicationChannel, InterpreterContext, SAY,
        SAY_RULES, SAY_TO, SAY_TO_RULES,
    },
    rate_limiter::RateLimit,
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
        oneshot::{self},
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

    pub async fn await_response_from_user(
        &self,
        user: MatrixUser,
        timeout: f64,
    ) -> Result<String, CommandError> {
        let (tx, rx) = oneshot::channel::<String>();
        let mut asks = self.pending_asks.lock().await;
        asks.insert(user, tx);
        drop(asks);

        match timeout_at(Instant::now() + Duration::from_secs_f64(timeout), rx).await {
            Ok(Ok(response)) => {
                if response == "-STOP-" {
                    return Err(fsl_interpreter::types::command::CommandError::Custom(
                        format!("{}", "user quit the program"),
                    ));
                } else {
                    return Ok(response);
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
}

impl CommunicationChannel for MatrixCtx {
    fn say(&self, message: &str) {
        let room = self.room.clone();
        let message = message.to_owned();
        tokio::spawn(async move {
            let content = RoomMessageEventContent::text_markdown(message);
            room.send(content).await.unwrap();
        });
    }

    fn say_to_user(
        &self,
        user_name: &str,
        message: &str,
    ) -> impl std::future::Future<Output = Result<(), CommandError>> + Send {
        let room = self.room.clone();
        let message = message.to_owned();
        async move {
            let user = user_name_to_id(&user_name, room.clone()).await?;

            if !message.is_empty() {
                let message =
                    RoomMessageEventContent::text_markdown(format!("{}\n\n{}", user, &message));
                let message = message.add_mentions(Mentions::with_user_ids([user]));
                room.send(message).await.unwrap();
            }

            Ok(())
        }
    }

    fn mention(&self) -> String {
        self.sender.user_id.to_string()
    }

    fn await_response(
        &self,
        timeout: f64,
    ) -> impl std::future::Future<Output = Result<String, CommandError>> + Send {
        async move {
            self.await_response_from_user(self.sender.clone(), timeout)
                .await
        }
    }

    fn await_user_response(
        &self,
        user_name: &str,
        timeout: f64,
    ) -> impl std::future::Future<Output = Result<String, CommandError>> + Send {
        async move {
            let user = user_name_to_id(&user_name, self.room.clone()).await?;
            let receiver = MatrixUser::new(self.room.room_id().into(), user);
            self.await_response_from_user(receiver, timeout).await
        }
    }
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

pub async fn create_interpreter(
    funboy: Arc<Funboy<MatrixUser>>,
    matrix_ctx: MatrixCtx,
) -> Arc<Mutex<FslInterpreter>> {
    let mut interpreter = FslInterpreter::new();
    let ictx = InterpreterContext::new(
        matrix_ctx.clone().sender,
        funboy.clone(),
        // TODO give this a longer lifetime
        Arc::new(Mutex::new(RateLimit::default())),
        matrix_ctx,
    );
    interpreter.add_command(
        SAY,
        SAY_RULES,
        funboy_core::interpreter::create_say_command(ictx.clone()),
    );
    interpreter.add_command(
        SAY_TO,
        SAY_TO_RULES,
        funboy_core::interpreter::create_say_to_command(ictx.clone()),
    );
    interpreter.add_command(
        ASK,
        ASK_RULES,
        funboy_core::interpreter::create_ask_command(ictx.clone()),
    );
    interpreter.add_command(
        ASK_TO,
        ASK_TO_RULES,
        funboy_core::interpreter::create_ask_to_command(ictx.clone()),
    );
    Arc::new(Mutex::new(interpreter))
}
