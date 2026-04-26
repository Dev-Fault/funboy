use std::{sync::Arc, time::Duration};

use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    commands::{NUMERIC_TYPES, TEXT_TYPES, WHOLE_NUMBER_TYPES},
    types::{
        command::{ArgPos, ArgRule, Command, CommandError, Handler},
        value::Value,
    },
};
use tokio::{sync::Mutex, time::sleep};

use crate::{
    Funboy,
    format::{TWO_THOUSAND, split_message},
    ollama::{OllamaGenerator, OllamaSettings},
    rate_limiter::{RateLimit, RateLimitResult},
    template_substitutor::TemplateDelimiter,
    user::FunboyUserId,
};

pub trait Messenger: Clone + Sync + Send + 'static {
    fn say(&self, message: &str);
    fn await_response(
        &self,
        timeout: f64,
    ) -> impl std::future::Future<Output = Result<String, CommandError>> + Send;
    fn mention(&self) -> String;
}

pub trait Interactor: Clone + Sync + Send + 'static {
    fn say_to_user(
        &self,
        user_name: &str,
        message: &str,
    ) -> impl std::future::Future<Output = Result<(), CommandError>> + Send;
    fn await_user_response(
        &self,
        user_name: &str,
        timeout: f64,
    ) -> impl std::future::Future<Output = Result<String, CommandError>> + Send;
}

#[derive(Clone)]
pub struct InterpreterLimits<U: FunboyUserId> {
    pub rate_limit: Option<Arc<Mutex<RateLimit<U>>>>,
    pub message_limit: Option<u16>,
    pub message_delay_ms: Option<u64>,
}

impl<U: FunboyUserId> InterpreterLimits<U> {
    pub fn new(
        rate_limit: Option<RateLimit<U>>,
        message_limit: Option<u16>,
        message_delay_ms: Option<u64>,
    ) -> Self {
        Self {
            rate_limit: rate_limit.map(|r| Arc::new(Mutex::new(r))),
            message_limit,
            message_delay_ms,
        }
    }

    pub fn none() -> Self {
        Self {
            rate_limit: None,
            message_limit: None,
            message_delay_ms: None,
        }
    }
}

impl<U: FunboyUserId> Default for InterpreterLimits<U> {
    fn default() -> Self {
        Self {
            rate_limit: Some(Default::default()),
            message_limit: Some(TWO_THOUSAND_MESSAGES),
            message_delay_ms: Some(FIVE_HUNDRED_MS),
        }
    }
}

#[derive(Clone)]
pub struct InterpreterContext<U: FunboyUserId, M: Messenger> {
    pub user_id: U,
    pub funboy: Arc<Funboy<U>>,
    pub messages_sent: Arc<Mutex<u16>>,
    pub messenger: M,
    pub limits: InterpreterLimits<U>,
    interpreter: Arc<Mutex<FslInterpreter>>,
}

impl<U: FunboyUserId, M: Messenger> InterpreterContext<U, M> {
    pub fn new(
        user_id: U,
        funboy: Arc<Funboy<U>>,
        messenger: M,
        limits: InterpreterLimits<U>,
    ) -> Self {
        Self {
            user_id: user_id,
            funboy: funboy,
            messages_sent: Arc::new(Mutex::new(0)),
            messenger,
            limits,
            interpreter: Arc::new(Mutex::new(FslInterpreter::new())),
        }
    }

    pub async fn generate_message(&self, message: &str) -> Result<String, CommandError> {
        match self
            .funboy
            .generate(message, self.interpreter.clone())
            .await
        {
            Ok(gen_msg) => Ok(gen_msg),
            Err(e) => {
                return Err(CommandError::Custom(e.to_string()));
            }
        }
    }
}

async fn check_limits<U: FunboyUserId, M: Messenger>(
    ictx: InterpreterContext<U, M>,
) -> Result<(), CommandError> {
    if let Some(limit) = ictx.limits.message_limit {
        let mut call_count = ictx.messages_sent.lock().await;
        if *call_count >= limit {
            *call_count = 0;
            return Err(CommandError::Custom(format!("message limit exceeded",)));
        }
        *call_count = call_count.saturating_add(1);
    }

    if let Some(rate_limit) = ictx.limits.rate_limit {
        let mut rate_limit = rate_limit.lock().await;
        match rate_limit.check(ictx.user_id) {
            RateLimitResult::MaxLimitsReached => {
                return Err(CommandError::Custom(format!(
                    "exceeded rate limit too many times, please wait a bit before trying again",
                )));
            }
            RateLimitResult::UsesPerIntervalreached => Ok(()),
            RateLimitResult::Ok => Ok(()),
        }
    } else {
        Ok(())
    }
}

pub const SAY: &str = "say";
pub const SAY_RULES: &'static [ArgRule] = &[ArgRule::new(ArgPos::Index(0), TEXT_TYPES)];
pub const FIVE_HUNDRED_MS: u64 = 500;
pub const TWO_THOUSAND_MESSAGES: u16 = 2000;
pub const SAY_MAX_OUTPUT_LENGTH: usize = 8000;
pub fn say_command<U: FunboyUserId, M: Messenger>(ictx: InterpreterContext<U, M>) -> Handler {
    Handler::from({
        let ictx = ictx.clone();
        move |command: Command, interpreter_data| {
            let ictx = ictx.clone();
            async move {
                let mut values = command.take_args();
                let message = values
                    .pop_front()
                    .unwrap()
                    .as_text(interpreter_data)
                    .await?;

                let message = ictx.generate_message(&message).await?;

                if message.len() < SAY_MAX_OUTPUT_LENGTH {
                    for m in split_message(&message, TWO_THOUSAND) {
                        check_limits(ictx.clone()).await?;
                        ictx.messenger.say(m);
                    }

                    if let Some(delay) = ictx.limits.message_delay_ms {
                        sleep(Duration::from_millis(delay)).await;
                    }

                    Ok(Value::None)
                } else {
                    return Err(CommandError::Custom(format!(
                        "Message exceeded max length of {} characters",
                        SAY_MAX_OUTPUT_LENGTH,
                    )));
                }
            }
        }
    })
}

pub const SAY_TO: &str = "say_to";
pub const SAY_TO_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::Index(1), TEXT_TYPES),
];
pub fn say_to_command<U: FunboyUserId, M: Messenger + Interactor>(
    ictx: InterpreterContext<U, M>,
) -> Handler {
    Handler::from({
        let ictx = ictx.clone();
        move |command: Command, interpreter_data: Arc<InterpreterData>| {
            let ictx = ictx.clone();
            async move {
                check_limits(ictx.clone()).await?;

                let mut values = command.take_args();
                let user_name = values
                    .pop_front()
                    .unwrap()
                    .as_text(interpreter_data.clone())
                    .await?;
                let message = values
                    .pop_front()
                    .unwrap()
                    .as_text(interpreter_data)
                    .await?;

                let message = ictx.generate_message(&message).await?;
                ictx.messenger.say_to_user(&user_name, &message).await?;

                if let Some(delay) = ictx.limits.message_delay_ms {
                    sleep(Duration::from_millis(delay)).await;
                }

                Ok(Value::None)
            }
        }
    })
}

pub const DEFAULT_TIMEOUT_SECS: f64 = 60.0 * 30.0;
pub const ASK: &str = "ask";
pub const ASK_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::OptionalIndex(1), NUMERIC_TYPES),
];
const MAX_TIMEOUT_SECS: f64 = 60.0 * 60.0;
const STOP: &str = "-STOP-";

pub fn ask_command<U: FunboyUserId, M: Messenger>(ictx: InterpreterContext<U, M>) -> Handler {
    Handler::from({
        move |command: Command, data: Arc<InterpreterData>| {
            let ictx = ictx.clone();
            async move {
                check_limits(ictx.clone()).await?;

                let mut values = command.take_args();

                let question = values.pop_front().unwrap().as_text(data.clone()).await?;
                let timeout = values
                    .pop_front()
                    .unwrap_or(Value::Float(DEFAULT_TIMEOUT_SECS));

                let question = format!("{}\n\n{}", ictx.messenger.mention(), question);
                let question = format!("{}\n\n{}", question, "(enter -STOP- to quit)");
                let question = ictx.generate_message(&question).await?;

                if question.len() < SAY_MAX_OUTPUT_LENGTH {
                    for m in split_message(&question, TWO_THOUSAND) {
                        ictx.messenger.say(&m);
                        sleep(Duration::from_millis(FIVE_HUNDRED_MS)).await;
                    }
                } else {
                    return Err(CommandError::Custom(format!(
                        "Message exceeded max length of {} characters",
                        SAY_MAX_OUTPUT_LENGTH,
                    )));
                }

                let timeout = timeout.as_float(data.clone()).await?;
                validate_time_out(timeout, MAX_TIMEOUT_SECS)?;

                let response = ictx.messenger.await_response(timeout).await?;
                if response == STOP {
                    return Err(CommandError::ProgramExited);
                }
                let response = ictx.generate_message(&response).await?;

                Ok(Value::Text(response))
            }
        }
    })
}

pub const ASK_TO: &str = "ask_to";
pub const ASK_TO_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::Index(1), TEXT_TYPES),
    ArgRule::new(ArgPos::OptionalIndex(2), NUMERIC_TYPES),
];

pub fn ask_to_command<U: FunboyUserId, M: Messenger + Interactor>(
    ictx: InterpreterContext<U, M>,
) -> Handler {
    Handler::from({
        move |command: Command, data: Arc<InterpreterData>| {
            let ictx = ictx.clone();
            async move {
                check_limits(ictx.clone()).await?;

                sleep(Duration::from_millis(FIVE_HUNDRED_MS)).await;

                let mut values = command.take_args();

                let user_name = values.pop_front().unwrap().as_text(data.clone()).await?;
                let question = values.pop_front().unwrap().as_text(data.clone()).await?;
                let timeout = values
                    .pop_front()
                    .unwrap_or(Value::Float(DEFAULT_TIMEOUT_SECS));

                let question = format!("{}\n\n{}", question, "(enter -STOP- to quit)");

                let timeout = timeout.as_float(data.clone()).await?;
                validate_time_out(timeout, MAX_TIMEOUT_SECS)?;

                ictx.messenger
                    .say_to_user(&user_name, &ictx.generate_message(&question).await?)
                    .await?;

                let response = ictx
                    .messenger
                    .await_user_response(&user_name, timeout)
                    .await?;
                if response == STOP {
                    return Err(CommandError::ProgramExited);
                }
                let response = ictx.generate_message(&response).await?;

                Ok(Value::Text(response))
            }
        }
    })
}

pub fn validate_time_out(time_out: f64, max: f64) -> Result<(), CommandError> {
    if !time_out.is_finite() {
        return Err(CommandError::NonFiniteValue);
    } else if time_out.is_sign_negative() {
        return Err(CommandError::Custom(format!(
            "time_out cannot be a negative number"
        )));
    } else if time_out > max {
        return Err(CommandError::Custom(format!(
            "timeout cannot be greater than {} seconds",
            max
        )));
    }
    Ok(())
}

pub const GET_SUB: &str = "get_sub";
pub const GET_SUB_RULES: &[ArgRule] = &[ArgRule::new(ArgPos::Index(0), TEXT_TYPES)];
pub fn get_sub_command<U: FunboyUserId>(funboy: Arc<Funboy<U>>) -> Handler {
    Handler::from({
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
    })
}

pub const ASK_AI: &str = "ask_ai";
pub const ASK_AI_RULES: &[ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::Index(1), WHOLE_NUMBER_TYPES),
];
pub const MAX_WORD_LIMIT: i64 = 500;
pub fn ask_ai_command<U: FunboyUserId>(funboy: Arc<Funboy<U>>) -> Handler {
    Handler::from({
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
    })
}
