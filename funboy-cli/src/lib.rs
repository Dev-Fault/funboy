use std::{str::FromStr, sync::Arc, time::Duration};

use clap::Parser;
use dotenvy::dotenv;
use fsl_interpreter::{
    FslInterpreter,
    commands::{NUMERIC_TYPES, TEXT_TYPES},
    types::command::{ArgPos, ArgRule},
};
use funboy_core::{
    Funboy,
    ollama::OllamaSettings,
    template_database::{Limit, OrderBy, TemplateDatabase},
};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Mutex;

pub const SAY: &str = "say";
pub const SAY_RULES: &'static [ArgRule] = &[ArgRule::new(ArgPos::Index(0), TEXT_TYPES)];
pub const DEFAULT_TIMEOUT_SECS: f64 = 60.0 * 30.0;
pub const ASK: &str = "ask";
pub const ASK_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::OptionalIndex(1), NUMERIC_TYPES),
];

pub struct FunboyEnv {
    pub debug_mode: bool,
    pub db_url: String,
    pub default_ollama_model: Option<String>,
}

pub fn get_env() -> FunboyEnv {
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

pub async fn get_funboy(env: &FunboyEnv) -> Funboy {
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

#[derive(Debug)]
pub enum ParseError {
    EmptyInput,
    UnknownCommand(String),
    MissingArg(String),
}

impl Into<Error> for ParseError {
    fn into(self) -> Error {
        Error::ParseError(self)
    }
}

#[derive(Debug)]
pub enum CommandError {
    ExecutionFailed(String),
}

impl ToString for CommandError {
    fn to_string(&self) -> String {
        match self {
            CommandError::ExecutionFailed(error_text) => error_text.clone(),
        }
    }
}

impl Into<Error> for CommandError {
    fn into(self) -> Error {
        Error::CommandError(self)
    }
}

#[derive(Debug)]
pub enum Error {
    CommandError(CommandError),
    ParseError(ParseError),
}

const GENERATE: &str = "generate";
const FSL: &str = "fsl";

#[derive(Debug, Copy, Clone)]
pub enum Context {
    Generate,
    FSL,
}

impl FromStr for Context {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            GENERATE => Ok(Context::Generate),
            FSL => Ok(Context::FSL),
            _ => Err(format!("Unknown context {}", s)),
        }
    }
}

pub enum CommandResult {
    Text(String),
    ContextSwitch(Context),
    Exit,
}

const DEFAULT: &str = "default";
const ID: &str = "id";

#[derive(Debug, Copy, Clone)]
pub enum ListStyle {
    Default,
    Id,
}

impl FromStr for ListStyle {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            DEFAULT => Ok(ListStyle::Default),
            ID => Ok(ListStyle::Id),
            _ => Err(format!("Unknown context {}", s)),
        }
    }
}

#[derive(Parser, Debug)]
enum Command {
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

        #[arg(trailing_var_arg = true)]
        substitutes: Vec<String>,
    },
    Delete {
        template: String,

        #[arg(short, long)]
        single: bool,

        #[arg(trailing_var_arg = true)]
        substitutes: Vec<String>,
    },
    List {
        template: Option<String>,

        #[arg(short, long, default_value = None)]
        search_term: Option<String>,

        #[arg(short, long, value_parser = clap::value_parser!(ListStyle), default_value = DEFAULT)]
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
    Context {
        #[arg(value_parser = clap::value_parser!(Context))]
        context: Context,
    },
    Exit,
}

const MODEL: &str = "model";
const MODELS: &str = "models";
const SETTINGS: &str = "settings";

#[derive(Parser, Debug, Copy, Clone)]
enum OllamaListOption {
    Model,
    Models,
    Settings,
}

impl FromStr for OllamaListOption {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            MODEL => Ok(OllamaListOption::Model),
            MODELS => Ok(OllamaListOption::Models),
            SETTINGS => Ok(OllamaListOption::Settings),
            _ => Err(format!("Unknown list option {}", s)),
        }
    }
}

#[derive(Parser, Debug, Clone)]
enum OllamaSetOptions {
    #[command(name = "model")]
    Model { model: String },
    #[command(name = "system_prompt")]
    SystemPrompt {
        #[arg(trailing_var_arg = true)]
        system_prompt: Vec<String>,
    },
    #[command(name = "template")]
    Template { template: String },
    #[command(name = "output_limit")]
    OutputLimit { limit: u16 },
    #[command(name = "temperature")]
    Temperature { temperature: f32 },
    #[command(name = "top_k")]
    TopK { top_k: u32 },
    #[command(name = "top_p")]
    TopP { top_p: f32 },
    #[command(name = "repeat_penalty")]
    RepeatPenalty { repeat_penalty: f32 },
}
/*
#[derive(Copy, Clone)]
pub struct OllamaParameters {
    pub temperature: Option<f32>,
    pub repeat_penalty: Option<f32>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
}
*/

#[derive(Parser, Debug)]
enum OllamaAction {
    List {
        #[arg(value_parser = clap::value_parser!(OllamaListOption))]
        option: OllamaListOption,
    },

    Set {
        #[command(subcommand)]
        option: OllamaSetOptions,
    },
}

fn parse_substitutes<'a>(input: &'a str, single: bool) -> Vec<&'a str> {
    if single {
        return vec![input];
    } else {
        let mut subs: Vec<&str> = Vec::new();
        let mut in_quotes = false;
        let bytes = input.as_bytes();

        let mut start = 0;
        for (end, byte) in bytes.iter().enumerate() {
            match byte {
                b'"' => {
                    if !in_quotes {
                        start = end + 1;
                    } else {
                        subs.push(&input[start..end]);
                        start = end + 1;
                    }
                    in_quotes = !in_quotes;
                }
                b' ' if !in_quotes => {
                    if start != end {
                        subs.push(&input[start..end]);
                        start = end;
                    }
                    start = end + 1;
                }
                _ => {}
            }
        }

        if start < input.len() {
            subs.push(&input[start..]);
        }

        subs
    }
}

pub struct BotData {
    pub funboy: Arc<Funboy>,
    pub interpreter: Arc<Mutex<FslInterpreter>>,
    pub ollama_settings: Arc<Mutex<OllamaSettings>>,
}

pub async fn interpret_bot_commands(
    bot_data: &BotData,
    input: &str,
) -> Result<CommandResult, Error> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ParseError::EmptyInput.into());
    };

    let args: Vec<&str> = input.split_whitespace().collect();

    let mut full_args = vec!["funboy"];
    full_args.extend(&args);

    let funboy = &bot_data.funboy;
    let interpreter = &bot_data.interpreter;
    let ollama_settings = &bot_data.ollama_settings;

    match Command::try_parse_from(full_args) {
        Ok(command) => match command {
            Command::Generate {
                input,
                file,
                ollama,
            } => {
                let input = input.join(" ");
                let input = if file {
                    let input = match std::fs::read_to_string(input) {
                        Ok(input) => input,
                        Err(e) => {
                            return Err(Error::CommandError(CommandError::ExecutionFailed(
                                e.to_string(),
                            )));
                        }
                    };
                    input
                } else {
                    input
                };
                let result = if ollama {
                    let ollama_settings = ollama_settings.lock().await;
                    funboy
                        .generate_ollama(
                            funboy.get_ollama_model().await,
                            &ollama_settings,
                            &input,
                            interpreter.clone(),
                        )
                        .await
                        .map(|o| o.response)
                } else {
                    funboy.generate(&input, interpreter.clone()).await
                };
                match result {
                    Ok(output) => return Ok(CommandResult::Text(output)),
                    Err(e) => {
                        return Err(CommandError::ExecutionFailed(e.to_string()).into());
                    }
                };
            }
            Command::Context { context } => return Ok(CommandResult::ContextSwitch(context)),
            Command::Add {
                template,
                substitutes,
                single,
            } => {
                let substitutes = substitutes.join(" ");
                let substitutes: Vec<&str> = parse_substitutes(&substitutes, single);
                if substitutes.len() > 0 {
                    let result = funboy.add_substitutes(&template, &substitutes).await;
                    match result {
                        Ok(receipt) => {
                            let output = format!(
                                "added: {}\nignored: {}",
                                receipt.updated_to_string(),
                                receipt.ignored_to_string()
                            );
                            return Ok(CommandResult::Text(output));
                        }
                        Err(e) => {
                            return Err(CommandError::ExecutionFailed(e.to_string()).into());
                        }
                    }
                } else {
                    let result = funboy.add_substitutes(&template, &vec![]).await;
                    match result {
                        Ok(_) => {
                            let output = format!("created {}", template);
                            return Ok(CommandResult::Text(output));
                        }
                        Err(e) => {
                            return Err(CommandError::ExecutionFailed(e.to_string()).into());
                        }
                    }
                }
            }
            Command::Delete {
                template,
                substitutes,
                single,
            } => {
                let substitutes = substitutes.join(" ");
                let substitutes: Vec<&str> = parse_substitutes(&substitutes, single);

                if substitutes.len() > 0 {
                    let result = funboy.delete_substitutes(&template, &substitutes).await;
                    match result {
                        Ok(receipt) => {
                            let output = format!(
                                "removed: {}\nignored: {}",
                                receipt.updated_to_string(),
                                receipt.ignored_to_string()
                            );
                            return Ok(CommandResult::Text(output));
                        }
                        Err(e) => {
                            return Err(CommandError::ExecutionFailed(e.to_string()).into());
                        }
                    }
                } else {
                    let result = funboy.delete_template(&template).await;
                    match result {
                        Ok(deleted_template) => {
                            let output = if deleted_template.is_some() {
                                format!("deleted {}", template)
                            } else {
                                format!("{} was not present in database", template)
                            };
                            return Ok(CommandResult::Text(output));
                        }
                        Err(e) => {
                            return Err(CommandError::ExecutionFailed(e.to_string()).into());
                        }
                    }
                }
            }
            Command::List {
                template,
                search_term,
                list_style,
            } => match template {
                Some(template) => {
                    let subs = funboy
                        .get_substitutes(
                            &template,
                            search_term.as_deref(),
                            OrderBy::Default,
                            Limit::None,
                        )
                        .await;
                    match subs {
                        Ok(subs) => match list_style {
                            ListStyle::Default => {
                                let subs: Vec<String> =
                                    subs.iter().map(|s| s.name.to_string()).collect();
                                return Ok(CommandResult::Text(subs.join(" ")));
                            }
                            ListStyle::Id => {
                                let subs: Vec<String> =
                                    subs.iter().map(|s| s.id.to_string()).collect();
                                return Ok(CommandResult::Text(subs.join(" ")));
                            }
                        },
                        Err(e) => {
                            return Err(CommandError::ExecutionFailed(e.to_string()).into());
                        }
                    }
                }
                None => {
                    let subs = funboy
                        .get_templates(search_term.as_deref(), OrderBy::Default, Limit::None)
                        .await;
                    match subs {
                        Ok(subs) => {
                            let subs: Vec<String> =
                                subs.iter().map(|s| s.name.to_string()).collect();
                            return Ok(CommandResult::Text(subs.join(" ")));
                        }
                        Err(e) => {
                            return Err(CommandError::ExecutionFailed(e.to_string()).into());
                        }
                    }
                }
            },
            Command::Ollama { action } => match action {
                OllamaAction::List { option } => match option {
                    OllamaListOption::Model => {
                        let model = funboy.get_ollama_model().await;
                        match model {
                            Some(model) => return Ok(CommandResult::Text(model)),
                            None => {
                                return Ok(CommandResult::Text(
                                    "No model currently set".to_string(),
                                ));
                            }
                        }
                    }
                    OllamaListOption::Models => {
                        let models = funboy.get_ollama_models().await;
                        match models {
                            Ok(models) => return Ok(CommandResult::Text(models.join("\n"))),
                            Err(e) => {
                                return Err(CommandError::ExecutionFailed(e.to_string()).into());
                            }
                        }
                    }
                    OllamaListOption::Settings => {
                        let ollama_settings = ollama_settings.lock().await;
                        let settings_string = ollama_settings.to_string();
                        return Ok(CommandResult::Text(settings_string));
                    }
                },
                OllamaAction::Set { option } => match option {
                    OllamaSetOptions::SystemPrompt { system_prompt } => {
                        let mut ollama_settings = ollama_settings.lock().await;
                        let system_prompt = system_prompt.join(" ");
                        ollama_settings.set_system_prompt(&system_prompt);
                        drop(ollama_settings);
                        Ok(CommandResult::Text(format!(
                            "Set ollama system prompt to {}",
                            system_prompt
                        )))
                    }
                    OllamaSetOptions::Model { model } => {
                        funboy.set_ollama_model(Some(model.to_string())).await;
                        return Ok(CommandResult::Text(format!("Set model to {}", model)));
                    }
                    OllamaSetOptions::Template { template } => {
                        let mut ollama_settings = ollama_settings.lock().await;
                        ollama_settings.set_template(&template);
                        drop(ollama_settings);
                        Ok(CommandResult::Text(format!(
                            "Set ollama template to {}",
                            template
                        )))
                    }
                    OllamaSetOptions::OutputLimit { limit } => {
                        let mut ollama_settings = ollama_settings.lock().await;
                        ollama_settings.set_output_limit(limit);
                        drop(ollama_settings);
                        Ok(CommandResult::Text(format!(
                            "Set ollama output limit to {}",
                            limit
                        )))
                    }
                    OllamaSetOptions::Temperature { temperature } => {
                        let mut ollama_settings = ollama_settings.lock().await;
                        ollama_settings.set_temperature(temperature);
                        drop(ollama_settings);
                        Ok(CommandResult::Text(format!(
                            "Set ollama temperature limit to {}",
                            temperature
                        )))
                    }
                    OllamaSetOptions::TopK { top_k } => {
                        let mut ollama_settings = ollama_settings.lock().await;
                        ollama_settings.set_top_k(top_k);
                        drop(ollama_settings);
                        Ok(CommandResult::Text(format!(
                            "Set ollama top_k to {}",
                            top_k
                        )))
                    }
                    OllamaSetOptions::TopP { top_p } => {
                        let mut ollama_settings = ollama_settings.lock().await;
                        ollama_settings.set_top_p(top_p);
                        drop(ollama_settings);
                        Ok(CommandResult::Text(format!(
                            "Set ollama top_p to {}",
                            top_p
                        )))
                    }
                    OllamaSetOptions::RepeatPenalty { repeat_penalty } => {
                        let mut ollama_settings = ollama_settings.lock().await;
                        ollama_settings.set_repeat_penalty(repeat_penalty);
                        drop(ollama_settings);
                        Ok(CommandResult::Text(format!(
                            "Set ollama repeat penalty to {}",
                            repeat_penalty
                        )))
                    }
                },
            },
            Command::Copy {
                from_template,
                to_template,
            } => {
                let result = funboy.copy_substitutes(&from_template, &to_template).await;
                match result {
                    Ok(receipt) => {
                        let output = format!(
                            "{}\ncopied from template {} to {}",
                            receipt
                                .iter()
                                .map(|s| s.name.clone())
                                .collect::<Vec<String>>()
                                .join(" "),
                            from_template,
                            to_template,
                        );
                        return Ok(CommandResult::Text(output));
                    }
                    Err(e) => return Err(CommandError::ExecutionFailed(e.to_string()).into()),
                }
            }
            Command::Rename {
                from_template,
                to_template,
            } => {
                let result = funboy.rename_template(&from_template, &to_template).await;
                match result {
                    Ok(receipt) => match receipt {
                        Some(_) => {
                            let output = format!("renamed {} to {}", from_template, to_template);
                            return Ok(CommandResult::Text(output));
                        }
                        None => {
                            let output =
                                format!("no template named {} in database", from_template,);
                            return Ok(CommandResult::Text(output));
                        }
                    },
                    Err(e) => return Err(CommandError::ExecutionFailed(e.to_string()).into()),
                }
            }
            Command::Replace {
                template,
                substitute,
                with_substitute,
                id,
            } => {
                if id {
                    match substitute.parse::<i64>() {
                        Ok(id) => {
                            let result =
                                funboy.replace_substitute_by_id(id, &with_substitute).await;
                            match result {
                                Ok(sub) => match sub {
                                    Some(_) => {
                                        let output = format!(
                                            "replaced substitute with id \n{}\nwith \n{}",
                                            id, with_substitute
                                        );
                                        return Ok(CommandResult::Text(output));
                                    }
                                    None => {
                                        let output =
                                            format!("no substitute with id {} in database", id);
                                        return Ok(CommandResult::Text(output));
                                    }
                                },
                                Err(e) => {
                                    return Err(CommandError::ExecutionFailed(e.to_string()).into());
                                }
                            }
                        }
                        Err(e) => return Err(CommandError::ExecutionFailed(e.to_string()).into()),
                    }
                } else {
                    if let Some(template) = template {
                        let result = funboy
                            .replace_substitute(&template, &substitute, &with_substitute)
                            .await;
                        match result {
                            Ok(sub) => match sub {
                                Some(_) => {
                                    let output = format!(
                                        "replaced substitute \n{}\nwith \n{}",
                                        substitute, with_substitute
                                    );
                                    return Ok(CommandResult::Text(output));
                                }
                                None => {
                                    let output = format!("no substitute \n{}\nin database", id);
                                    return Ok(CommandResult::Text(output));
                                }
                            },
                            Err(e) => {
                                return Err(CommandError::ExecutionFailed(e.to_string()).into());
                            }
                        }
                    } else {
                        return Err(CommandError::ExecutionFailed(format!(
                            "must include template name when replacing substitute by name"
                        ))
                        .into());
                    }
                }
            }
            Command::Exit => Ok(CommandResult::Exit),
        },
        Err(e) => Err(Error::ParseError(ParseError::UnknownCommand(e.to_string()))),
    }

    /*
    match command {
        Command::Help => todo!(),
        Command::ResetOllamaTemplate => todo!(),
        Command::Unknown => return Err(ParseError::UnknownCommand(command_str.to_string()).into()),
    }
    */
}
