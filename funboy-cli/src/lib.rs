use std::{collections::VecDeque, sync::Arc};

use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    commands::{NUMERIC_TYPES, TEXT_TYPES},
    types::{
        command::{ArgPos, ArgRule, Executor},
        value::Value,
    },
};
use funboy_core::Funboy;
use rustyline::DefaultEditor;
use tokio::sync::Mutex;

const GENERATE: &str = "generate";
const GENERATE_FILE: &str = "generate_file";
const GENERATE_OLLAMA: &str = "generate_ollama";
const GENERATE_INTERACTIVE: &str = "generate_interactive";
const INTERPRETER: &str = "interpreter";
const ADD_SUBS: &str = "add_subs";
const UPLOAD_SUB: &str = "upload_subs";
const COPY_SUBS: &str = "copy_subs";
const DELETE_SUBS: &str = "delete_subs";
const LIST_SUBS: &str = "list_subs";
const RENAME_TEMPLATE: &str = "rename_template";
const DELETE_TEMPLATES: &str = "delete_templates";
const LIST_TEMPLATES: &str = "list_templates";
const HELP: &str = "help";
const LIST_OLLAMA_MODELS: &str = "list_ollama_models";
const SET_OLLAMA_MODEL: &str = "set_ollama_model";
const LIST_OLLAMA_SETTINGS: &str = "list_ollama_settings";
const SET_OLLAMA_WORD_LIMIT: &str = "set_ollama_word_limit";
const SET_OLLAMA_PARAMETERS: &str = "set_ollama_parameters";
const RESET_OLLAMA_PARAMETERS: &str = "reset_ollama_parameters";
const SET_OLLAMA_SYSTEM_PROMPT: &str = "set_ollama_system_prompt";
const RESET_OLLAMA_SYSTEM_PROMPT: &str = "reset_ollama_system_prompt";
const SET_OLLAMA_TEMPLATE: &str = "set_ollama_template";
const RESET_OLLAMA_TEMPLATE: &str = "reset_ollama_template";

pub enum Command {
    Generate,
    GenerateFile,
    GenerateOllama,
    GenerateInteractive,
    Interpreter,
    AddSubs,
    UploadSub,
    CopySubs,
    DeleteSubs,
    ListSubs,
    RenameTemplate,
    DeleteTemplates,
    ListTemplates,
    Help,
    ListOllamaModels,
    SetOllamaModel,
    ListOllamaSettings,
    SetOllamaWordLimit,
    SetOllamaParameters,
    ResetOllamaParameters,
    SetOllamaSystemPrompt,
    ResetOllamaSystemPrompt,
    SetOllamaTemplate,
    ResetOllamaTemplate,
    Unknown,
}

impl From<&str> for Command {
    fn from(value: &str) -> Self {
        match value {
            GENERATE => Command::Generate,
            GENERATE_INTERACTIVE => Command::GenerateInteractive,
            INTERPRETER => Command::Interpreter,
            _ => Command::Unknown,
        }
    }
}

#[derive(Debug)]
pub enum ParseError {
    EmptyInput,
    UnknownCommand(String),
}

impl Into<Error> for ParseError {
    fn into(self) -> Error {
        Error::ParseError(self)
    }
}

#[derive(Debug)]
pub enum CommandError {
    ExecutionFailed(String, String),
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

pub enum Context {
    Normal,
    InteractiveGeneration,
    Interpreter,
}

pub enum CommandResult {
    Text(String),
    ContextSwitch(Context),
}

pub async fn interpret_input(
    funboy: Arc<Funboy>,
    interpreter: Arc<Mutex<FslInterpreter>>,
    input: &str,
) -> Result<CommandResult, Error> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ParseError::EmptyInput.into());
    };

    let mut tokens: VecDeque<&str> = input.split_whitespace().collect();
    let command_str = tokens
        .pop_front()
        .expect("tokens should have at least one str after empty check");
    let command = Command::from(command_str);

    match command {
        Command::Generate => {
            let generate_input = Into::<Vec<_>>::into(tokens).join(" ");
            let result = funboy.generate(&generate_input, interpreter).await;
            match result {
                Ok(output) => return Ok(CommandResult::Text(output)),
                Err(e) => {
                    return Err(CommandError::ExecutionFailed(
                        command_str.to_string(),
                        e.to_string(),
                    )
                    .into());
                }
            };
        }
        Command::GenerateFile => todo!(),
        Command::GenerateOllama => todo!(),
        Command::GenerateInteractive => {
            return Ok(CommandResult::ContextSwitch(Context::InteractiveGeneration));
        }
        Command::Interpreter => {
            return Ok(CommandResult::ContextSwitch(Context::Interpreter));
        }
        Command::AddSubs => todo!(),
        Command::UploadSub => todo!(),
        Command::CopySubs => todo!(),
        Command::DeleteSubs => todo!(),
        Command::ListSubs => todo!(),
        Command::RenameTemplate => todo!(),
        Command::DeleteTemplates => todo!(),
        Command::ListTemplates => todo!(),
        Command::Help => todo!(),
        Command::ListOllamaModels => todo!(),
        Command::SetOllamaModel => todo!(),
        Command::ListOllamaSettings => todo!(),
        Command::SetOllamaWordLimit => todo!(),
        Command::SetOllamaParameters => todo!(),
        Command::ResetOllamaParameters => todo!(),
        Command::SetOllamaSystemPrompt => todo!(),
        Command::ResetOllamaSystemPrompt => todo!(),
        Command::SetOllamaTemplate => todo!(),
        Command::ResetOllamaTemplate => todo!(),
        Command::Unknown => return Err(ParseError::UnknownCommand(command_str.to_string()).into()),
    }
}
