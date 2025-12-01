use std::{sync::Arc, time::Duration};

use fsl_interpreter::{
    FslInterpreter, InterpreterData,
    commands::{NUMERIC_TYPES, TEXT_TYPES},
    types::{
        command::{ArgPos, ArgRule, Command, CommandError, Executor},
        value::Value,
    },
};
use funboy_core::Funboy;
use serenity::{
    all::{Cache, ChannelId, GuildId, Http, Member, Mentionable, ShardMessenger, UserId},
    futures::StreamExt,
};
use tokio::{sync::Mutex, time::sleep};

use crate::{Context, rate_limiter::RateLimit};

#[derive(Clone)]
pub struct InterpreterContext {
    pub http: Arc<Http>,
    #[allow(dead_code)]
    pub cache: Arc<Cache>,
    pub shard: ShardMessenger,
    pub guild_id: Option<GuildId>,
    pub channel_id: ChannelId,
    pub author_id: UserId,
    pub funboy: Arc<Funboy>,
    pub rate_limit: Arc<Mutex<RateLimit>>,
    pub command_call_count: Arc<Mutex<u16>>,
    interpreter: Arc<Mutex<FslInterpreter>>,
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
            funboy: ctx.data().funboy.clone(),
            rate_limit: ctx.data().interpreter_rate_limit.clone(),
            command_call_count: Arc::new(Mutex::new(0)),
            interpreter: Arc::new(Mutex::new(FslInterpreter::new())),
        }
    }

    pub async fn get_guild_members(&self) -> Result<Vec<Member>, CommandError> {
        if let Some(guild_id) = self.guild_id {
            if let Ok(members) = guild_id.members(self.http.clone(), None, None).await {
                Ok(members)
            } else {
                return Err(CommandError::Custom(format!(
                    "failed to fetch guild members",
                )));
            }
        } else {
            return Err(CommandError::Custom(format!("failed to get guild id",)));
        }
    }

    pub async fn say_to_user(&self, user_name: &str, message: &str) -> Result<(), CommandError> {
        let members = self.get_guild_members().await?;

        let say_message = async |mention: &str| {
            let mention_message = format!("{} {}", mention, message);
            if let Err(e) = self.channel_id.say(&self.http, mention_message).await {
                return Err(CommandError::Custom(e.to_string()));
            };

            Ok(())
        };

        if let Some(member) = members.iter().find(|m| {
            m.user.name == user_name
                || m.user.tag() == user_name
                || m.user.display_name() == user_name
                || m.nick.as_ref().is_some_and(|nick| nick == &user_name)
        }) {
            say_message(&member.mention().to_string()).await?;
        } else if user_name == "everyone" {
            say_message("@everyone").await?;
        } else {
            return Err(CommandError::Custom(format!(
                "no user named {} found",
                user_name
            )));
        }
        Ok(())
    }

    pub async fn get_user_id(&self, user_name: &str) -> Result<UserId, CommandError> {
        let members = self.get_guild_members().await?;

        if let Some(member) = members.iter().find(|m| {
            m.user.name == user_name
                || m.user.tag() == user_name
                || m.user.display_name() == user_name
                || m.nick.as_ref().is_some_and(|nick| nick == &user_name)
        }) {
            Ok(member.user.id)
        } else {
            return Err(CommandError::Custom(format!(
                "no user named {} found",
                user_name
            )));
        }
    }

    pub async fn generate_message(&self, message: &str) -> Result<String, CommandError> {
        match self
            .funboy
            .generate(&message, self.interpreter.clone())
            .await
        {
            Ok(gen_msg) => Ok(gen_msg),
            Err(e) => {
                return Err(CommandError::Custom(e.to_string()));
            }
        }
    }
}

const COMMAND_MESSAGE_DELAY_MS: u64 = 500;
pub fn create_custom_interpreter(ctx: &Context<'_>) -> Arc<tokio::sync::Mutex<FslInterpreter>> {
    let mut interpreter = FslInterpreter::new();

    let ictx = InterpreterContext::from_poise(ctx);

    interpreter.add_command(SAY, SAY_RULES, create_say_command(ictx.clone()));
    interpreter.add_command(SAY_TO, SAY_TO_RULES, create_say_to_command(ictx.clone()));
    interpreter.add_command(ASK, ASK_RULES, create_ask_command(ictx.clone()));
    interpreter.add_command(ASK_TO, ASK_TO_RULES, create_ask_to_command(ictx.clone()));

    Arc::new(tokio::sync::Mutex::new(interpreter))
}

const MAX_CALLS: u16 = 200;
async fn check_limits(ictx: InterpreterContext) -> Result<(), CommandError> {
    let mut rate_limit = ictx.rate_limit.lock().await;
    let mut call_count = ictx.command_call_count.lock().await;
    if *call_count >= MAX_CALLS {
        *call_count = 0;
        return Err(CommandError::Custom(format!(
            "cannot use commands that send messages more than {} per generation",
            MAX_CALLS
        )));
    }
    *call_count = call_count.saturating_add(1);

    match rate_limit.check(ictx.author_id) {
        crate::rate_limiter::RateLimitResult::MaxLimitsReached => {
            return Err(CommandError::Custom(format!(
                "exceeded rate limit too many times, please wait a bit before trying again",
            )));
        }
        crate::rate_limiter::RateLimitResult::UsesPerIntervalreached => {
            std::thread::sleep(Duration::from_secs(3));
            Ok(())
        }
        crate::rate_limiter::RateLimitResult::Ok => Ok(()),
    }
}

const SAY: &str = "say";
const SAY_RULES: &'static [ArgRule] = &[ArgRule::new(ArgPos::Index(0), TEXT_TYPES)];
pub fn create_say_command(ictx: InterpreterContext) -> Executor {
    let say_command = {
        let ictx = ictx.clone();
        move |command: Command, interpreter_data| {
            let ictx = ictx.clone();
            async move {
                check_limits(ictx.clone()).await?;

                sleep(Duration::from_millis(COMMAND_MESSAGE_DELAY_MS)).await;

                let mut values = command.take_args();
                let message = values
                    .pop_front()
                    .unwrap()
                    .as_text(interpreter_data)
                    .await?;

                let message = ictx.generate_message(&message).await?;

                ictx.channel_id.say(&ictx.http, message).await.ok();

                Ok(Value::None)
            }
        }
    };
    Some(Arc::new(say_command))
}

const SAY_TO: &str = "say_to";
const SAY_TO_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::Index(1), TEXT_TYPES),
];
pub fn create_say_to_command(ictx: InterpreterContext) -> Executor {
    let say_command = {
        let ictx = ictx.clone();
        move |command: Command, interpreter_data: Arc<InterpreterData>| {
            let ictx = ictx.clone();
            async move {
                check_limits(ictx.clone()).await?;

                sleep(Duration::from_millis(COMMAND_MESSAGE_DELAY_MS)).await;

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

                ictx.say_to_user(&user_name, &ictx.generate_message(&message).await?)
                    .await?;

                Ok(Value::None)
            }
        }
    };
    Some(Arc::new(say_command))
}

const ASK: &str = "ask";
const ASK_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::OptionalIndex(1), NUMERIC_TYPES),
];
pub fn create_ask_command(ictx: InterpreterContext) -> Executor {
    const DEFAULT_TIMEOUT_SECS: f64 = 60.0 * 2.0;
    const MAX_TIMEOUT_SECS: f64 = 60.0 * 10.0;
    let ask_command = {
        move |command: Command, data: Arc<InterpreterData>| {
            let ictx = ictx.clone();
            async move {
                check_limits(ictx.clone()).await?;

                sleep(Duration::from_millis(COMMAND_MESSAGE_DELAY_MS)).await;

                let mut values = command.take_args();

                let arg_0 = values.pop_front().unwrap().as_text(data.clone()).await?;
                let arg_1 = values
                    .pop_front()
                    .unwrap_or(Value::Float(DEFAULT_TIMEOUT_SECS));

                let question = format!("{}\n{}", ictx.author_id.mention(), arg_0);
                let question = format!("{}\n\n{}", question, "(enter -STOP- to quit)");

                let time_out = arg_1.as_float(data.clone()).await?;
                validate_time_out(time_out, MAX_TIMEOUT_SECS)?;

                ictx.channel_id
                    .say(&ictx.http, ictx.generate_message(&question).await?)
                    .await
                    .ok();

                let mut collector = ictx
                    .channel_id
                    .await_reply(ictx.shard.clone())
                    .timeout(Duration::from_secs_f64(time_out))
                    .channel_id(ictx.channel_id)
                    .author_id(ictx.author_id)
                    .stream();

                if let Some(msg) = collector.next().await {
                    if msg.content == "-STOP-" {
                        Err(CommandError::Custom("User quit the program".into()))
                    } else {
                        Ok(Value::Text(ictx.generate_message(&msg.content).await?))
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

const ASK_TO: &str = "ask_to";
const ASK_TO_RULES: &'static [ArgRule] = &[
    ArgRule::new(ArgPos::Index(0), TEXT_TYPES),
    ArgRule::new(ArgPos::Index(1), TEXT_TYPES),
    ArgRule::new(ArgPos::OptionalIndex(2), NUMERIC_TYPES),
];
pub fn create_ask_to_command(ictx: InterpreterContext) -> Executor {
    const DEFAULT_TIMEOUT_SECS: f64 = 60.0 * 2.0;
    const MAX_TIMEOUT_SECS: f64 = 60.0 * 10.0;
    let ask_command = {
        move |command: Command, data: Arc<InterpreterData>| {
            let ictx = ictx.clone();
            async move {
                check_limits(ictx.clone()).await?;

                sleep(Duration::from_millis(COMMAND_MESSAGE_DELAY_MS)).await;

                let mut values = command.take_args();

                let user_name = values.pop_front().unwrap().as_text(data.clone()).await?;
                let arg_1 = values.pop_front().unwrap().as_text(data.clone()).await?;
                let arg_2 = values
                    .pop_front()
                    .unwrap_or(Value::Float(DEFAULT_TIMEOUT_SECS));

                let question = format!("{}\n{}", ictx.author_id.mention(), arg_1);
                let question = format!("{}\n\n{}", question, "(enter -STOP- to quit)");

                let time_out = arg_2.as_float(data.clone()).await?;
                validate_time_out(time_out, MAX_TIMEOUT_SECS)?;

                ictx.say_to_user(&user_name, &ictx.generate_message(&question).await?)
                    .await?;

                let mut collector = if user_name == "everyone" {
                    ictx.channel_id
                        .await_reply(ictx.shard.clone())
                        .timeout(Duration::from_secs_f64(time_out))
                        .channel_id(ictx.channel_id)
                        .stream()
                } else {
                    let user_id = ictx.get_user_id(&user_name).await?;
                    ictx.channel_id
                        .await_reply(ictx.shard.clone())
                        .timeout(Duration::from_secs_f64(time_out))
                        .channel_id(ictx.channel_id)
                        .author_id(user_id)
                        .stream()
                };

                if let Some(msg) = collector.next().await {
                    if msg.content == "-STOP-" {
                        Err(CommandError::Custom("User quit the program".into()))
                    } else {
                        Ok(Value::Text(ictx.generate_message(&msg.content).await?))
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
