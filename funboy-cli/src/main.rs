use std::sync::Arc;

use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    types::{command::Executor, value::Value},
};
use funboy_cli::{
    ASK, ASK_RULES, BotData, CommandResult, Context, DEFAULT_TIMEOUT_SECS, Permissions, SAY,
    SAY_RULES, get_env, get_funboy, interpret_bot_commands,
};
use funboy_core::{
    Funboy,
    ollama::{MAX_PREDICT, OllamaSettings},
};
use rustyline::DefaultEditor;
use tokio::sync::Mutex;

pub fn create_say_command(funboy: Arc<Funboy>) -> Executor {
    let say_command = {
        move |command: fsl_interpreter::types::command::Command, interpreter_data| {
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
    };
    Some(Arc::new(say_command))
}

pub fn create_ask_command(funboy: Arc<Funboy>, rl: Arc<Mutex<DefaultEditor>>) -> Executor {
    let ask_command = {
        move |command: fsl_interpreter::types::command::Command, data: Arc<InterpreterData>| {
            let funboy = funboy.clone();
            let rl = rl.clone();
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
                            Ok(Value::Text(output))
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

pub async fn enter_interactive_generation(
    funboy: Arc<Funboy>,
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

pub async fn enter_interpreter(
    funboy: Arc<Funboy>,
    rl: Arc<Mutex<DefaultEditor>>,
) -> rustyline::Result<()> {
    let interpreter = create_interpreter(funboy.clone(), rl.clone()).await;
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

async fn create_interpreter(
    funboy: Arc<Funboy>,
    rl: Arc<Mutex<DefaultEditor>>,
) -> Arc<Mutex<FslInterpreter>> {
    let interpreter = Arc::new(Mutex::new(FslInterpreter::new_unbounded()));
    let mut interpreter_lock = interpreter.lock().await;
    interpreter_lock.add_command(SAY, SAY_RULES, create_say_command(funboy.clone()));
    interpreter_lock.add_command(
        ASK,
        ASK_RULES,
        create_ask_command(funboy.clone(), rl.clone()),
    );
    drop(interpreter_lock);
    interpreter
}

#[tokio::main]
async fn main() -> rustyline::Result<()> {
    let env = get_env();
    let funboy = Arc::new(get_funboy(&env).await);
    funboy.set_ollama_model(env.default_ollama_model).await;
    let rl = Arc::new(Mutex::new(DefaultEditor::new()?));
    let mut ollama_settings = OllamaSettings::default();
    ollama_settings.set_output_limit(MAX_PREDICT);

    let bot_data = BotData {
        funboy: funboy.clone(),
        interpreter: create_interpreter(funboy.clone(), rl.clone()).await,
        ollama_settings: Arc::new(Mutex::new(OllamaSettings::default())),
    };

    let permissions = Permissions::all();

    loop {
        let mut rl_lock = rl.lock().await;
        let readline = rl_lock.readline(">> ");
        match readline {
            Ok(line) => {
                rl_lock.add_history_entry(&line)?;
                drop(rl_lock);
                match interpret_bot_commands(&bot_data, &permissions, &line).await {
                    Ok(output) => match output {
                        CommandResult::Text(text) => println!("{}", text),
                        CommandResult::ContextSwitch(context) => match context {
                            Context::Generate => {
                                enter_interactive_generation(funboy.clone(), rl.clone()).await?;
                            }
                            Context::FSL => {
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
