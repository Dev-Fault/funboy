use std::{str::FromStr, sync::Arc, time::Duration};

use clap::Parser;
use dotenvy::dotenv;
use fsl_interpreter::FslInterpreter;
use funboy_core::{
    Funboy, FunboyError, UserId,
    format::{
        AsStrs, LIST_STYLE_NONE, ListStyle, ONE_HUNDRED, SeperatedListOptions, TruncateEllipsize,
        format_as_item_seperated_list, format_item_list, parse_bot_args,
    },
    template_database::{Limit, OrderBy, SortOrder, SubstituteReceipt, TemplateDatabase},
};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Mutex;

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
    UnhandledCommand(Command),
}

impl ToString for CommandError {
    fn to_string(&self) -> String {
        match self {
            CommandError::ExecutionFailed(error_text) => error_text.clone(),
            CommandError::LackingPermission(permission) => {
                format!("User lacks {} permission", permission.to_string())
            }
            CommandError::UnknownCommand(e) => e.clone(),
            CommandError::UnhandledCommand(command) => {
                format!("{:?} command not available in this context", command)
            }
        }
    }
}

const GENERATE: &str = "generate";
const FSL: &str = "fsl";

#[derive(Debug, Copy, Clone)]
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

pub enum CommandResult {
    Text(String),
    Mode(Mode),
    None,
    Exit,
}

const MODEL: &str = "model";
const MODELS: &str = "models";
const SETTINGS: &str = "settings";

#[derive(Parser, Debug, Copy, Clone)]
pub enum OllamaListOption {
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
pub enum OllamaSetOptions {
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

#[derive(Parser, Debug, Clone)]
pub enum OllamaAction {
    List {
        #[arg(value_parser = clap::value_parser!(OllamaListOption))]
        option: OllamaListOption,
    },

    Set {
        #[command(subcommand)]
        option: OllamaSetOptions,
    },
}

#[derive(Parser, Debug, Clone)]
pub enum ImageAction {
    Embed { url: String },
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
    Context {
        #[arg(value_parser = clap::value_parser!(Mode))]
        context: Mode,
    },
    Image {
        #[command(subcommand)]
        action: ImageAction,
    },
    Exit,
}

fn parse_substitutes<'a>(input: &'a str, single: bool) -> Result<Vec<&'a str>, CommandError> {
    if single {
        return Ok(vec![input]);
    } else {
        parse_bot_args(input).map_err(|e| CommandError::ExecutionFailed(e.to_string()))
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Context {
    Cli,
    Matrix,
}

pub async fn interpret_bot_commands<U: UserId>(
    user_id: U,
    funboy: &Funboy<U>,
    interpreter: Arc<Mutex<FslInterpreter>>,
    permissions: &Permissions,
    context: Context,
    input: &str,
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
            Command::Generate { file: true, .. } if context == Context::Matrix => {
                return Err(CommandError::UnhandledCommand(command));
            }
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
            Command::Context { context } => return Ok(CommandResult::Mode(context)),
            Command::Add { file: true, .. } if context == Context::Matrix => {
                return Err(CommandError::UnhandledCommand(command));
            }
            Command::Add {
                template,
                substitutes,
                single,
                file,
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
            Command::Image { .. } => Err(CommandError::UnhandledCommand(command)),
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
                                "Replaced substitute with id \n{}\nwith \n{}",
                                id,
                                with_substitute.truncate_with_ellipse(ONE_HUNDRED)
                            );
                            return Ok(CommandResult::Text(output));
                        }
                        None => {
                            let output = format!("No substitute with id {}", id);
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
                            "Replaced substitute \n{}\nwith \n{}",
                            substitute.truncate_with_ellipse(ONE_HUNDRED),
                            with_substitute.truncate_with_ellipse(ONE_HUNDRED)
                        );
                        return Ok(CommandResult::Text(output));
                    }
                    None => {
                        let output = format!(
                            "No substitute \n{}",
                            substitute.truncate_with_ellipse(ONE_HUNDRED)
                        );
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
                let output = format!(
                    "Renamed template `{}` to template `{}`",
                    from_template.truncate_with_ellipse(ONE_HUNDRED),
                    to_template.truncate_with_ellipse(ONE_HUNDRED)
                );
                return Ok(CommandResult::Text(output));
            }
            None => {
                let output = format!(
                    "No template named `{}` in database",
                    from_template.truncate_with_ellipse(ONE_HUNDRED)
                );
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
                "`{}\nCopied substitutes from template `{}` to template `{}`",
                receipt
                    .iter()
                    .map(|s| s.name.clone())
                    .collect::<Vec<String>>()
                    .join(" "),
                from_template.truncate_with_ellipse(ONE_HUNDRED),
                to_template.truncate_with_ellipse(ONE_HUNDRED),
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
                    OrderBy::NameIgnoreCase(SortOrder::Ascending),
                    Limit::Count(1000),
                )
                .await;
            match subs {
                Ok(subs) => {
                    let subs: Vec<String> = format_item_list(subs, list_style, None);
                    Ok(CommandResult::Text(subs.join(" ")))
                }
                Err(e) => {
                    return Err(CommandError::ExecutionFailed(e.to_string()).into());
                }
            }
        }
        None => {
            let templates = funboy
                .get_templates(
                    search_term.as_deref(),
                    OrderBy::NameIgnoreCase(SortOrder::Ascending),
                    Limit::Count(1000),
                )
                .await;
            match templates {
                Ok(templates) => {
                    let templates: Vec<String> = format_item_list(templates, list_style, None);
                    return Ok(CommandResult::Text(templates.join(" ")));
                }
                Err(e) => {
                    return Err(CommandError::ExecutionFailed(e.to_string()).into());
                }
            }
        }
    }
}

fn sub_receipt_to_string(
    receipt: SubstituteReceipt,
    updated_caption: &str,
    ignored_caption: &str,
) -> String {
    let added: Vec<String> = if receipt.updated.len() == 0 {
        vec![format!("")]
    } else {
        let caption = format!("\n{}", updated_caption,);
        format_item_list(receipt.updated, ListStyle::None, Some(&caption))
    };
    let ignored: Vec<String> = if receipt.ignored.len() == 0 {
        vec![format!("")]
    } else {
        let caption = format!("\n{}", ignored_caption,);
        format_as_item_seperated_list(
            &receipt.ignored.as_strs(),
            &caption,
            SeperatedListOptions::space_seperated(),
        )
    };

    format!("{}\n{}", added.join("\n"), ignored.join("\n"))
}

async fn delete<U: UserId>(
    funboy: &Funboy<U>,
    template: String,
    substitutes: Vec<String>,
    single: bool,
) -> Result<CommandResult, CommandError> {
    let substitutes = substitutes.join(" ");
    let substitutes: Vec<&str> = parse_substitutes(&substitutes, single)?;
    if substitutes.len() > 0 {
        let result = funboy.delete_substitutes(&template, &substitutes).await;
        match result {
            Ok(receipt) => {
                return Ok(CommandResult::Text(sub_receipt_to_string(
                    receipt,
                    &format!(
                        "deleted from `{}`",
                        &template.truncate_with_ellipse(ONE_HUNDRED)
                    ),
                    &format!("not in `{}`", &template).truncate_with_ellipse(ONE_HUNDRED),
                )));
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
                    format!(
                        "Deleted template `{}`",
                        template.truncate_with_ellipse(ONE_HUNDRED)
                    )
                } else {
                    format!(
                        "Template `{}` was not present in database",
                        template.truncate_with_ellipse(ONE_HUNDRED)
                    )
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
    let substitutes: Vec<&str> = parse_substitutes(&substitutes, single)?;
    if substitutes.len() > 0 {
        let result = funboy.add_substitutes(&template, &substitutes).await;
        match result {
            Ok(receipt) => {
                return Ok(CommandResult::Text(sub_receipt_to_string(
                    receipt,
                    &format!(
                        "added to `{}`",
                        &template.truncate_with_ellipse(ONE_HUNDRED)
                    ),
                    &format!(
                        "already in `{}`",
                        &template.truncate_with_ellipse(ONE_HUNDRED)
                    ),
                )));
            }
            Err(e) => {
                return Err(CommandError::ExecutionFailed(e.to_string()).into());
            }
        }
    } else {
        let result = funboy.add_substitutes(&template, &vec![]).await;
        match result {
            Ok(_) => {
                let output = format!("Created {}", template.truncate_with_ellipse(ONE_HUNDRED));
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
