use std::sync::Arc;

use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    types::{command::Executor, value::Value},
};
use funboy_cli::{
    ASK, ASK_RULES, CommandResult, Context, FslContext, FunboyEnv, Mode, Permissions, SAY,
    SAY_RULES, get_funboy, interpret_bot_commands,
};
use funboy_core::{
    Funboy, UserId,
    ollama::{MAX_PREDICT, OllamaSettings},
};
use rustyline::DefaultEditor;
use tokio::sync::Mutex;

fn create_say_command<U: UserId>(fsl_ctx: FslContext<U>) -> Executor {
    let say_command = {
        move |command: fsl_interpreter::types::command::Command, interpreter_data| {
            let fsl_ctx = fsl_ctx.clone();
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
                        Ok(output) => println!("{}", output),
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

fn create_ask_command<U: UserId>(
    fsl_ctx: FslContext<U>,
    rl: Arc<Mutex<DefaultEditor>>,
) -> Executor {
    let ask_command = {
        move |command: fsl_interpreter::types::command::Command, data: Arc<InterpreterData>| {
            let fsl_ctx = fsl_ctx.clone();
            let rl = rl.clone();
            async move {
                let mut values = command.take_args();

                let arg_0 = values.pop_front().unwrap().as_text(data.clone()).await?;

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

                println!("{}", question);

                let mut rl = rl.lock().await;
                let result = rl.readline("A> ");
                match result {
                    Ok(output) => {
                        if output == "-STOP-" {
                            Err(fsl_interpreter::types::command::CommandError::Custom(
                                format!("{}", "user quit the program"),
                            ))
                        } else {
                            Ok(Value::Text(fsl_ctx.generate_message(&output).await?))
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
    };
    Some(Arc::new(ask_command))
}

pub async fn enter_interactive_generation<U: UserId>(
    funboy: Arc<Funboy<U>>,
    rl: Arc<Mutex<DefaultEditor>>,
) -> rustyline::Result<()> {
    let interpreter = create_interpreter(funboy.clone(), rl.clone()).await;
    loop {
        let mut rl = rl.lock().await;
        let readline = rl.readline("G> ");
        match readline {
            Ok(input) => {
                rl.add_history_entry(&input)?;
                drop(rl);
                match funboy.generate(&input, interpreter.clone()).await {
                    Ok(output) => println!("{}", output),
                    Err(e) => {
                        eprint!("{:?}", e);
                    }
                };
            }
            Err(e) => {
                eprintln!("{:?}", e);
                drop(rl);
                break;
            }
        }
    }
    Ok(())
}

pub async fn enter_interpreter<U: UserId>(
    funboy: Arc<Funboy<U>>,
    rl: Arc<Mutex<DefaultEditor>>,
) -> rustyline::Result<()> {
    let interpreter = create_interpreter(funboy, rl.clone()).await;
    loop {
        let mut rl = rl.lock().await;
        let readline = rl.readline("I> ");

        match readline {
            Ok(input) => {
                rl.add_history_entry(&input)?;
                drop(rl);
                let interpreter_lock = interpreter.lock().await;
                let result = interpreter_lock.interpret(&input).await;
                drop(interpreter_lock);
                match result {
                    Ok(output) => println!("{}", output),
                    Err(e) => {
                        eprintln!("{:?}", e)
                    }
                }
            }
            Err(e) => {
                eprintln!("{:?}", e);
                drop(rl);
                break;
            }
        }
    }
    Ok(())
}

async fn create_interpreter<U: UserId>(
    funboy: Arc<Funboy<U>>,
    rl: Arc<Mutex<DefaultEditor>>,
) -> Arc<Mutex<FslInterpreter>> {
    let mut interpreter = FslInterpreter::new_unbounded();
    let fsl_context = FslContext::new(funboy);
    interpreter.add_command(SAY, SAY_RULES, create_say_command(fsl_context.clone()));
    interpreter.add_command(
        ASK,
        ASK_RULES,
        create_ask_command(fsl_context.clone(), rl.clone()),
    );
    Arc::new(Mutex::new(interpreter))
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct Id(u64);
impl UserId for Id {}

#[tokio::main]
async fn main() -> rustyline::Result<()> {
    let env = FunboyEnv::new();
    let funboy = Arc::new(get_funboy::<Id>(&env).await);
    funboy.set_ollama_model(env.default_ollama_model).await;
    let rl = Arc::new(Mutex::new(DefaultEditor::new()?));
    let mut ollama_settings = OllamaSettings::default();
    ollama_settings.set_output_limit(MAX_PREDICT);

    let permissions = Permissions::all();

    loop {
        let mut rl_lock = rl.lock().await;
        let readline = rl_lock.readline(">> ");
        match readline {
            Ok(line) => {
                rl_lock.add_history_entry(&line)?;
                drop(rl_lock);
                match interpret_bot_commands(
                    Id(0),
                    &funboy,
                    create_interpreter(funboy.clone(), rl.clone()).await,
                    &permissions,
                    Context::Cli,
                    &line,
                )
                .await
                {
                    Ok(output) => match output {
                        CommandResult::Text(text) => println!("{}", text),
                        CommandResult::Mode(context) => match context {
                            Mode::Generate => {
                                enter_interactive_generation(funboy.clone(), rl.clone()).await?;
                            }
                            Mode::FSL => {
                                enter_interpreter(funboy.clone(), rl.clone()).await?;
                            }
                        },
                        CommandResult::None => {
                            continue;
                        }
                        CommandResult::Exit => {
                            break;
                        }
                    },
                    Err(e) => println!("{}", e.to_string()),
                }
            }
            Err(e) => {
                eprintln!("{}", e);
                break;
            }
        }
    }

    Ok(())
}
