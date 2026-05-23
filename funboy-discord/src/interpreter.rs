use std::{sync::Arc, time::Duration};

use fsl_core::{FslInterpreter, error::RuntimeError};
use funboy_core::{
    format::{TWO_THOUSAND, split_message},
    interpreter::{
        ASK, ASK_RULES, ASK_TO, ASK_TO_RULES, Interactor, InterpreterContext, Messenger, SAY,
        SAY_RULES, SAY_TO, SAY_TO_RULES,
    },
};
use serenity::{
    all::{Cache, ChannelId, GuildId, Http, Member, Mentionable, Message, ShardMessenger, UserId},
    futures::StreamExt,
};

use crate::{Context, Data, DiscordUserId, context_extension::BOT_MAX_MESSAGE_SIZE};

#[derive(Clone)]
pub struct DiscordContext {
    pub http: Arc<Http>,
    #[allow(dead_code)]
    pub cache: Arc<Cache>,
    pub shard: ShardMessenger,
    pub guild_id: Option<GuildId>,
    pub channel_id: ChannelId,
    pub author_id: UserId,
}

impl Messenger for DiscordContext {
    fn say(&self, message: &str) {
        let channel_id = self.channel_id.clone();
        let http = self.http.clone();
        let message = message.to_owned();
        tokio::spawn(async move {
            channel_id.say(http, message).await.ok();
        });
    }

    fn mention(&self) -> String {
        self.author_id.mention().to_string()
    }

    fn await_response(
        &self,
        timeout: f64,
    ) -> impl std::future::Future<Output = Result<String, RuntimeError>> + Send {
        async move {
            let mut collector = self
                .channel_id
                .await_reply(self.shard.clone())
                .timeout(Duration::from_secs_f64(timeout))
                .channel_id(self.channel_id)
                .author_id(self.author_id)
                .stream();

            if let Some(msg) = collector.next().await {
                Ok(msg.content)
            } else {
                Err(RuntimeError::Custom(format!(
                    "Didn't receive a message before timeout ended"
                )))
            }
        }
    }
}

impl Interactor for DiscordContext {
    async fn say_to_user(&self, user_name: &str, message: &str) -> Result<(), RuntimeError> {
        let members = if let Some(guild_id) = self.guild_id {
            if let Ok(members) = guild_id.members(self.http.clone(), None, None).await {
                members
            } else {
                return Err(RuntimeError::Custom(format!(
                    "failed to fetch guild members",
                )));
            }
        } else {
            return Err(RuntimeError::Custom(format!("failed to get guild id",)));
        };

        let say_message = async |mention: &str| {
            if message.len() < BOT_MAX_MESSAGE_SIZE {
                let mention_message = &format!("{} {}", mention, message);
                for m in split_message(mention_message, TWO_THOUSAND) {
                    if let Err(e) = self.channel_id.say(&self.http, m).await {
                        return Err(RuntimeError::Custom(e.to_string()));
                    };
                }
            } else {
                return Err(RuntimeError::Custom(format!(
                    "Message exceeded max length of {} characters",
                    BOT_MAX_MESSAGE_SIZE,
                )));
            }

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
            return Err(RuntimeError::Custom(format!(
                "no user named {} found",
                user_name
            )));
        }
        Ok(())
    }

    fn await_user_response(
        &self,
        user_name: &str,
        timeout: f64,
    ) -> impl std::future::Future<Output = Result<String, RuntimeError>> + Send {
        async move {
            let mut collector = if user_name == "everyone" {
                self.channel_id
                    .await_reply(self.shard.clone())
                    .timeout(Duration::from_secs_f64(timeout))
                    .channel_id(self.channel_id)
                    .stream()
            } else {
                let user_id = self.get_user_id(&user_name).await?;
                self.channel_id
                    .await_reply(self.shard.clone())
                    .timeout(Duration::from_secs_f64(timeout))
                    .channel_id(self.channel_id)
                    .author_id(user_id)
                    .stream()
            };

            if let Some(msg) = collector.next().await {
                Ok(msg.content)
            } else {
                Err(RuntimeError::Custom(format!(
                    "Didn't receive a message before timeout ended"
                )))
            }
        }
    }
}

impl DiscordContext {
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

    pub fn from_serenity(ctx: &serenity::prelude::Context, message: &Message) -> Self {
        Self {
            http: ctx.http.clone(),
            cache: ctx.cache.clone(),
            shard: ctx.shard.clone(),
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            author_id: message.author.id,
        }
    }

    pub async fn get_guild_members(&self) -> Result<Vec<Member>, RuntimeError> {
        if let Some(guild_id) = self.guild_id {
            if let Ok(members) = guild_id.members(self.http.clone(), None, None).await {
                Ok(members)
            } else {
                return Err(RuntimeError::Custom(format!(
                    "failed to fetch guild members",
                )));
            }
        } else {
            return Err(RuntimeError::Custom(format!("failed to get guild id",)));
        }
    }

    pub async fn get_user_id(&self, user_name: &str) -> Result<UserId, RuntimeError> {
        let members = self.get_guild_members().await?;

        if let Some(member) = members.iter().find(|m| {
            m.user.name == user_name
                || m.user.tag() == user_name
                || m.user.display_name() == user_name
                || m.nick.as_ref().is_some_and(|nick| nick == &user_name)
        }) {
            Ok(member.user.id)
        } else {
            return Err(RuntimeError::Custom(format!(
                "no user named {} found",
                user_name
            )));
        }
    }
}

pub fn interpreter_from_poise(ctx: &Context<'_>) -> Arc<tokio::sync::Mutex<FslInterpreter>> {
    let mut interpreter = FslInterpreter::new();

    let dctx = DiscordContext::from_poise(ctx);
    let ictx = InterpreterContext::new(
        DiscordUserId(ctx.author().id),
        ctx.data().funboy.clone(),
        dctx,
        ctx.data().interpreter_limits.clone(),
    );

    interpreter.register(
        SAY,
        SAY_RULES,
        funboy_core::interpreter::say_command(ictx.clone()),
    );
    interpreter.register(
        SAY_TO,
        SAY_TO_RULES,
        funboy_core::interpreter::say_to_command(ictx.clone()),
    );
    interpreter.register(
        ASK,
        ASK_RULES,
        funboy_core::interpreter::ask_command(ictx.clone()),
    );
    interpreter.register(
        ASK_TO,
        ASK_TO_RULES,
        funboy_core::interpreter::ask_to_command(ictx.clone()),
    );

    Arc::new(tokio::sync::Mutex::new(interpreter))
}

pub fn interpreter_from_serenity(
    ctx: &serenity::prelude::Context,
    data: &Data,
    message: &Message,
) -> Arc<tokio::sync::Mutex<FslInterpreter>> {
    let mut interpreter = FslInterpreter::new();

    let dctx = DiscordContext::from_serenity(ctx, message);
    let ictx = InterpreterContext::new(
        DiscordUserId(message.author.id),
        data.funboy.clone(),
        dctx,
        data.interpreter_limits.clone(),
    );

    interpreter.register(
        SAY,
        SAY_RULES,
        funboy_core::interpreter::say_command(ictx.clone()),
    );
    interpreter.register(
        SAY_TO,
        SAY_TO_RULES,
        funboy_core::interpreter::say_to_command(ictx.clone()),
    );
    interpreter.register(
        ASK,
        ASK_RULES,
        funboy_core::interpreter::ask_command(ictx.clone()),
    );
    interpreter.register(
        ASK_TO,
        ASK_TO_RULES,
        funboy_core::interpreter::ask_to_command(ictx.clone()),
    );

    Arc::new(tokio::sync::Mutex::new(interpreter))
}
