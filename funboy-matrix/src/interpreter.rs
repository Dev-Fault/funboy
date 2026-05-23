use std::{collections::HashMap, sync::Arc, time::Duration};

use fsl_core::{FslInterpreter, error::RuntimeError};
use funboy_core::{
    Funboy,
    interpreter::{
        ASK, ASK_RULES, ASK_TO, ASK_TO_RULES, Interactor, InterpreterContext, InterpreterLimits,
        Messenger, SAY, SAY_RULES, SAY_TO, SAY_TO_RULES,
    },
};
use matrix_sdk::{
    Room,
    ruma::{OwnedUserId, events::room::message::RoomMessageEventContent},
};
use tokio::{
    sync::{
        Mutex,
        oneshot::{self},
    },
    time::{Instant, timeout_at},
};

use crate::{MatrixUser, send_msg_with_mixed_content};

#[derive(Clone)]
pub struct MatrixCtx {
    pub funboy: Arc<Funboy<MatrixUser>>,
    pub room: Room,
    pub pending_asks: Arc<Mutex<HashMap<MatrixUser, oneshot::Sender<String>>>>,
    pub sender: MatrixUser,
    pub interpreter_limits: InterpreterLimits<MatrixUser>,
}

impl MatrixCtx {
    pub fn new(
        funboy: Arc<Funboy<MatrixUser>>,
        room: Room,
        pending_asks: Arc<Mutex<HashMap<MatrixUser, oneshot::Sender<String>>>>,
        sender: MatrixUser,
    ) -> Self {
        Self {
            funboy,
            room,
            pending_asks,
            sender,
            interpreter_limits: InterpreterLimits::default(),
        }
    }

    pub async fn await_response_from_user(
        &self,
        user: MatrixUser,
        timeout: f64,
    ) -> Result<String, RuntimeError> {
        let (tx, rx) = oneshot::channel::<String>();
        let mut asks = self.pending_asks.lock().await;
        asks.insert(user, tx);
        drop(asks);

        match timeout_at(Instant::now() + Duration::from_secs_f64(timeout), rx).await {
            Ok(Ok(response)) => {
                return Ok(response);
            }
            Ok(Err(e)) => {
                return Err(RuntimeError::Custom(format!("{}", e.to_string())));
            }
            Err(_) => {
                return Err(RuntimeError::Custom(format!(
                    "{}",
                    "didn't receive message before timeout"
                )));
            }
        }
    }
}

impl Messenger for MatrixCtx {
    fn say(&self, message: &str) {
        let room = self.room.clone();
        let message = message.to_owned();
        let funboy = self.funboy.clone();
        let user_id = self.sender.clone();
        tokio::spawn(async move {
            match funboy.users.get_permissions(user_id).await {
                Ok(permissions) => send_msg_with_mixed_content(&message, &permissions, room).await,
                Err(e) => {
                    room.send(RoomMessageEventContent::text_plain(e.to_string()))
                        .await
                        .unwrap();
                }
            };
        });
    }

    fn mention(&self) -> String {
        self.sender.user_id.to_string()
    }

    fn await_response(
        &self,
        timeout: f64,
    ) -> impl std::future::Future<Output = Result<String, RuntimeError>> + Send {
        async move {
            self.await_response_from_user(self.sender.clone(), timeout)
                .await
        }
    }
}

impl Interactor for MatrixCtx {
    fn say_to_user(
        &self,
        user_name: &str,
        message: &str,
    ) -> impl std::future::Future<Output = Result<(), RuntimeError>> + Send {
        let room = self.room.clone();
        let message = message.to_owned();
        let funboy = self.funboy.clone();
        let user_id = self.sender.clone();
        async move {
            let user = user_name_to_id(&user_name, room.clone()).await?;

            if !message.trim().is_empty() {
                let message = format!("{}\n\n{}", user, &message);
                match funboy.users.get_permissions(user_id).await {
                    Ok(permissions) => {
                        send_msg_with_mixed_content(&message, &permissions, room).await
                    }
                    Err(e) => {
                        room.send(RoomMessageEventContent::text_plain(e.to_string()))
                            .await
                            .unwrap();
                    }
                };
            }

            Ok(())
        }
    }

    fn await_user_response(
        &self,
        user_name: &str,
        timeout: f64,
    ) -> impl std::future::Future<Output = Result<String, RuntimeError>> + Send {
        async move {
            let user = user_name_to_id(&user_name, self.room.clone()).await?;
            let receiver = MatrixUser::new(self.room.room_id().into(), user);
            self.await_response_from_user(receiver, timeout).await
        }
    }
}

async fn user_name_to_id(user_name: &str, room: Room) -> Result<OwnedUserId, RuntimeError> {
    let users = room.users_with_power_levels().await;

    let users: Vec<&OwnedUserId> = users
        .iter()
        .map(|u| u.0)
        .filter(|u| u.as_str() == user_name || u.localpart() == user_name)
        .collect();

    let user = match users.get(0) {
        Some(user) => user,
        None => {
            return Err(RuntimeError::Custom(format!(
                "no user named {} present in room",
                user_name
            )));
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
        matrix_ctx.clone(),
        matrix_ctx.interpreter_limits,
    );
    interpreter.register(
        SAY,
        SAY_RULES,
        funboy_core::interpreter::say_command(ictx.clone()),
    );
    interpreter.register(
        SAY_TO,
        SAY_TO_RULES,
        funboy_core::interpreter::say_to_command(ictx.clone()),
    );
    interpreter.register(
        ASK,
        ASK_RULES,
        funboy_core::interpreter::ask_command(ictx.clone()),
    );
    interpreter.register(
        ASK_TO,
        ASK_TO_RULES,
        funboy_core::interpreter::ask_to_command(ictx.clone()),
    );

    #[cfg(all(target_os = "linux", feature = "sh_exec"))]
    {
        use funboy_core::interpreter::sh_exec::*;
        let permissions = funboy
            .users
            .get_permissions(matrix_ctx.sender.clone())
            .await;

        match permissions {
            Ok(permissions) => {
                if permissions.can_exec() {
                    interpreter.register(SANDBOXED_SH, SANDBOXED_SH_RULES, sandboxed_sh_command());
                    interpreter.register(
                        INTERACTIVE_SH,
                        INTERACTIVE_SH_RULES,
                        interactive_sh_command(ictx),
                    );
                }
            }
            Err(e) => eprintln!("{e}"),
        }
    }

    Arc::new(Mutex::new(interpreter))
}
