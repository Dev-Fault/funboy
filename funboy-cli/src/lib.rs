use std::{
    str::FromStr,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use clap::Parser;
use dotenvy::dotenv;
use fsl_interpreter::{
    FslInterpreter,
    commands::{NUMERIC_TYPES, TEXT_TYPES},
    types::command::{ArgPos, ArgRule},
};
use funboy_core::{
    Funboy, UserId,
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

pub async fn get_funboy<U: UserId>(env: &FunboyEnv) -> Funboy<U> {
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

#[derive(Debug, Clone)]
pub enum CommandError {
    ExecutionFailed(String),
    LackingPermission(Permission),
    UnknownCommand(String),
}

impl ToString for CommandError {
    fn to_string(&self) -> String {
        match self {
            CommandError::ExecutionFailed(error_text) => error_text.clone(),
            CommandError::LackingPermission(permission) => {
                format!("User lacks {} permission", permission.to_string())
            }
            CommandError::UnknownCommand(e) => e.clone(),
        }
    }
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
    None,
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Permission {
    FileAccess,
    Add,
    Modify,
    OllamaUsage,
}

impl ToString for Permission {
    fn to_string(&self) -> String {
        match self {
            Permission::FileAccess => "file access",
            Permission::Add => "add",
            Permission::Modify => "modify",
            Permission::OllamaUsage => "ollama usage",
        }
        .to_string()
    }
}

pub struct Permissions(Vec<Permission>);

impl Permissions {
    pub fn all() -> Self {
        Permissions(vec![
            Permission::FileAccess,
            Permission::Add,
            Permission::Modify,
            Permission::OllamaUsage,
        ])
    }

    pub fn power_user() -> Self {
        Permissions(vec![
            Permission::Add,
            Permission::Modify,
            Permission::OllamaUsage,
        ])
    }

    pub fn user() -> Self {
        Permissions(vec![Permission::Add, Permission::OllamaUsage])
    }

    pub fn can_access_files(&self) -> bool {
        self.0.contains(&Permission::FileAccess)
    }

    pub fn can_add(&self) -> bool {
        self.0.contains(&Permission::Add)
    }

    pub fn can_modify(&self) -> bool {
        self.0.contains(&Permission::Modify)
    }

    pub fn can_use_ollama(&self) -> bool {
        self.0.contains(&Permission::OllamaUsage)
    }
}

#[derive(Clone)]
pub struct FslContext<U: UserId> {
    pub funboy: Arc<Funboy<U>>,
    pub interpreter: Arc<Mutex<FslInterpreter>>,
}

impl<U: UserId> FslContext<U> {
    pub fn new(funboy: Arc<Funboy<U>>) -> Self {
        Self {
            funboy: funboy,
            interpreter: Arc::new(Mutex::new(FslInterpreter::new())),
        }
    }

    pub async fn generate_message(
        &self,
        message: &str,
    ) -> Result<String, fsl_interpreter::types::command::CommandError> {
        match self
            .funboy
            .generate(&message, self.interpreter.clone())
            .await
        {
            Ok(gen_msg) => Ok(gen_msg),
            Err(e) => {
                return Err(fsl_interpreter::types::command::CommandError::Custom(
                    e.to_string(),
                ));
            }
        }
    }
}

pub async fn interpret_bot_commands<U: UserId>(
    funboy: &Funboy<U>,
    interpreter: Arc<Mutex<FslInterpreter>>,
    permissions: &Permissions,
    input: &str,
    user_id: U,
) -> Result<CommandResult, CommandError> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(CommandResult::None);
    };

    let args: Vec<&str> = input.split_whitespace().collect();

    let mut full_args = vec!["funboy"];
    full_args.extend(&args);

    match Command::try_parse_from(full_args) {
        Ok(command) => match command {
            Command::Generate {
                input,
                file,
                ollama,
            } => {
                if file && !permissions.can_access_files() {
                    return Err(CommandError::LackingPermission(Permission::FileAccess).into());
                } else if ollama && !permissions.can_use_ollama() {
                    return Err(CommandError::LackingPermission(Permission::OllamaUsage).into());
                }
                generate(funboy, user_id, interpreter, input, file, ollama).await
            }
            Command::Context { context } => return Ok(CommandResult::ContextSwitch(context)),
            Command::Add {
                template,
                substitutes,
                single,
            } => {
                if !permissions.can_add() {
                    return Err(CommandError::LackingPermission(Permission::Add).into());
                }
                add(funboy, template, substitutes, single).await
            }
            Command::Delete {
                template,
                substitutes,
                single,
            } => {
                if !permissions.can_modify() {
                    return Err(CommandError::LackingPermission(Permission::Modify).into());
                }
                delete(funboy, template, substitutes, single).await
            }
            Command::List {
                template,
                search_term,
                list_style,
            } => list(funboy, template, search_term, list_style).await,
            Command::Ollama { action } => {
                if !permissions.can_use_ollama() {
                    return Err(CommandError::LackingPermission(Permission::OllamaUsage).into());
                }
                ollama(funboy, user_id, action).await
            }
            Command::Copy {
                from_template,
                to_template,
            } => {
                if !permissions.can_modify() {
                    return Err(CommandError::LackingPermission(Permission::Modify).into());
                }
                copy(funboy, from_template, to_template).await
            }
            Command::Rename {
                from_template,
                to_template,
            } => {
                if !permissions.can_modify() {
                    return Err(CommandError::LackingPermission(Permission::Modify).into());
                }
                rename(funboy, from_template, to_template).await
            }
            Command::Replace {
                template,
                substitute,
                with_substitute,
                id,
            } => {
                if !permissions.can_modify() {
                    return Err(CommandError::LackingPermission(Permission::Modify).into());
                }
                replace(funboy, template, substitute, with_substitute, id).await
            }
            Command::Exit => Ok(CommandResult::Exit),
        },
        Err(e) => Err(CommandError::UnknownCommand(e.to_string())),
    }
}

async fn replace<U: UserId>(
    funboy: &Funboy<U>,
    template: Option<String>,
    substitute: String,
    with_substitute: String,
    id: bool,
) -> Result<CommandResult, CommandError> {
    if id {
        match substitute.parse::<i64>() {
            Ok(id) => {
                let result = funboy.replace_substitute_by_id(id, &with_substitute).await;
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
                            let output = format!("no substitute with id {} in database", id);
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

async fn rename<U: UserId>(
    funboy: &Funboy<U>,
    from_template: String,
    to_template: String,
) -> Result<CommandResult, CommandError> {
    let result = funboy.rename_template(&from_template, &to_template).await;
    match result {
        Ok(receipt) => match receipt {
            Some(_) => {
                let output = format!("renamed {} to {}", from_template, to_template);
                return Ok(CommandResult::Text(output));
            }
            None => {
                let output = format!("no template named {} in database", from_template,);
                return Ok(CommandResult::Text(output));
            }
        },
        Err(e) => return Err(CommandError::ExecutionFailed(e.to_string()).into()),
    }
}

async fn copy<U: UserId>(
    funboy: &Funboy<U>,
    from_template: String,
    to_template: String,
) -> Result<CommandResult, CommandError> {
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

async fn ollama<U: UserId>(
    funboy: &Funboy<U>,
    user_id: U,
    action: OllamaAction,
) -> Result<CommandResult, CommandError> {
    let ollama_settings = funboy.get_user_ctx(user_id).await.ollama_settings;
    match action {
        OllamaAction::List { option } => match option {
            OllamaListOption::Model => {
                let model = funboy.get_ollama_model().await;
                match model {
                    Some(model) => return Ok(CommandResult::Text(model)),
                    None => {
                        return Ok(CommandResult::Text("No model currently set".to_string()));
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
    }
}

async fn list<U: UserId>(
    funboy: &Funboy<U>,
    template: Option<String>,
    search_term: Option<String>,
    list_style: ListStyle,
) -> Result<CommandResult, CommandError> {
    match template {
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
                        let subs: Vec<String> = subs.iter().map(|s| s.name.to_string()).collect();
                        return Ok(CommandResult::Text(subs.join(" ")));
                    }
                    ListStyle::Id => {
                        let subs: Vec<String> = subs.iter().map(|s| s.id.to_string()).collect();
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
                    let subs: Vec<String> = subs.iter().map(|s| s.name.to_string()).collect();
                    return Ok(CommandResult::Text(subs.join(" ")));
                }
                Err(e) => {
                    return Err(CommandError::ExecutionFailed(e.to_string()).into());
                }
            }
        }
    }
}

async fn delete<U: UserId>(
    funboy: &Funboy<U>,
    template: String,
    substitutes: Vec<String>,
    single: bool,
) -> Result<CommandResult, CommandError> {
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

async fn add<U: UserId>(
    funboy: &Funboy<U>,
    template: String,
    substitutes: Vec<String>,
    single: bool,
) -> Result<CommandResult, CommandError> {
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

async fn generate<U: UserId>(
    funboy: &Funboy<U>,
    user_id: U,
    interpreter: Arc<Mutex<FslInterpreter>>,
    input: Vec<String>,
    file: bool,
    ollama: bool,
) -> Result<CommandResult, CommandError> {
    let input = input.join(" ");
    let input = if file {
        let input = match std::fs::read_to_string(input) {
            Ok(input) => input,
            Err(e) => {
                return Err(CommandError::ExecutionFailed(e.to_string()));
            }
        };
        input
    } else {
        input
    };
    let result = if ollama {
        funboy
            .user_generate_ollama(user_id, &input, interpreter.clone())
            .await
            .map(|o| format!("{}{}", o.prompt, o.generated_text))
    } else {
        funboy
            .user_generate(user_id, &input, interpreter.clone())
            .await
    };
    match result {
        Ok(output) => return Ok(CommandResult::Text(output)),
        Err(e) => {
            return Err(CommandError::ExecutionFailed(e.to_string()).into());
        }
    };
}
