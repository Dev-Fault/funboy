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
        command::{ArgPos, ArgRule, Command, CommandError, Executor},
        value::Value,
    },
};
use serenity::{
    all::{Cache, ChannelId, GuildId, Http, Member, Mentionable, ShardMessenger, UserId},
    futures::StreamExt,
};
use tokio::{sync::Mutex, time::sleep};

use crate::Context;

#[derive(Clone)]
pub struct InterpreterContext {
    pub http: Arc<Http>,
    #[allow(dead_code)]
    pub cache: Arc<Cache>,
    pub shard: ShardMessenger,
    pub guild_id: Option<GuildId>,
    pub channel_id: ChannelId,
    pub author_id: UserId,
}

impl InterpreterContext {
    pub fn from_poise(ctx: &Context<'_>) -> Self {
        Self {
            http: ctx.serenity_context().http.clone(),
            cache: ctx.serenity_context().cache.clone(),
            shard: ctx.serenity_context().shard.clone(),
            guild_id: ctx.guild_id(),
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
pub fn create_custom_interpreter(ctx: &Context<'_>) -> Arc<tokio::sync::Mutex<FslInterpreter>> {
    let mut interpreter = FslInterpreter::new();

    let ictx = InterpreterContext::from_poise(ctx);
    let rate_limit = Arc::new(Mutex::new(RateLimit::new(30)));

    interpreter.add_command(
        "say",
        SAY_RULES,
        create_say_command(rate_limit.clone(), ictx.clone()),
    );

    interpreter.add_command(
        "say_to",
        SAY_TO_RULES,
        create_say_to_command(rate_limit.clone(), ictx.clone()),
    );

    interpreter.add_command(
        "ask",
        ASK_RULES,
        create_ask_command(rate_limit.clone(), ictx.clone()),
    );

    Arc::new(tokio::sync::Mutex::new(interpreter))
}

const SAY_RULES: &'static [ArgRule] = &[ArgRule::new(ArgPos::Index(0), TEXT_TYPES)];
pub fn create_say_command(rate_limit: Arc<Mutex<RateLimit>>, ictx: InterpreterContext) -> Executor {
    const SAY: &str = "say";
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
    Some(Arc::new(say_command))
}

const SAY_TO_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::Index(1), TEXT_TYPES),
];
pub fn create_say_to_command(
    rate_limit: Arc<Mutex<RateLimit>>,
    ictx: InterpreterContext,
) -> Executor {
    const SAY_TO: &str = "say_to";
    const SAY_TO_LIMIT: u64 = 300;
    let say_count: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let say_command = {
        let rate_limit = rate_limit.clone();
        let ictx = ictx.clone();
        move |command: Command, interpreter_data: Arc<InterpreterData>| {
            let ictx = ictx.clone();
            let say_count = say_count.clone();
            let rate_limit = rate_limit.clone();
            async move {
                if say_count.load(Ordering::Relaxed) >= SAY_TO_LIMIT {
                    return Err(CommandError::Custom(format!(
                        "Cannot use say more than {} times in one go",
                        SAY_TO_LIMIT
                    )));
                }
                say_count.fetch_add(1, Ordering::Relaxed);

                sleep(Duration::from_millis(COMMAND_MESSAGE_DELAY_MS)).await;

                let mut values = command.take_args();
                let user = values
                    .pop_front()
                    .unwrap()
                    .as_text(interpreter_data.clone())
                    .await?;
                let message = values
                    .pop_front()
                    .unwrap()
                    .as_text(interpreter_data)
                    .await?;

                if let Some(guild_id) = ictx.guild_id {
                    if let Ok(members) = guild_id.members(ictx.http.clone(), None, None).await {
                        let mut found_member: Option<&Member> = None;
                        for member in members.iter() {
                            if member.nick.as_ref().is_some_and(|nick| nick == &user) {
                                found_member = Some(member);
                                break;
                            } else if member.display_name() == &user {
                                found_member = Some(member);
                                break;
                            }
                        }

                        let say_message = async |mention: String| {
                            let mention_message = format!("{} {}", mention, message);
                            if let Err(e) = ictx.channel_id.say(&ictx.http, mention_message).await {
                                return Err(CommandError::Custom(e.to_string()));
                            };

                            if let Err(e) = rate_limit.lock().await.check_limit(ictx.author_id) {
                                return Err(CommandError::Custom(format!(
                                    "{} on {} command",
                                    e, SAY_TO
                                )));
                            } else {
                                Ok(())
                            }
                        };

                        if let Some(user) = found_member {
                            say_message(user.mention().to_string()).await?;
                        } else if let Some(user) = members
                            .iter()
                            .find(|m| m.user.name == user || m.user.tag() == user)
                        {
                            say_message(user.mention().to_string()).await?;
                        } else if user == "everyone" {
                            say_message(user).await?;
                        } else {
                            return Err(CommandError::Custom(format!(
                                "no user named {} found",
                                user
                            )));
                        }
                    } else {
                        return Err(CommandError::Custom(format!(
                            "failed to fetch guild members",
                        )));
                    }

                    Ok(Value::None)
                } else {
                    return Err(CommandError::Custom(format!(
                        "Cannot use {} outside of a guild",
                        SAY_TO
                    )));
                }
            }
        }
    };
    Some(Arc::new(say_command))
}

const ASK_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::OptionalIndex(1), NUMERIC_TYPES),
];
pub fn create_ask_command(rate_limit: Arc<Mutex<RateLimit>>, ictx: InterpreterContext) -> Executor {
    const ASK: &str = "ask";
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
    Some(Arc::new(ask_command))
}
