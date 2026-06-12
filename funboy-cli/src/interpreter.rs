use std::sync::Arc;

use fsl_core::{FslInterpreter, error::RuntimeError};
use funboy_core::{
    Funboy,
    interpreter::{InterpreterContext, InterpreterLimits, Messenger},
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
        _timeout: f64,
    ) -> impl std::future::Future<Output = Result<String, RuntimeError>> + Send {
        let rl = self.rl.clone();
        async move {
            let mut rl = rl.lock().await;
            let result = rl.readline("A> ");

            match result {
                Ok(output) => Ok(output),
                Err(e) => {
                    return Err(RuntimeError::Custom(format!("{:?}", e)));
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
) -> FslInterpreter {
    let cli_context = CliContext::new(rl);
    let mut ictx = InterpreterContext::new(Id(0), funboy, cli_context, InterpreterLimits::none());
    ictx.register_default_funboy_commands();
    ictx.interpreter
        .register_library(fsl_core::libraries::Library::Exec);

    ictx.interpreter.clone()
}
