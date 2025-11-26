use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    commands::{NUMERIC_TYPES, TEXT_TYPES},
    types::{
        command::{ArgPos, ArgRule, Command, CommandError},
        value::Value,
    },
};
use serenity::{
    all::{Cache, ChannelId, CreateMessage, Http, Mentionable, ShardMessenger, UserId},
    futures::StreamExt,
};
use tokio::time::sleep;

use crate::{Context, io_format::context_extension::MESSAGE_DELAY_MS};

#[derive(Clone)]
pub struct InterpreterContext {
    pub http: Arc<Http>,
    #[allow(dead_code)]
    pub cache: Arc<Cache>,
    pub shard: ShardMessenger,
    pub channel_id: ChannelId,
    pub author_id: UserId,
}

impl InterpreterContext {
    pub fn from_poise(ctx: &Context<'_>) -> Self {
        Self {
            http: ctx.serenity_context().http.clone(),
            cache: ctx.serenity_context().cache.clone(),
            shard: ctx.serenity_context().shard.clone(),
            channel_id: ctx.channel_id(),
            author_id: ctx.author().id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RateLimit {
    users: HashMap<UserId, Vec<SystemTime>>,
    uses_per_minute: usize,
}

impl RateLimit {
    pub fn new(per_minute_limit: usize) -> Self {
        Self {
            users: HashMap::new(),
            uses_per_minute: per_minute_limit,
        }
    }

    pub fn check_limit(&mut self, user_id: UserId) -> Result<(), String> {
        let now = SystemTime::now();
        let usage_window = now - Duration::from_secs(30);

        let uses = self.users.entry(user_id).or_insert_with(Vec::new);

        uses.retain(|&t| t > usage_window);

        if uses.len() >= self.uses_per_minute {
            return Err(format!(
                "Exceeded rate limit of {} uses per minute",
                self.uses_per_minute
            ));
        }

        uses.push(now);
        Ok(())
    }
}

const COMMAND_MESSAGE_DELAY_MS: u64 = 500;
pub fn create_custom_interpreter(ctx: &Context<'_>) -> FslInterpreter {
    let mut interpreter = FslInterpreter::new();

    let ictx = InterpreterContext::from_poise(ctx);
    let rate_limit = Arc::new(tokio::sync::Mutex::new(RateLimit::new(60)));

    const SAY: &str = "say";
    const SAY_RULES: &'static [ArgRule] = &[ArgRule::new(ArgPos::Index(0), TEXT_TYPES)];
    const SAY_LIMIT: u64 = 300;
    let say_count: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let say_command = {
        let rate_limit = rate_limit.clone();
        let ictx = ictx.clone();
        move |command: Command, interpreter_data| {
            let ictx = ictx.clone();
            let say_count = say_count.clone();
            let rate_limit = rate_limit.clone();
            async move {
                if say_count.load(Ordering::Relaxed) >= SAY_LIMIT {
                    return Err(CommandError::Custom(format!(
                        "Cannot use say more than {} times in one go",
                        SAY_LIMIT
                    )));
                }
                say_count.fetch_add(1, Ordering::Relaxed);

                sleep(Duration::from_millis(COMMAND_MESSAGE_DELAY_MS)).await;

                let mut values = command.take_args();
                let message = values
                    .pop_front()
                    .unwrap()
                    .as_text(interpreter_data)
                    .await?;
                ictx.channel_id.say(&ictx.http, message).await.ok();

                if let Err(e) = rate_limit.lock().await.check_limit(ictx.author_id) {
                    return Err(CommandError::Custom(format!("{} on {} command", e, SAY)));
                }

                Ok(Value::None)
            }
        }
    };

    const ASK: &str = "ask";
    const ASK_RULES: &'static [ArgRule] = &[
        ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
        ArgRule::new(ArgPos::OptionalIndex(1), NUMERIC_TYPES),
    ];
    const ASK_LIMIT: u64 = 300;
    const DEFAULT_TIMEOUT_SECS: f64 = 60.0 * 2.0;
    const MAX_TIMEOUT_SECS: f64 = 60.0 * 10.0;
    let ask_count: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let ask_command = {
        move |command: Command, data: Arc<InterpreterData>| {
            let ictx = ictx.clone();
            let ask_count = ask_count.clone();
            let rate_limit = rate_limit.clone();
            async move {
                if ask_count.load(Ordering::Relaxed) >= ASK_LIMIT {
                    return Err(CommandError::Custom(format!(
                        "Cannot use {} more than {} times in one go",
                        ASK, ASK_LIMIT
                    )));
                }
                ask_count.fetch_add(1, Ordering::Relaxed);

                sleep(Duration::from_millis(COMMAND_MESSAGE_DELAY_MS)).await;

                let mut values = command.take_args();
                let question = values.pop_front().unwrap().as_text(data.clone()).await?;
                let timeout = if let Some(value) = values.pop_front() {
                    let timeout = value.as_float(data).await?;
                    if !timeout.is_finite() {
                        return Err(CommandError::NonFiniteValue);
                    } else if timeout.is_sign_negative() {
                        return Err(CommandError::Custom(format!(
                            "timeout cannot be a negative number"
                        )));
                    } else if timeout > MAX_TIMEOUT_SECS {
                        return Err(CommandError::Custom(format!(
                            "timeout cannot be greater than {} seconds",
                            MAX_TIMEOUT_SECS
                        )));
                    }
                    timeout
                } else {
                    DEFAULT_TIMEOUT_SECS
                };

                let question = format!("{}\n{}", ictx.author_id.mention(), question);
                let question = format!("{}\n\n{}", question, "(enter -STOP- to quit)");

                ictx.channel_id.say(&ictx.http, question).await.ok();

                let mut collector = ictx
                    .channel_id
                    .await_reply(ictx.shard)
                    .timeout(Duration::from_secs_f64(timeout))
                    .channel_id(ictx.channel_id)
                    .author_id(ictx.author_id)
                    .stream();

                if let Err(e) = rate_limit.lock().await.check_limit(ictx.author_id) {
                    return Err(CommandError::Custom(format!("{} on {} command", e, ASK)));
                }

                if let Some(msg) = collector.next().await {
                    if msg.content == "-STOP-" {
                        Err(CommandError::Custom("User quit the program".into()))
                    } else {
                        Ok(Value::Text(msg.content))
                    }
                } else {
                    Err(CommandError::Custom(format!(
                        "Didn't receive a message before timeout ended"
                    )))
                }
            }
        }
    };

    interpreter.add_command(
        "say",
        SAY_RULES,
        FslInterpreter::construct_executor(say_command),
    );

    interpreter.add_command(
        "ask",
        ASK_RULES,
        FslInterpreter::construct_executor(ask_command),
    );

    interpreter
}
