use std::{str::FromStr, sync::Arc, time::Duration};

use clap::{Parser, ValueEnum};
use dotenvy::dotenv;
use fsl_core::FslInterpreter;
use funboy_core::{
    Funboy,
    commands::{
        AddArgs, CommandError, CommandResult, CopyArgs, DeleteArgs, GenerateArgs, ListArgs,
        OllamaArgs, RenameArgs, ReplaceArgs, parse_command_args,
    },
    database::{FunboyDatabase, Platform},
    user::FunboyUserId,
};
use rustyline::DefaultEditor;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Mutex;

use crate::interpreter::create_interpreter;

pub mod interpreter;

pub async fn enter_interactive_generation(
    funboy: Arc<Funboy<Id>>,
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
    funboy: Arc<Funboy<Id>>,
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

pub async fn enter_chat(
    funboy: Arc<Funboy<Id>>,
    rl: Arc<Mutex<DefaultEditor>>,
) -> rustyline::Result<()> {
    let interpreter = create_interpreter(funboy.clone(), rl.clone()).await;
    loop {
        let mut rl = rl.lock().await;
        let readline = rl.readline("C> ");

        match readline {
            Ok(input) => {
                rl.add_history_entry(&input)?;
                match funboy.user_chat(Id(0), input, interpreter.clone()).await {
                    Ok(response) => {
                        println!("{response}");
                    }
                    Err(e) => eprintln!("{e}"),
                };
                drop(rl);
            }
            Err(e) => {
                eprintln!("{e}");
                drop(rl);
                break;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Id(pub u64);
impl FunboyUserId for Id {}

impl ToString for Id {
    fn to_string(&self) -> String {
        self.0.to_string()
    }
}

pub struct FunboyEnv {
    pub debug_mode: bool,
    pub db_url: String,
    pub default_ollama_model: Option<String>,
}

impl FunboyEnv {
    pub fn new() -> FunboyEnv {
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

        FunboyEnv {
            debug_mode,
            db_url,
            default_ollama_model,
        }
    }
}

pub async fn get_funboy<U: FunboyUserId>(env: &FunboyEnv, platform: Platform) -> Funboy<U> {
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

    FunboyDatabase::migrate(&pool)
        .await
        .expect("sqlx migration failed");

    Funboy::new(FunboyDatabase::new(pool), platform)
}

const GENERATE: &str = "generate";
const FSL: &str = "fsl";

#[derive(Debug, Copy, Clone, ValueEnum)]
pub enum Mode {
    Generate,
    FSL,
    Chat,
}

impl FromStr for Mode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            GENERATE => Ok(Mode::Generate),
            FSL => Ok(Mode::FSL),
            _ => Err(format!("Unknown context {}", s)),
        }
    }
}

#[derive(Parser, Debug, Clone)]
pub enum CliCommand {
    Generate {
        #[command(flatten)]
        args: GenerateArgs,
    },
    Add {
        #[command(flatten)]
        args: AddArgs,
    },
    Delete {
        #[command(flatten)]
        args: DeleteArgs,
    },
    List {
        #[command(flatten)]
        args: ListArgs,
    },
    Copy {
        #[command(flatten)]
        args: CopyArgs,
    },
    Rename {
        #[command(flatten)]
        args: RenameArgs,
    },
    Replace {
        #[command(flatten)]
        args: ReplaceArgs,
    },
    Ollama {
        #[command(flatten)]
        args: OllamaArgs,
    },
    Mode {
        #[arg(value_parser = clap::value_parser!(Mode))]
        mode: Mode,
    },
    Cancel,
    Exit,
}

pub enum CliCommandResult {
    CommandResult(CommandResult),
    Mode(Mode),
    Exit,
}

impl Into<CliCommandResult> for CommandResult {
    fn into(self) -> CliCommandResult {
        CliCommandResult::CommandResult(self)
    }
}

pub async fn interpret_bot_commands<U: FunboyUserId>(
    user_id: U,
    funboy: &Funboy<U>,
    interpreter: Arc<Mutex<FslInterpreter>>,
    input: &str,
) -> Result<CliCommandResult, CommandError> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(CommandResult::None.into());
    };

    let args = parse_command_args(input);

    match CliCommand::try_parse_from(args) {
        Ok(command) => match command {
            CliCommand::Generate { args } => {
                let GenerateArgs {
                    file,
                    ollama,
                    input,
                } = args;
                funboy
                    .generate_command(Platform::Cli, user_id, interpreter, input, file, ollama)
                    .await
                    .map(|r| r.into())
            }
            CliCommand::Mode { mode } => return Ok(CliCommandResult::Mode(mode)),
            CliCommand::Add { args } => {
                let AddArgs {
                    template,
                    single,
                    file: _,
                    substitutes,
                } = args;
                let substitutes = substitutes.join(" ");
                funboy
                    .add_command(user_id, Platform::Cli, template, substitutes, single)
                    .await
                    .map(|r| r.into())
            }
            CliCommand::Delete { args } => {
                let DeleteArgs {
                    template,
                    single,
                    id,
                    substitutes,
                } = args;
                let substitutes = substitutes.join(" ");
                funboy
                    .delete_command(user_id, Platform::Cli, template, substitutes, single, id)
                    .await
                    .map(|r| r.into())
            }
            CliCommand::List { args } => {
                let ListArgs {
                    template,
                    search_term,
                    list_style,
                } = args;
                funboy
                    .list_command(template, search_term, list_style)
                    .await
                    .map(|r| r.into())
            }
            CliCommand::Ollama { args } => {
                let OllamaArgs { action } = args;
                funboy
                    .ollama_command(user_id, Platform::Cli, action)
                    .await
                    .map(|r| r.into())
            }
            CliCommand::Copy { args } => {
                let CopyArgs {
                    from_template,
                    to_template,
                } = args;
                funboy
                    .copy_command(user_id, from_template, to_template)
                    .await
                    .map(|r| r.into())
            }
            CliCommand::Rename { args } => {
                let RenameArgs {
                    from_template,
                    to_template,
                } = args;
                funboy
                    .rename_command(user_id, from_template, to_template)
                    .await
                    .map(|r| r.into())
            }
            CliCommand::Replace { args } => {
                let ReplaceArgs {
                    substitute,
                    with_substitute,
                    template,
                    id,
                } = args;
                funboy
                    .replace_command(user_id, template, substitute, with_substitute, id)
                    .await
                    .map(|r| r.into())
            }
            CliCommand::Exit => Ok(CliCommandResult::Exit),
            CliCommand::Cancel => funboy.cancel_command(user_id).await.map(|r| r.into()),
        },
        Err(e) => Err(CommandError::UnknownCommand(e.to_string())),
    }
}
