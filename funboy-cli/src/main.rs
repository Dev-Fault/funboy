use std::{sync::Arc, time::Duration};

use dotenvy::dotenv;
use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    commands::{NUMERIC_TYPES, TEXT_TYPES},
    types::{
        command::{ArgPos, ArgRule, Executor},
        value::Value,
    },
};
use funboy_cli::{
    BotData, CommandResult, Context,
    Error::{CommandError, ParseError},
    ParseError::{EmptyInput, UnknownCommand},
    interpret_bot_commands,
};
use funboy_core::{
    self, Funboy,
    ollama::{MAX_PREDICT, OllamaSettings},
    template_database::TemplateDatabase,
};
use rustyline::DefaultEditor;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Mutex;

struct Env {
    debug_mode: bool,
    db_url: String,
    default_ollama_model: Option<String>,
}

const SAY: &str = "say";
const SAY_RULES: &'static [ArgRule] = &[ArgRule::new(ArgPos::Index(0), TEXT_TYPES)];
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

const DEFAULT_TIMEOUT_SECS: f64 = 60.0 * 30.0;
const ASK: &str = "ask";
const ASK_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::OptionalIndex(1), NUMERIC_TYPES),
];
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

async fn get_env() -> Env {
    dotenv().ok();

    let debug_mode = std::env::var("DEBUG_MODE")
        .unwrap_or("false".to_string())
        .parse::<bool>()
        .expect("DEBUG_MODE must be of type bool");

    let db_url = if debug_mode == false {
        println!("Launching in release mode.");
        std::env::var("DATABASE_URL").expect("missing DATABASE_URL")
    } else {
        println!("Launching in debug mode.");
        std::env::var("DEBUG_DATABASE_URL").expect("missing DATABASE_URL")
    };

    let default_ollama_model = std::env::var("DEFAULT_OLLAMA_MODEL").ok();

    Env {
        debug_mode,
        db_url,
        default_ollama_model,
    }
}

async fn get_funboy(env: &Env) -> Funboy {
    let pool = Arc::new(
        PgPoolOptions::new()
            .max_connections(15)
            .min_connections(2)
            .acquire_timeout(Duration::from_secs(5))
            .idle_timeout(Duration::from_secs(60 * 10))
            .max_lifetime(Duration::from_secs(60 * 30))
            .connect(&env.db_url)
            .await
            .expect("failed to connect to database"),
    );

    TemplateDatabase::migrate(&pool)
        .await
        .expect("sqlx migration failed");

    Funboy::new(TemplateDatabase::new(pool))
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
    let env = get_env().await;
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

    loop {
        let mut rl_lock = rl.lock().await;
        let readline = rl_lock.readline(">> ");
        match readline {
            Ok(line) => {
                rl_lock.add_history_entry(&line)?;
                drop(rl_lock);
                match interpret_bot_commands(&bot_data, &line).await {
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
                        CommandResult::Exit => {
                            break;
                        }
                    },
                    Err(e) => match e {
                        CommandError(e) => println!("{}", e.to_string()),
                        ParseError(parse_error) => match parse_error {
                            EmptyInput => {
                                continue;
                            }
                            UnknownCommand(e) => eprintln!("{}", e),
                            funboy_cli::ParseError::MissingArg(e) => eprintln!("{}", e),
                        },
                    },
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
