use std::{sync::Arc, time::Duration};

use fsl_core::data::InterpreterData;
use fsl_core::error::{ExecutionError, RuntimeError, Span, ToExecutionError};
use fsl_core::libraries::Library;
use fsl_core::libraries::standard::{MAYBE_INDEXABLE, NOT_NONE};
use fsl_core::types::FslType;
use fsl_core::types::value::FslValue;
use fsl_core::{
    FslInterpreter,
    libraries::standard::{MAYBE_INT, MAYBE_NUMBER, MAYBE_TEXT},
    types::{
        command::{ArgPos, ArgRule, Command, Handler},
        value::Value,
    },
};
use tokio::{sync::Mutex, time::sleep};

use crate::database::{self, OrderBy};
use crate::format::AsStrs;
use crate::{
    Funboy,
    format::{TWO_THOUSAND, split_message},
    ollama::{OllamaGenerator, OllamaSettings},
    rate_limiter::{RateLimit, RateLimitResult},
    template_substitutor::TemplateDelimiter,
    user::FunboyUserId,
};

use futures::FutureExt;

pub trait Messenger: Clone + Sync + Send + 'static {
    fn say(&self, message: &str);
    fn await_response(
        &self,
        timeout: f64,
    ) -> impl std::future::Future<Output = Result<String, RuntimeError>> + Send;
    fn mention(&self) -> String;
}

pub trait Interactor: Clone + Sync + Send + 'static {
    fn say_to_user(
        &self,
        user_name: &str,
        message: &str,
    ) -> impl std::future::Future<Output = Result<(), RuntimeError>> + Send;
    fn await_user_response(
        &self,
        user_name: &str,
        timeout: f64,
    ) -> impl std::future::Future<Output = Result<String, RuntimeError>> + Send;
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct InterpreterContext<U: FunboyUserId, M: Messenger> {
    pub user_id: U,
    pub funboy: Arc<Funboy<U>>,
    pub messages_sent: Arc<Mutex<u16>>,
    pub messenger: M,
    pub limits: InterpreterLimits<U>,
    pub interpreter: FslInterpreter,
}

impl<U: FunboyUserId, M: Messenger> InterpreterContext<U, M> {
    pub async fn new(
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
            interpreter: FslInterpreter::new().await,
        }
    }

    pub async fn register_default_funboy_commands(&mut self) {
        self.interpreter.register_library(Library::Async).await;
        self.interpreter
            .register(
                GET_SUBS,
                GET_SUBS_RULES,
                get_subs_command(self.funboy.clone()),
            )
            .await;
        self.interpreter
            .register(
                ADD_SUBS,
                ADD_SUBS_RULES,
                add_subs_command(self.funboy.clone()),
            )
            .await;
        self.interpreter
            .register(
                DELETE_SUBS,
                DELETE_SUBS_RULES,
                delete_subs_command(self.funboy.clone()),
            )
            .await;
        self.interpreter
            .register(ASK_AI, ASK_AI_RULES, ask_ai_command(self.funboy.clone()))
            .await;
        self.interpreter
            .register(SAY, SAY_RULES, say_command(self.clone()))
            .await;
        self.interpreter
            .register(ASK, ASK_RULES, ask_command(self.clone()))
            .await;
    }

    pub async fn generate_message<'c>(
        &self,
        message: &str,
        span: Span<'c>,
    ) -> Result<String, ExecutionError<'c>> {
        match self.funboy.generate(message, &self.interpreter).await {
            Ok(gen_msg) => Ok(gen_msg),
            Err(e) => {
                let e = e.to_string();
                let e = e.replace("```", "");
                return Err(RuntimeError::Custom(e).to_exec(span));
            }
        }
    }
}

impl<U: FunboyUserId, M: Messenger + Interactor> InterpreterContext<U, M> {
    pub async fn register_interactive_funboy_commands(&mut self) {
        self.register_default_funboy_commands().await;
        self.interpreter
            .register(GET_SUB, GET_SUB_RULES, get_sub_command(self.clone()))
            .await;
        self.interpreter
            .register(SAY_TO, SAY_TO_RULES, say_to_command(self.clone()))
            .await;
        self.interpreter
            .register(ASK_TO, ASK_TO_RULES, ask_to_command(self.clone()))
            .await;
    }

    pub async fn register_shell_commands(&mut self) {
        {
            use sh_exec::*;
            let permissions = self
                .funboy
                .users
                .get_permissions(self.user_id.clone())
                .await;
            match permissions {
                Ok(permissions) => {
                    if permissions.can_exec() {
                        self.interpreter
                            .register(SANDBOXED_SH, SANDBOXED_SH_RULES, sandboxed_sh_command())
                            .await;
                        self.interpreter
                            .register(
                                INTERACTIVE_SH,
                                INTERACTIVE_SH_RULES,
                                interactive_sh_command(self.clone()),
                            )
                            .await;
                    }
                }
                Err(e) => eprintln!("{e}"),
            }
        }
    }
}

async fn check_limits<'c, U: FunboyUserId, M: Messenger>(
    ictx: InterpreterContext<U, M>,
    span: Span<'c>,
) -> Result<(), ExecutionError<'c>> {
    if let Some(limit) = ictx.limits.message_limit {
        let mut call_count = ictx.messages_sent.lock().await;
        if *call_count >= limit {
            *call_count = 0;
            return Err(RuntimeError::Custom(format!("message limit exceeded",)).to_exec(span));
        }
        *call_count = call_count.saturating_add(1);
    }

    if let Some(rate_limit) = ictx.limits.rate_limit {
        let mut rate_limit = rate_limit.lock().await;
        match rate_limit.check(ictx.user_id) {
            RateLimitResult::MaxLimitsReached => {
                return Err(RuntimeError::Custom(format!(
                    "exceeded rate limit too many times, please wait a bit before trying again",
                ))
                .to_exec(span));
            }
            RateLimitResult::UsesPerIntervalreached => Ok(()),
            RateLimitResult::Ok => Ok(()),
        }
    } else {
        Ok(())
    }
}

pub const SAY: &str = "say";
pub const SAY_RULES: &'static [ArgRule] = &[ArgRule::new(ArgPos::AnyFrom(0), NOT_NONE)];
pub const FIVE_HUNDRED_MS: u64 = 500;
pub const TWO_THOUSAND_MESSAGES: u16 = 2000;
pub const SAY_MAX_OUTPUT_LENGTH: usize = 8000;
pub fn say_command<U: FunboyUserId, M: Messenger>(ictx: InterpreterContext<U, M>) -> Handler {
    Handler::new(move |command, data| {
        let ictx = ictx.clone();
        async move {
            let mut command = command;
            let args = command.take_args();

            let mut message = String::new();

            for arg in args {
                let text = arg.as_text(data.clone()).await?;
                message.push_str(&text);
            }

            let message = ictx.generate_message(&message, command.span).await?;

            if message.len() < SAY_MAX_OUTPUT_LENGTH {
                for m in split_message(&message, TWO_THOUSAND) {
                    check_limits(ictx.clone(), command.span).await?;
                    ictx.messenger.say(m);
                }

                if let Some(delay) = ictx.limits.message_delay_ms {
                    sleep(Duration::from_millis(delay)).await;
                }

                Ok(Value::None)
            } else {
                return Err(RuntimeError::Custom(format!(
                    "Message exceeded max length of {} characters",
                    SAY_MAX_OUTPUT_LENGTH,
                ))
                .to_exec(command.span));
            }
        }
        .boxed()
    })
}

pub const SAY_TO: &str = "say_to";
pub const SAY_TO_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), MAYBE_TEXT),
    ArgRule::new(ArgPos::AnyFrom(1), NOT_NONE),
];
pub fn say_to_command<U: FunboyUserId, M: Messenger + Interactor>(
    ictx: InterpreterContext<U, M>,
) -> Handler {
    Handler::new(move |command: Command, data: Arc<InterpreterData>| {
        let ictx = ictx.clone();
        async move {
            let mut command = command;
            check_limits(ictx.clone(), command.span).await?;

            let mut args = command.take_args();
            let user_name = args.pop_front().unwrap();
            let user_name = user_name.as_text(data.clone()).await?;

            let mut message = String::new();

            for arg in args {
                let text = arg.as_text(data.clone()).await?;
                message.push_str(&text);
            }

            let message = ictx.generate_message(&message, command.span).await?;

            ictx.messenger
                .say_to_user(&user_name, &message)
                .await
                .map_err(|e| e.to_exec(command.span))?;

            if let Some(delay) = ictx.limits.message_delay_ms {
                sleep(Duration::from_millis(delay)).await;
            }

            Ok(Value::None)
        }
        .boxed()
    })
}

pub const DEFAULT_TIMEOUT_SECS: f64 = 60.0 * 30.0;
pub const ASK: &str = "ask";
pub const ASK_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), MAYBE_TEXT),
    ArgRule::new(ArgPos::OptionalIndex(1), MAYBE_NUMBER),
];
const MAX_TIMEOUT_SECS: f64 = 60.0 * 60.0;
const STOP: &str = "-STOP-";

pub fn ask_command<U: FunboyUserId, M: Messenger>(ictx: InterpreterContext<U, M>) -> Handler {
    Handler::new(move |command: Command, data: Arc<InterpreterData>| {
        let ictx = ictx.clone();
        async move {
            let mut command = command;
            check_limits(ictx.clone(), command.span).await?;

            let mut args = command.take_args();

            let question = args.pop_front().unwrap();
            let question_span = question.span;
            let question = question.as_text(data.clone()).await?;
            let timeout = match args.pop_front() {
                Some(timeout) => {
                    let timeout_span = timeout.span;
                    let timeout = timeout.as_float(data.clone()).await?;
                    validate_time_out(timeout, MAX_TIMEOUT_SECS, timeout_span)?;
                    timeout
                }
                None => DEFAULT_TIMEOUT_SECS,
            };

            let question = format!("{}\n\n{}", ictx.messenger.mention(), question);
            let question = format!("{}\n\n{}", question, "(enter -STOP- to quit)");
            let question = ictx.generate_message(&question, question_span).await?;

            if question.len() < SAY_MAX_OUTPUT_LENGTH {
                for m in split_message(&question, TWO_THOUSAND) {
                    ictx.messenger.say(&m);
                    sleep(Duration::from_millis(FIVE_HUNDRED_MS)).await;
                }
            } else {
                return Err(RuntimeError::Custom(format!(
                    "Message exceeded max length of {} characters",
                    SAY_MAX_OUTPUT_LENGTH,
                ))
                .to_exec(command.span));
            }

            let response = ictx
                .messenger
                .await_response(timeout)
                .await
                .map_err(|e| e.to_exec(command.span))?;
            if response == STOP {
                return Err(RuntimeError::ProgramExited.to_exec(command.span));
            }
            let response = ictx.generate_message(&response, command.span).await?;

            Ok(Value::from(response))
        }
        .boxed()
    })
}

pub const ASK_TO: &str = "ask_to";
pub const ASK_TO_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), MAYBE_TEXT),
    ArgRule::new(ArgPos::Index(1), MAYBE_TEXT),
    ArgRule::new(ArgPos::OptionalIndex(2), MAYBE_NUMBER),
];

pub fn ask_to_command<U: FunboyUserId, M: Messenger + Interactor>(
    ictx: InterpreterContext<U, M>,
) -> Handler {
    Handler::new(move |command: Command, data: Arc<InterpreterData>| {
        let ictx = ictx.clone();
        async move {
            let mut command = command;
            check_limits(ictx.clone(), command.span).await?;

            sleep(Duration::from_millis(FIVE_HUNDRED_MS)).await;

            let mut args = command.take_args();

            let user_name = args.pop_front().unwrap();
            let user_name = user_name.as_text(data.clone()).await?;
            let question = args.pop_front().unwrap();
            let question_span = question.span;
            let question = question.as_text(data.clone()).await?;

            let question = format!("{}\n\n{}", question, "(enter -STOP- to quit)");

            let timeout = match args.pop_front() {
                Some(timeout) => {
                    let timeout_span = timeout.span;
                    let timeout = timeout.as_float(data.clone()).await?;
                    validate_time_out(timeout, MAX_TIMEOUT_SECS, timeout_span)?;
                    timeout
                }
                None => DEFAULT_TIMEOUT_SECS,
            };

            ictx.messenger
                .say_to_user(
                    &user_name,
                    &ictx.generate_message(&question, question_span).await?,
                )
                .await
                .map_err(|e| e.to_exec(command.span))?;

            let response = ictx
                .messenger
                .await_user_response(&user_name, timeout)
                .await
                .map_err(|e| e.to_exec(command.span))?;

            if response == STOP {
                return Err(RuntimeError::ProgramExited.to_exec(command.span));
            }
            let response = ictx.generate_message(&response, command.span).await?;

            Ok(Value::from(response))
        }
        .boxed()
    })
}

pub fn validate_time_out<'c>(
    time_out: f64,
    max: f64,
    span: Span<'c>,
) -> Result<(), ExecutionError<'c>> {
    if !time_out.is_finite() {
        return Err(RuntimeError::NonFiniteValue.to_exec(span));
    } else if time_out.is_sign_negative() {
        return Err(
            RuntimeError::Custom(format!("time_out cannot be a negative number")).to_exec(span),
        );
    } else if time_out > max {
        return Err(RuntimeError::Custom(format!(
            "timeout cannot be greater than {} seconds",
            max
        ))
        .to_exec(span));
    }
    Ok(())
}

pub mod sh_exec {
    use fsl_core::error::ToExecutionError;
    use fsl_core::libraries::standard::MAYBE_TEXT;
    use fsl_core::types::command::Handler;
    use fsl_core::types::command::{ArgPos, ArgRule};
    use fsl_core::types::value::FslValue;
    use fsl_core::types::value::Value;
    use fsl_core::{data::InterpreterData, types::command::Command};
    use futures::FutureExt;
    use std::process::Stdio;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::interpreter::{Interactor, InterpreterContext, Messenger, STOP, check_limits};
    use crate::user::FunboyUserId;
    use fsl_core::error::RuntimeError;

    pub const SANDBOXED_SH_RULES: &[ArgRule] = &[ArgRule::new(ArgPos::Index(0), MAYBE_TEXT)];
    pub const SANDBOXED_SH: &str = "sh";
    #[cfg(target_os = "linux")]
    pub fn sandboxed_sh_command() -> Handler {
        Handler::new(move |command: Command, data: Arc<InterpreterData>| {
            async move {
                use fsl_core::error::ToExecutionError;

                let mut command = command;
                let mut args = command.take_args();
                let script = args.pop_front().unwrap().as_text(data).await?;
                let output = tokio::process::Command::new("sudo")
                    .args(["-u", "sandbox", "sh", "-c", &script])
                    .current_dir("/home/sandbox")
                    .output()
                    .await
                    .map_err(|_| {
                        RuntimeError::FailedToRun {
                            process: "sh".into(),
                        }
                        .to_exec(command.span)
                    })?;

                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    eprintln!("{err}");
                }

                let output = output.stdout;
                let text = match String::from_utf8(output) {
                    Ok(s) => s,
                    Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
                };
                Ok(Value::from(text))
            }
            .boxed()
        })
    }

    const INTERACTIVE_LOOP_LIMIT: u16 = 1000;
    const INTERACTIVE_BUF_SIZE: usize = 4096;
    pub const INTERACTIVE_SH: &str = "interactive_sh";
    pub const INTERACTIVE_SH_RULES: &'static [ArgRule] = &[
        ArgRule::new(ArgPos::Index(0), MAYBE_TEXT),
        ArgRule::new(ArgPos::OptionalIndex(1), MAYBE_TEXT),
    ];
    pub fn interactive_sh_command<U: FunboyUserId, M: Messenger + Interactor>(
        ictx: InterpreterContext<U, M>,
    ) -> Handler {
        Handler::new(move |command: Command, data: Arc<InterpreterData>| {
            let ictx = ictx.clone();
            async move {
                let mut command = command;
                // interactive_sh("python game.py", optional_user)
                let mut args = command.take_args();
                let child_process = args.pop_front().unwrap().as_text(data.clone()).await?;
                let user_name = match args.pop_front() {
                    Some(user_name) => &*user_name.as_text(data.clone()).await?,
                    None => &ictx.messenger.mention(),
                };
                let child = tokio::process::Command::new("sudo")
                    .args(["-u", "sandbox", "sh", "-c", &child_process])
                    .current_dir("/home/sandbox")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn();

                let mut child = match child {
                    Ok(child) => child,
                    Err(e) => {
                        return Err(fsl_core::error::RuntimeError::Custom(format!("{e}"))
                            .to_exec(command.span));
                    }
                };

                let mut stdout = child.stdout.take().unwrap();
                let mut stdin = child.stdin.take().unwrap();
                let mut buf = vec![0u8; INTERACTIVE_BUF_SIZE];

                for _ in 0..INTERACTIVE_LOOP_LIMIT {
                    check_limits(ictx.clone(), command.span).await?;

                    let result = stdout.read(&mut buf).await;
                    let n = match result {
                        Ok(n) => n,
                        Err(e) => {
                            return Err(fsl_core::error::RuntimeError::Custom(format!("{e}"))
                                .to_exec(command.span));
                        }
                    };

                    if n == 0 {
                        break;
                    }

                    let output = String::from_utf8_lossy(&buf[..n]);
                    let output = format!("{}\n\n{}", output, "(enter -STOP- to quit)");
                    ictx.messenger
                        .say_to_user(&user_name, &output)
                        .await
                        .map_err(|e| e.to_exec(command.span))?;
                    let response = ictx
                        .messenger
                        .await_user_response(&user_name, 60.0)
                        .await
                        .map_err(|e| e.to_exec(command.span))?;

                    if response == STOP {
                        break;
                    }

                    let response = ictx.generate_message(&response, command.span).await?;

                    if let Err(e) = stdin.write_all(response.as_bytes()).await {
                        return Err(fsl_core::error::RuntimeError::Custom(format!("{e}"))
                            .to_exec(command.span));
                    }
                    if let Err(e) = stdin.write_all(b"\n").await {
                        return Err(fsl_core::error::RuntimeError::Custom(format!("{e}"))
                            .to_exec(command.span));
                    }
                }

                let _ = child.kill().await;

                Ok(Value::None)
            }
            .boxed()
        })
    }
}

pub const GET_SUB: &str = "get_sub";
pub const GET_SUB_RULES: &[ArgRule] = &[ArgRule::new(ArgPos::Index(0), MAYBE_TEXT)];
pub fn get_sub_command<U: FunboyUserId, M: Messenger + Interactor>(
    ictx: InterpreterContext<U, M>,
) -> Handler {
    Handler::new(move |command: Command, data: Arc<InterpreterData>| {
        let ictx = ictx.clone();
        async move {
                let mut command = command;
                let mut args = command.take_args();
                let template = args.pop_front().unwrap().as_text(data).await?;
                let regex = TemplateDelimiter::BackTick.to_regex().await;
                if regex.is_match(&template) {
                    let template = template.trim_matches('`');
                    let sub = ictx.funboy.get_random_substitute(template).await;
                    match sub {
                        Ok(sub) => {
                            let sub = sub.name;
                            let text = ictx.generate_message(&sub, command.span).await?;
                            Ok(Value::from(text))
                        },
                        Err(e) => Err(RuntimeError::Custom(e.to_string()).to_exec(command.span)),
                    }
                } else {
                    return Err(RuntimeError::Custom(format!(
                        "template name must be preceeded by a single ` (backtick)\nThis ensures if the template is renamed this {} will not be invalid",
                        GET_SUB
                    )).to_exec(command.span));
                }
            }.boxed()
    })
}

pub const GET_SUBS: &str = "get_subs";
pub const GET_SUBS_RULES: &[ArgRule] = &[ArgRule::new(ArgPos::Index(0), MAYBE_TEXT)];
pub fn get_subs_command<U: FunboyUserId>(funboy: Arc<Funboy<U>>) -> Handler {
    Handler::new(move |command: Command, data: Arc<InterpreterData>| {
        let funboy = funboy.clone();
        async move {
                let mut command = command;
                let mut args = command.take_args();
                let template = args.pop_front().unwrap().as_text(data).await?;
                let regex = TemplateDelimiter::BackTick.to_regex().await;
                if regex.is_match(&template) {
                    let template = template.trim_matches('`');
                    let subs = funboy.get_substitutes(template, None, OrderBy::Default, database::Limit::None).await;
                    match subs {
                        Ok(subs) => {
                            let mut values = Vec::with_capacity(subs.len());
                            for sub in subs {
                                let sub = Value::from(sub.name);
                                values.push(sub);
                            }
                            Ok(Value::from(values))
                        },
                        Err(e) => Err(RuntimeError::Custom(e.to_string()).to_exec(command.span)),
                    }
                } else {
                    return Err(RuntimeError::Custom(format!(
                        "template name must be preceeded by a single ` (backtick)\nThis ensures if the template is renamed this {} will not be invalid",
                        GET_SUBS
                    )).to_exec(command.span));
                }
            }.boxed()
    })
}

pub const ADD_SUBS: &str = "add_subs";
pub const ADD_SUBS_RULES: &[ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), MAYBE_TEXT),
    ArgRule::new(ArgPos::Index(1), MAYBE_INDEXABLE),
];
pub fn add_subs_command<U: FunboyUserId>(funboy: Arc<Funboy<U>>) -> Handler {
    Handler::new(move |command: Command, data: Arc<InterpreterData>| {
        let funboy = funboy.clone();
        async move {
                let mut command = command;
                let mut args = command.take_args();
                let template = args.pop_front().unwrap().as_text(data.clone()).await?;
                let subs = args
                    .pop_front()
                    .unwrap()
                    .as_raw_checked(data.clone(), &[FslType::Text, FslType::List])
                    .await?;
                let regex = TemplateDelimiter::BackTick.to_regex().await;

                let subs = match subs.value {
                    Value::Text(sub) => vec![sub.into_owned()],
                    Value::List(values) => {
                        let span = subs.span;
                        let mut subs = vec![];
                        for value in values {
                            let value = value.as_text(data.clone()).await.map_err(|e| e.to_exec(span))?;
                            subs.push(value.into_owned());
                        }
                        subs
                    }
                    _ => unreachable!("as raw should have verified type"),
                };

                if regex.is_match(&template) {
                    let template = template.trim_matches('`');
                    let result = funboy.add_substitutes(template, &subs.as_strs()).await;
                    match result {
                        Ok(receipt) => Ok(Value::from(receipt.updated_to_string())),
                        Err(e) => Err(RuntimeError::Custom(format!(
                            "{}",
                            e.to_string()
                        )).to_exec(command.span)),
                    }
                } else {
                    return Err(RuntimeError::Custom(format!(
                        "template name must be preceeded by a single ` (backtick)\nThis ensures if the template is renamed this {} will not be invalid",
                        ADD_SUBS
                    )).to_exec(command.span));
                }
            }.boxed()
    })
}

pub const DELETE_SUBS: &str = "delete_subs";
pub const DELETE_SUBS_RULES: &[ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), MAYBE_TEXT),
    ArgRule::new(ArgPos::Index(1), MAYBE_INDEXABLE),
];
pub fn delete_subs_command<U: FunboyUserId>(funboy: Arc<Funboy<U>>) -> Handler {
    Handler::new(move |command: Command, data: Arc<InterpreterData>| {
        let funboy = funboy.clone();
        async move {
                let mut command = command;
                let mut args = command.take_args();
                let template = args.pop_front().unwrap().as_text(data.clone()).await?;
                let subs = args
                    .pop_front()
                    .unwrap()
                    .as_raw_checked(data.clone(), &[FslType::Text, FslType::List])
                    .await?;
                let regex = TemplateDelimiter::BackTick.to_regex().await;

                let subs = match subs.value {
                    Value::Text(sub) => vec![sub.into_owned()],
                    Value::List(values) => {
                        let span = subs.span;
                        let mut subs = vec![];
                        for value in values {
                            let value = value.as_text(data.clone()).await.map_err(|e| e.to_exec(span))?;
                            subs.push(value.into_owned());
                        }
                        subs
                    }
                    _ => unreachable!("as raw should have verified type"),
                };

                if regex.is_match(&template) {
                    let template = template.trim_matches('`');
                    let result = funboy.delete_substitutes(template, &subs.as_strs()).await;
                    match result {
                        Ok(receipt) => Ok(Value::from(receipt.updated_to_string())),
                        Err(e) => Err(RuntimeError::Custom(format!(
                            "{}",
                            e.to_string()
                        )).to_exec(command.span)),
                    }
                } else {
                    return Err(RuntimeError::Custom(format!(
                        "template name must be preceeded by a single ` (backtick)\nThis ensures if the template is renamed this {} will not be invalid",
                        ADD_SUBS
                    )).to_exec(command.span));
                }
            }.boxed()
    })
}

pub const ASK_AI: &str = "ask_ai";
pub const ASK_AI_RULES: &[ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), MAYBE_TEXT),
    ArgRule::new(ArgPos::Index(1), MAYBE_INT),
];
pub const MAX_WORD_LIMIT: i64 = 2000;
pub fn ask_ai_command<U: FunboyUserId>(funboy: Arc<Funboy<U>>) -> Handler {
    Handler::new(move |command: Command, data: Arc<InterpreterData>| {
        let funboy = funboy.clone();
        async move {
            let mut command = command;
            let mut args = command.take_args();
            let prompt = args.pop_front().unwrap().as_text(data.clone()).await?;

            let word_limit = args.pop_front().unwrap();
            let word_limit_span = word_limit.span;
            let word_limit = word_limit.as_int(data).await?;
            if word_limit <= 0 {
                return Err(RuntimeError::Custom(format!(
                    "word limit `{}` must be greater than zero",
                    word_limit
                ))
                .to_exec(word_limit_span));
            } else if word_limit > MAX_WORD_LIMIT {
                return Err(RuntimeError::Custom(format!(
                    "word limit `{}` cannot be greater than {}",
                    word_limit, MAX_WORD_LIMIT
                ))
                .to_exec(word_limit_span));
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
                    Ok(Value::from(response))
                }
                Err(e) => Err(RuntimeError::Custom(e.to_string()).to_exec(command.span)),
            }
        }
        .boxed()
    })
}
