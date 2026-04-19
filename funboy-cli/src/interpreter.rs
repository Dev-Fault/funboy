use std::sync::Arc;

use fsl_interpreter::FslInterpreter;
use funboy_core::{
    Funboy,
    interpreter::{ASK, ASK_RULES, InterpreterContext, Messenger, SAY, SAY_RULES},
    rate_limiter::RateLimit,
};
use rustyline::DefaultEditor;
use tokio::sync::Mutex;

use crate::Id;

#[derive(Clone)]
pub struct CliContext {
    rl: Arc<Mutex<DefaultEditor>>,
}

impl CliContext {
    pub fn new(rl: Arc<Mutex<DefaultEditor>>) -> Self {
        Self { rl }
    }
}

impl Messenger for CliContext {
    fn say(&self, message: &str) {
        println!("{message}");
    }

    fn await_response(
        &self,
        timeout: f64,
    ) -> impl std::future::Future<
        Output = Result<String, fsl_interpreter::types::command::CommandError>,
    > + Send {
        let rl = self.rl.clone();
        async move {
            let mut rl = rl.lock().await;
            let result = rl.readline("A> ");

            match result {
                Ok(output) => {
                    if output == "-STOP-" {
                        Err(fsl_interpreter::types::command::CommandError::Custom(
                            format!("{}", "user quit the program"),
                        ))
                    } else {
                        Ok(output)
                    }
                }
                Err(e) => {
                    return Err(fsl_interpreter::types::command::CommandError::Custom(
                        format!("{:?}", e),
                    ));
                }
            }
        }
    }

    fn mention(&self) -> String {
        Default::default()
    }
}

pub async fn create_interpreter(
    funboy: Arc<Funboy<Id>>,
    rl: Arc<Mutex<DefaultEditor>>,
) -> Arc<Mutex<FslInterpreter>> {
    let mut interpreter = FslInterpreter::new_unbounded();
    let cli_context = CliContext::new(rl);
    let ictx = InterpreterContext::new(
        Id(0),
        funboy,
        Arc::new(Mutex::new(RateLimit::none())),
        cli_context,
    );
    interpreter.add_command(
        SAY,
        SAY_RULES,
        funboy_core::interpreter::create_say_command(ictx.clone()),
    );
    interpreter.add_command(
        ASK,
        ASK_RULES,
        funboy_core::interpreter::create_ask_command(ictx.clone()),
    );
    Arc::new(Mutex::new(interpreter))
}
