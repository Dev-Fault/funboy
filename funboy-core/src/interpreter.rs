use std::sync::Arc;

use fsl_interpreter::{
    InterpreterData,
    commands::{NUMERIC_TYPES, TEXT_TYPES, WHOLE_NUMBER_TYPES},
    types::{
        command::{ArgPos, ArgRule, Command, CommandError, Executor},
        value::Value,
    },
};

use crate::{
    Funboy, UserId,
    ollama::{OllamaGenerator, OllamaSettings},
    template_substitutor::TemplateDelimiter,
};

pub const SAY_TO: &str = "say_to";
pub const SAY_TO_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::Index(1), TEXT_TYPES),
];

pub const ASK_TO: &str = "ask_to";
pub const ASK_TO_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::Index(1), TEXT_TYPES),
    ArgRule::new(ArgPos::OptionalIndex(2), NUMERIC_TYPES),
];

pub const SAY: &str = "say";
pub const SAY_RULES: &'static [ArgRule] = &[ArgRule::new(ArgPos::Index(0), TEXT_TYPES)];
pub const DEFAULT_TIMEOUT_SECS: f64 = 60.0 * 30.0;
pub const ASK: &str = "ask";
pub const ASK_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::OptionalIndex(1), NUMERIC_TYPES),
];

pub const GET_SUB: &str = "get_sub";
pub const GET_SUB_RULES: &[ArgRule] = &[ArgRule::new(ArgPos::Index(0), TEXT_TYPES)];
pub fn create_get_sub_command<U: UserId>(funboy: Arc<Funboy<U>>) -> Executor {
    let get_sub_command = {
        move |command: Command, data: Arc<InterpreterData>| {
            let funboy = funboy.clone();
            async move {
                let mut args = command.take_args();
                let template = args.pop_front().unwrap().as_text(data).await?;
                let regex = TemplateDelimiter::BackTick.to_regex().await;
                if regex.is_match(&template) {
                    let template = template.trim_matches('`');
                    let sub = funboy.get_random_substitute(template).await;
                    match sub {
                        Ok(sub) => Ok(Value::Text(sub.name)),
                        Err(e) => Err(CommandError::Custom(e.to_string())),
                    }
                } else {
                    return Err(CommandError::Custom(format!(
                        "template name must be preceeded by a single ` (backtick)\nThis ensures if the template is renamed this {} will not be invalid",
                        GET_SUB
                    )));
                }
            }
        }
    };
    Some(Arc::new(get_sub_command))
}

pub const ASK_AI: &str = "ask_ai";
pub const ASK_AI_RULES: &[ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::Index(1), WHOLE_NUMBER_TYPES),
];
pub const MAX_WORD_LIMIT: i64 = 500;
pub fn create_ask_ai_command<U: UserId>(funboy: Arc<Funboy<U>>) -> Executor {
    let get_sub_command = {
        move |command: Command, data: Arc<InterpreterData>| {
            let funboy = funboy.clone();
            async move {
                let mut args = command.take_args();
                let prompt = args.pop_front().unwrap().as_text(data.clone()).await?;

                let word_limit = args.pop_front().unwrap().as_int(data).await?;
                if word_limit <= 0 {
                    return Err(CommandError::Custom(
                        "word limit must be greater than zero".to_string(),
                    ));
                } else if word_limit > MAX_WORD_LIMIT {
                    return Err(CommandError::Custom(format!(
                        "word limit cannot be greater than {}",
                        MAX_WORD_LIMIT
                    )));
                }

                let mut ollama_settings = OllamaSettings::default();
                ollama_settings.set_output_limit(word_limit as u16);
                let ollama_generator = OllamaGenerator::default();
                let model = funboy.get_ollama_model().await;

                let response = ollama_generator
                    .generate(&prompt, &ollama_settings, model)
                    .await;
                match response {
                    Ok(response) => {
                        let response = response.response;
                        Ok(Value::Text(response))
                    }
                    Err(e) => Err(CommandError::Custom(e.to_string())),
                }
            }
        }
    };
    Some(Arc::new(get_sub_command))
}
