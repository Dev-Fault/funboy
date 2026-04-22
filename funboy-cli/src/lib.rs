use std::{str::FromStr, sync::Arc, time::Duration};

use clap::{Parser, ValueEnum};
use dotenvy::dotenv;
use fsl_interpreter::FslInterpreter;
use funboy_core::{
    Funboy,
    commands::{CommandError, CommandResult, OllamaAction, parse_command_args},
    database::{FunboyDatabase, Platform},
    format::{LIST_STYLE_NONE, ListStyle},
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
pub enum Command {
    Generate {
        #[arg(trailing_var_arg = true)]
        input: Vec<String>,

        #[arg(short, long)]
        file: bool,

        #[arg(short, long)]
        ollama: bool,
    },
    Add {
        template: String,

        #[arg(short, long)]
        single: bool,

        #[arg(short, long)]
        file: bool,

        #[arg(trailing_var_arg = true)]
        substitutes: Vec<String>,
    },
    Delete {
        template: String,

        #[arg(short, long)]
        single: bool,

        #[arg(short, long)]
        id: bool,

        #[arg(trailing_var_arg = true)]
        substitutes: Vec<String>,
    },
    List {
        template: Option<String>,

        #[arg(short, long, default_value = None)]
        search_term: Option<String>,

        #[arg(short, long, value_parser = clap::value_parser!(ListStyle), default_value = LIST_STYLE_NONE)]
        list_style: ListStyle,
    },
    Copy {
        from_template: String,
        to_template: String,
    },
    Rename {
        from_template: String,
        to_template: String,
    },
    Replace {
        substitute: String,
        with_substitute: String,

        #[arg(short, long)]
        template: Option<String>,

        #[arg(short, long)]
        id: bool,
    },
    Ollama {
        #[command(subcommand)]
        action: OllamaAction,
    },
    Mode {
        #[arg(value_parser = clap::value_parser!(Mode))]
        mode: Mode,
    },
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

    match Command::try_parse_from(args) {
        Ok(command) => match command {
            Command::Generate {
                input,
                file,
                ollama,
            } => funboy
                .generate_command(Platform::Cli, user_id, interpreter, input, file, ollama)
                .await
                .map(|r| r.into()),
            Command::Mode { mode } => return Ok(CliCommandResult::Mode(mode)),
            Command::Add {
                template,
                substitutes,
                single,
                file: _,
            } => {
                let substitutes = substitutes.join(" ");
                funboy
                    .add_command(user_id, Platform::Cli, template, substitutes, single)
                    .await
                    .map(|r| r.into())
            }
            Command::Delete {
                template,
                substitutes,
                single,
                id,
            } => {
                let substitutes = substitutes.join(" ");
                funboy
                    .delete_command(user_id, Platform::Cli, template, substitutes, single, id)
                    .await
                    .map(|r| r.into())
            }
            Command::List {
                template,
                search_term,
                list_style,
            } => funboy
                .list_command(template, search_term, list_style)
                .await
                .map(|r| r.into()),
            Command::Ollama { action } => funboy
                .ollama_command(user_id, Platform::Cli, action)
                .await
                .map(|r| r.into()),
            Command::Copy {
                from_template,
                to_template,
            } => funboy
                .copy_command(user_id, from_template, to_template)
                .await
                .map(|r| r.into()),
            Command::Rename {
                from_template,
                to_template,
            } => funboy
                .rename_command(user_id, from_template, to_template)
                .await
                .map(|r| r.into()),
            Command::Replace {
                template,
                substitute,
                with_substitute,
                id,
            } => funboy
                .replace_command(user_id, template, substitute, with_substitute, id)
                .await
                .map(|r| r.into()),
            Command::Exit => Ok(CliCommandResult::Exit),
        },
        Err(e) => Err(CommandError::UnknownCommand(e.to_string())),
    }
}
