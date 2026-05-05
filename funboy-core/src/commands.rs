use std::{num::ParseIntError, sync::Arc};

use clap::{Args, Parser, ValueEnum};
use fsl_core::FslInterpreter;
use strum::EnumString;
use tokio::sync::Mutex;

use crate::{
    Funboy, FunboyError, Permission, Permissions, Role,
    database::{KeySize, Limit, OrderBy, Platform, SortOrder, SubstituteReceipt},
    format::{
        AsStrs, LIST_STYLE_NONE, ListStyle, ONE_HUNDRED, SeperatedListOptions, TruncateEllipsize,
        format_as_item_seperated_list, format_item_list, parse_bot_args,
    },
    user::FunboyUserId,
};

pub enum CommandResult {
    Text(String),
    None,
}

impl Into<Option<String>> for CommandResult {
    fn into(self) -> Option<String> {
        match self {
            CommandResult::Text(text) => Some(text),
            CommandResult::None => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum CommandError {
    ExecutionFailed(String),
    LackingPermission(Permission),
    LackingPermissions(Permissions),
    UnknownCommand(String),
}

impl From<FunboyError> for CommandError {
    fn from(value: FunboyError) -> Self {
        CommandError::ExecutionFailed(value.to_string())
    }
}

impl From<String> for CommandError {
    fn from(value: String) -> Self {
        CommandError::ExecutionFailed(value)
    }
}

#[derive(Parser, Debug, Clone)]
pub enum OllamaAction {
    List {
        #[command(subcommand)]
        option: OllamaListOption,
    },

    Set {
        #[command(subcommand)]
        option: OllamaSetOption,
    },

    Toggle {
        #[command(subcommand)]
        tool: OllamaToggleTool,
    },

    Reset {
        #[command(subcommand)]
        option: OllamaResetOption,
    },

    Clear,
}

#[derive(
    Parser, Debug, Copy, Clone, PartialEq, Eq, Hash, ValueEnum, strum_macros::Display, EnumString,
)]
pub enum OllamaToggleTool {
    WebSearch,
}

impl OllamaToggleTool {
    pub fn as_str(&self) -> &'static str {
        match self {
            OllamaToggleTool::WebSearch => "web search",
        }
    }
}

#[derive(Parser, Debug, Clone)]
pub enum OllamaSetOption {
    Model {
        model: String,
    },
    SystemPrompt {
        #[arg(trailing_var_arg = true)]
        system_prompt: Vec<String>,
    },
    Template {
        #[arg(trailing_var_arg = true)]
        template: Vec<String>,
    },
    OutputLimit {
        limit: u16,
    },
    Temperature {
        temperature: f32,
    },
    TopK {
        top_k: u32,
    },
    TopP {
        top_p: f32,
    },
    RepeatPenalty {
        repeat_penalty: f32,
    },
}

#[derive(
    Parser, Debug, Copy, Clone, PartialEq, Eq, Hash, ValueEnum, strum_macros::Display, EnumString,
)]
pub enum OllamaResetOption {
    #[strum(to_string = "System Prompt")]
    SystemPrompt,
    #[strum(to_string = "Template")]
    Template,
    #[strum(to_string = "Output Limit")]
    OutputLimit,
    #[strum(to_string = "Parameters")]
    Parameters,
}

impl OllamaResetOption {
    pub fn as_str(&self) -> &'static str {
        match self {
            OllamaResetOption::SystemPrompt => "system-prompt",
            OllamaResetOption::Template => "template",
            OllamaResetOption::OutputLimit => "output-limit",
            OllamaResetOption::Parameters => "parameters",
        }
    }
}

#[derive(
    Parser, Debug, Copy, Clone, PartialEq, Eq, Hash, ValueEnum, strum_macros::Display, EnumString,
)]
pub enum OllamaListOption {
    Model,
    Models,
    Settings,
}

impl OllamaListOption {
    pub fn as_str(&self) -> &'static str {
        match self {
            OllamaListOption::Model => "model",
            OllamaListOption::Models => "models",
            OllamaListOption::Settings => "settings",
        }
    }
}

impl ToString for CommandError {
    fn to_string(&self) -> String {
        match self {
            CommandError::ExecutionFailed(error_text) => error_text.clone(),
            CommandError::LackingPermission(permission) => {
                format!("User lacks {} permission", permission.to_string())
            }
            CommandError::LackingPermissions(permissions) => {
                format!("User lacks [{}] permissions", permissions.to_string(),)
            }
            CommandError::UnknownCommand(e) => e.clone(),
        }
    }
}

pub fn parse_command_args<'a>(input: &'a str) -> Vec<&'a str> {
    let input = input.trim();
    let args: Vec<&str> = input.split(' ').collect();

    let mut full_args = vec!["funboy"];
    full_args.extend(&args);

    full_args
}

#[derive(Args, Clone, Debug)]
pub struct GenerateArgs {
    #[arg(short, long)]
    pub file: bool,

    #[arg(short, long)]
    pub ollama: bool,

    #[arg(trailing_var_arg = true)]
    pub input: Vec<String>,
}

#[derive(Args, Clone, Debug)]
pub struct FslArgs {
    #[arg(trailing_var_arg = true)]
    pub input: Vec<String>,
}

#[derive(Args, Clone, Debug)]
pub struct AddArgs {
    pub template: String,

    #[arg(short, long)]
    pub single: bool,

    #[arg(short, long)]
    pub file: bool,

    #[arg(trailing_var_arg = true)]
    pub substitutes: Vec<String>,
}

#[derive(Args, Clone, Debug)]
pub struct DeleteArgs {
    pub template: String,

    #[arg(short, long)]
    pub single: bool,

    #[arg(short, long)]
    pub id: bool,

    #[arg(trailing_var_arg = true)]
    pub substitutes: Vec<String>,
}

#[derive(Args, Clone, Debug)]
pub struct ListArgs {
    pub template: Option<String>,

    #[arg(short, long, default_value = None)]
    pub search_term: Option<String>,

    #[arg(short, long, value_parser = clap::value_parser!(ListStyle), default_value = LIST_STYLE_NONE)]
    pub list_style: ListStyle,
}

#[derive(Args, Clone, Debug)]
pub struct CopyArgs {
    pub from_template: String,
    pub to_template: String,
}

#[derive(Args, Clone, Debug)]
pub struct RenameArgs {
    pub from_template: String,
    pub to_template: String,
}

#[derive(Args, Clone, Debug)]
pub struct ReplaceArgs {
    pub substitute: String,
    pub with_substitute: String,

    #[arg(short, long)]
    pub template: Option<String>,

    #[arg(short, long)]
    pub id: bool,
}

#[derive(Args, Clone, Debug)]
pub struct OllamaArgs {
    #[command(subcommand)]
    pub action: OllamaAction,
}

impl<U: FunboyUserId> Funboy<U> {
    pub async fn replace_command(
        &self,
        user_id: U,
        template: Option<String>,
        substitute: String,
        with_substitute: String,
        id: bool,
    ) -> Result<CommandResult, CommandError> {
        let permissions = self.users.get_permissions(user_id.clone()).await?;
        if !permissions.can_update() {
            return Err(CommandError::LackingPermission(Permission::Update).into());
        }

        if id {
            match substitute.parse::<i64>() {
                Ok(id) => {
                    let result = self.replace_substitute_by_id(id, &with_substitute).await;
                    match result {
                        Ok(sub) => match sub {
                            Some(_) => {
                                let output = format!(
                                    "Replaced substitute with id `{}` with `{}`",
                                    id,
                                    with_substitute.truncate_with_ellipse(ONE_HUNDRED)
                                );
                                return Ok(CommandResult::Text(output));
                            }
                            None => {
                                let output = format!("No substitute with id `{}`", id);
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
                let result = self
                    .replace_substitute(&template, &substitute, &with_substitute)
                    .await;
                match result {
                    Ok(sub) => match sub {
                        Some(_) => {
                            let output = format!(
                                "Replaced substitute `{}` with `{}`",
                                substitute.truncate_with_ellipse(ONE_HUNDRED),
                                with_substitute.truncate_with_ellipse(ONE_HUNDRED)
                            );
                            return Ok(CommandResult::Text(output));
                        }
                        None => {
                            let output = format!(
                                "No substitute `{}`",
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
                    "Must include template name when replacing substitute by name"
                ))
                .into());
            }
        }
    }

    pub async fn rename_command(
        &self,
        user_id: U,
        from_template: String,
        to_template: String,
    ) -> Result<CommandResult, CommandError> {
        let permissions = self.users.get_permissions(user_id.clone()).await?;
        if !permissions.can_update() {
            return Err(CommandError::LackingPermission(Permission::Update).into());
        }

        let result = self.rename_template(&from_template, &to_template).await;
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
                        "No template named `{}`",
                        from_template.truncate_with_ellipse(ONE_HUNDRED)
                    );
                    return Ok(CommandResult::Text(output));
                }
            },
            Err(e) => return Err(CommandError::ExecutionFailed(e.to_string()).into()),
        }
    }

    pub async fn copy_command(
        &self,
        user_id: U,
        from_template: String,
        to_template: String,
    ) -> Result<CommandResult, CommandError> {
        let permissions = self.users.get_permissions(user_id.clone()).await?;
        if !permissions.can_update() {
            return Err(CommandError::LackingPermission(Permission::Update).into());
        }

        let result = self.copy_substitutes(&from_template, &to_template).await;
        match result {
            Ok(_) => {
                let output = format!(
                    "Copied substitutes from template `{}` to template `{}`",
                    from_template.truncate_with_ellipse(ONE_HUNDRED),
                    to_template.truncate_with_ellipse(ONE_HUNDRED),
                );
                return Ok(CommandResult::Text(output));
            }
            Err(e) => return Err(CommandError::ExecutionFailed(e.to_string()).into()),
        }
    }

    pub async fn ollama_command(
        &self,
        user_id: U,
        platform: Platform,
        action: OllamaAction,
    ) -> Result<CommandResult, CommandError> {
        let permissions = self.users.get_permissions(user_id.clone()).await?;
        if !permissions.can_use_ollama() {
            return Err(CommandError::LackingPermission(Permission::Ollama).into());
        }

        let ollama_settings = self
            .users
            .get_or_insert(user_id.clone())
            .await?
            .ollama_settings;
        match action {
            OllamaAction::List { option } => match option {
                OllamaListOption::Model => {
                    let model = self
                        .get_ollama_model()
                        .await
                        .unwrap_or("Model not set".to_string());
                    Ok(CommandResult::Text(model))
                }
                OllamaListOption::Models => {
                    let models = self.get_ollama_models().await;
                    match models {
                        Ok(models) => return Ok(CommandResult::Text(models.join("\n"))),
                        Err(e) => {
                            return Err(CommandError::ExecutionFailed(e.to_string()).into());
                        }
                    }
                }
                OllamaListOption::Settings => {
                    let model = self
                        .get_ollama_model()
                        .await
                        .map(|m| format!("Model: {}", m))
                        .unwrap_or_default();
                    let ollama_settings = ollama_settings.lock().await;
                    let settings_string = ollama_settings.to_string();
                    return Ok(CommandResult::Text(format!(
                        "{}\n{}",
                        model, settings_string
                    )));
                }
            },
            OllamaAction::Set { option } => match option {
                OllamaSetOption::SystemPrompt { system_prompt } => {
                    let mut ollama_settings = ollama_settings.lock().await;
                    let system_prompt = system_prompt.join(" ");
                    ollama_settings.set_system_prompt(&system_prompt);
                    let settings = ollama_settings.clone();
                    drop(ollama_settings);
                    self.users
                        .update_ollama_settings(user_id, platform, settings)
                        .await?;
                    Ok(CommandResult::Text(format!(
                        "Set ollama system prompt to {}",
                        system_prompt
                    )))
                }
                OllamaSetOption::Model { model } => {
                    self.set_ollama_model(Some(model.to_string())).await;
                    return Ok(CommandResult::Text(format!("Set model to {}", model)));
                }
                OllamaSetOption::Template { template } => {
                    let mut ollama_settings = ollama_settings.lock().await;
                    let template = template.join(" ");
                    ollama_settings.set_template(&template);
                    let settings = ollama_settings.clone();
                    drop(ollama_settings);
                    self.users
                        .update_ollama_settings(user_id, platform, settings)
                        .await?;
                    Ok(CommandResult::Text(format!(
                        "Set ollama template to {}",
                        template
                    )))
                }
                OllamaSetOption::OutputLimit { limit } => {
                    let mut ollama_settings = ollama_settings.lock().await;
                    ollama_settings.set_output_limit(limit);
                    let settings = ollama_settings.clone();
                    drop(ollama_settings);
                    self.users
                        .update_ollama_settings(user_id, platform, settings)
                        .await?;
                    Ok(CommandResult::Text(format!(
                        "Set ollama output limit to {}",
                        limit
                    )))
                }
                OllamaSetOption::Temperature { temperature } => {
                    let mut ollama_settings = ollama_settings.lock().await;
                    ollama_settings.set_temperature(temperature);
                    let settings = ollama_settings.clone();
                    drop(ollama_settings);
                    self.users
                        .update_ollama_settings(user_id, platform, settings)
                        .await?;
                    Ok(CommandResult::Text(format!(
                        "Set ollama temperature limit to {}",
                        temperature
                    )))
                }
                OllamaSetOption::TopK { top_k } => {
                    let mut ollama_settings = ollama_settings.lock().await;
                    ollama_settings.set_top_k(top_k);
                    let settings = ollama_settings.clone();
                    drop(ollama_settings);
                    self.users
                        .update_ollama_settings(user_id, platform, settings)
                        .await?;
                    Ok(CommandResult::Text(format!(
                        "Set ollama top_k to {}",
                        top_k
                    )))
                }
                OllamaSetOption::TopP { top_p } => {
                    let mut ollama_settings = ollama_settings.lock().await;
                    ollama_settings.set_top_p(top_p);
                    let settings = ollama_settings.clone();
                    drop(ollama_settings);
                    self.users
                        .update_ollama_settings(user_id, platform, settings)
                        .await?;
                    Ok(CommandResult::Text(format!(
                        "Set ollama top_p to {}",
                        top_p
                    )))
                }
                OllamaSetOption::RepeatPenalty { repeat_penalty } => {
                    let mut ollama_settings = ollama_settings.lock().await;
                    ollama_settings.set_repeat_penalty(repeat_penalty);
                    let settings = ollama_settings.clone();
                    drop(ollama_settings);
                    self.users
                        .update_ollama_settings(user_id, platform, settings)
                        .await?;
                    Ok(CommandResult::Text(format!(
                        "Set ollama repeat penalty to {}",
                        repeat_penalty
                    )))
                }
            },
            OllamaAction::Reset { option } => {
                let mut ollama_settings = ollama_settings.lock().await;
                match option {
                    OllamaResetOption::SystemPrompt => ollama_settings.reset_system_prompt(),
                    OllamaResetOption::Template => ollama_settings.reset_template(),
                    OllamaResetOption::OutputLimit => ollama_settings.reset_output_limit(),
                    OllamaResetOption::Parameters => ollama_settings.reset_parameters(),
                }
                let settings = ollama_settings.clone();
                drop(ollama_settings);
                self.users
                    .update_ollama_settings(user_id, platform, settings)
                    .await?;
                Ok(CommandResult::Text(format!("Reset {option}")))
            }
            OllamaAction::Toggle { tool } => {
                let mut ollama_settings = ollama_settings.lock().await;
                let result = match tool {
                    OllamaToggleTool::WebSearch => {
                        if ollama_settings.tools.toggle_web_search() {
                            "Enabled web search tool"
                        } else {
                            "Disabled web search tool"
                        }
                    }
                };
                let settings = ollama_settings.clone();
                drop(ollama_settings);
                self.users
                    .update_ollama_settings(user_id, platform, settings)
                    .await?;
                Ok(CommandResult::Text(result.into()))
            }
            OllamaAction::Clear => {
                let user_ctx = self.users.get_or_insert(user_id).await?;
                user_ctx.clear_ollama_history();
                Ok(CommandResult::Text("Cleared ollama chat history".into()))
            }
        }
    }

    pub async fn list_command(
        &self,
        template: Option<String>,
        search_term: Option<String>,
        list_style: ListStyle,
    ) -> Result<CommandResult, CommandError> {
        match template {
            Some(template) => {
                let subs = self
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
                let templates = self
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
        context: Platform,
        receipt: SubstituteReceipt,
        updated_caption: &str,
        ignored_caption: &str,
    ) -> String {
        let (list_options, list_style) = match context {
            Platform::Cli => (SeperatedListOptions::space_seperated(), ListStyle::None),
            Platform::Matrix => (
                SeperatedListOptions::default(),
                ListStyle::CommaSeparatedBlocks,
            ),
            Platform::Discord => (
                SeperatedListOptions::default(),
                ListStyle::CommaSeparatedBlocks,
            ),
        };

        let added: Vec<String> = if receipt.updated.len() == 0 {
            vec![format!("")]
        } else {
            let caption = format!("\n{}", updated_caption,);
            format_item_list(receipt.updated, list_style, Some(&caption))
        };

        let ignored: Vec<String> = if receipt.ignored.len() == 0 {
            vec![format!("")]
        } else {
            let caption = format!("\n{}", ignored_caption,);
            format_as_item_seperated_list(&receipt.ignored.as_strs(), &caption, list_options)
        };

        format!("{}\n{}", added.join("\n"), ignored.join("\n"))
    }

    pub async fn delete_command(
        &self,
        user_id: U,
        platform: Platform,
        template: String,
        substitutes: String,
        single: bool,
        id: bool,
    ) -> Result<CommandResult, CommandError> {
        let permissions = self.users.get_permissions(user_id.clone()).await?;
        if !permissions.can_update() {
            return Err(CommandError::LackingPermission(Permission::Update).into());
        }
        let substitutes: Vec<&str> = Self::parse_substitutes(&substitutes, single)?;
        if substitutes.len() > 0 {
            let result = if id {
                let ids: Result<Vec<KeySize>, ParseIntError> =
                    substitutes.iter().map(|s| s.parse::<KeySize>()).collect();
                match ids {
                    Ok(ids) => self.delete_substitutes_by_id(&ids).await,
                    Err(_) => {
                        return Err(CommandError::ExecutionFailed(
                            "Substitute ids must be a valid number".to_string(),
                        ));
                    }
                }
            } else {
                self.delete_substitutes(&template, &substitutes).await
            };

            match result {
                Ok(receipt) => {
                    return Ok(CommandResult::Text(Self::sub_receipt_to_string(
                        platform,
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
            let result = self.delete_template(&template).await;
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

    fn parse_substitutes<'a>(input: &'a str, single: bool) -> Result<Vec<&'a str>, CommandError> {
        if single {
            return Ok(vec![input]);
        } else {
            parse_bot_args(input).map_err(|e| CommandError::ExecutionFailed(e.to_string()))
        }
    }

    pub async fn add_command(
        &self,
        user_id: U,
        platform: Platform,
        template: String,
        substitutes: String,
        single: bool,
    ) -> Result<CommandResult, CommandError> {
        let permissions = self.users.get_permissions(user_id.clone()).await?;
        if !permissions.can_create() {
            return Err(CommandError::LackingPermission(Permission::Create).into());
        }
        let substitutes: Vec<&str> = Self::parse_substitutes(&substitutes, single)?;
        if substitutes.len() > 0 {
            let result = self.add_substitutes(&template, &substitutes).await;
            match result {
                Ok(receipt) => {
                    return Ok(CommandResult::Text(Self::sub_receipt_to_string(
                        platform,
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
            let result = self.add_substitutes(&template, &vec![]).await;
            match result {
                Ok(_) => {
                    let output = format!(
                        "Created template `{}`",
                        template.truncate_with_ellipse(ONE_HUNDRED)
                    );
                    return Ok(CommandResult::Text(output));
                }
                Err(e) => {
                    return Err(CommandError::ExecutionFailed(e.to_string()).into());
                }
            }
        }
    }

    pub async fn generate_command(
        &self,
        platform: Platform,
        user_id: U,
        interpreter: Arc<Mutex<FslInterpreter>>,
        input: Vec<String>,
        file: bool,
        ollama: bool,
    ) -> Result<CommandResult, CommandError> {
        let permissions = self.users.get_permissions(user_id.clone()).await?;
        if !permissions.can_generate() {
            return Err(CommandError::LackingPermission(Permission::Generate).into());
        } else if file && !permissions.has_permission(Permission::Owner) {
            // Must be host because this allows access to server file system
            return Err(CommandError::LackingPermission(Permission::Owner).into());
        } else if ollama && !permissions.can_use_ollama() {
            return Err(CommandError::LackingPermission(Permission::Ollama).into());
        }

        let input = input.join(" ");
        let input = if file && platform == Platform::Cli {
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
            self.user_generate_ollama(user_id, input, interpreter.clone())
                .await
                .map(|o| format!("{} {}", o.prompt, o.generated_text))
        } else {
            self.user_generate(user_id, input, interpreter.clone())
                .await
        };
        match result {
            Ok(output) => return Ok(CommandResult::Text(output)),
            Err(e) => {
                return Err(CommandError::ExecutionFailed(e.to_string()).into());
            }
        };
    }

    pub async fn fsl_command(
        &self,
        user_id: U,
        input: Vec<String>,
        interpreter: Arc<Mutex<FslInterpreter>>,
    ) -> Result<CommandResult, CommandError> {
        let permissions = self.users.get_permissions(user_id.clone()).await?;
        if !permissions.can_generate() {
            return Err(CommandError::LackingPermission(Permission::Generate).into());
        } else if !permissions.can_use_ollama() {
            return Err(CommandError::LackingPermission(Permission::Ollama).into());
        }

        let input = input.join(" ");

        let result = self
            .user_interpret_fsl(user_id, &input, interpreter.clone())
            .await;

        match result {
            Ok(output) => return Ok(CommandResult::Text(output)),
            Err(e) => {
                return Err(CommandError::ExecutionFailed(e.to_string()).into());
            }
        };
    }

    pub async fn grant_command(
        &self,
        invoker: U,
        receiver: U,
        permissions: Vec<Permission>,
    ) -> Result<CommandResult, CommandError> {
        let users = self.users.clone();
        let invoker_permissions = self.users.get_permissions(invoker.clone()).await?;

        if invoker_permissions.can_grant() {
            let result = users
                .grant_permissions(receiver.clone(), &permissions)
                .await;

            match result {
                Ok(_) => Ok(CommandResult::Text(format!(
                    "Granted {} permissions to {}",
                    permissions
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<String>>()
                        .join(", "),
                    receiver.to_string(),
                ))),
                Err(e) => Err(CommandError::ExecutionFailed(e.to_string())),
            }
        } else {
            Err(CommandError::LackingPermission(Permission::Grant))
        }
    }

    pub async fn revoke_command(
        &self,
        invoker: U,
        receiver: U,
        permissions: Vec<Permission>,
    ) -> Result<CommandResult, CommandError> {
        let users = self.users.clone();
        let invoker_permissions = self.users.get_permissions(invoker.clone()).await?;

        if invoker_permissions.can_revoke() {
            let result = users
                .revoke_permissions(receiver.clone(), &permissions)
                .await;

            match result {
                Ok(_) => Ok(CommandResult::Text(format!(
                    "Revoked {} permissions from {}",
                    permissions
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<String>>()
                        .join(", "),
                    receiver.to_string(),
                ))),
                Err(e) => Err(CommandError::ExecutionFailed(e.to_string())),
            }
        } else {
            Err(CommandError::LackingPermission(Permission::Revoke))
        }
    }

    pub async fn set_role(
        &self,
        invoker: U,
        receiver: U,
        role: Role,
    ) -> Result<CommandResult, CommandError> {
        let users = self.users.clone();
        let invoker_permissions = self.users.get_permissions(invoker.clone()).await?;

        if !invoker_permissions.can_grant() {
            Err(CommandError::LackingPermission(Permission::Grant))
        } else if !invoker_permissions.can_revoke() {
            Err(CommandError::LackingPermission(Permission::Revoke))
        } else {
            let result = users.set_role(receiver.clone(), role).await;

            match result {
                Ok(_) => Ok(CommandResult::Text(format!(
                    "Set role to {} for {}",
                    role,
                    receiver.to_string()
                ))),
                Err(e) => Err(CommandError::ExecutionFailed(e.to_string())),
            }
        }
    }

    pub async fn cancel_command(&self, user_id: U) -> Result<CommandResult, CommandError> {
        let user_ctx = self.users.get_or_insert(user_id).await?;
        user_ctx.cancel_generation.lock().await.cancel();
        Ok(CommandResult::None)
    }
}
