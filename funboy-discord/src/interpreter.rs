use std::{sync::Arc, time::Duration};

use fsl_interpreter::{
    ErrorContext, FslError, FslInterpreter, InterpreterData,
    types::{ArgPos, ArgRule, Command, FslType, Value},
};
use serenity::{
    all::{Cache, ChannelId, Http, Mentionable, ShardMessenger, UserId},
    futures::StreamExt,
};
use tokio::{sync::Mutex, time::sleep};

use crate::{Context, io_format::context_extension::MESSAGE_DELAY_MS};

#[derive(Clone)]
pub struct InterpreterContext {
    pub http: Arc<Http>,
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

pub fn create_custom_interpreter(ctx: &Context<'_>) -> FslInterpreter {
    let mut interpreter = FslInterpreter::new();

    let ictx = InterpreterContext::from_poise(ctx);

    const SAY_RULES: &'static [ArgRule] = &[ArgRule::new(ArgPos::Index(0), &[FslType::Text])];
    const SAY_LIMIT: u8 = 100;
    let say_count: Arc<Mutex<u8>> = Arc::new(Mutex::new(0));
    let say_command = {
        move |command: Command, interpreter_data| {
            let ictx = ictx.clone();
            let say_count = say_count.clone();
            async move {
                if *say_count.lock().await >= SAY_LIMIT {
                    return Err(FslError::CustomError(ErrorContext::new(
                        "say".into(),
                        format!("Cannot use say more than {} times in one go", SAY_LIMIT),
                    )));
                }
                *say_count.lock().await += 1;

                sleep(Duration::from_millis(MESSAGE_DELAY_MS)).await;

                let mut values = command.take_args();
                let message = values
                    .pop_front()
                    .unwrap()
                    .as_text(interpreter_data)
                    .await?;
                ictx.channel_id.say(&ictx.http, message).await.ok();
                Ok(Value::None)
            }
        }
    };

    const ASK_RULES: &'static [ArgRule] = &[ArgRule::new(ArgPos::Index(0), &[FslType::Text])];
    let ictx = InterpreterContext::from_poise(&ctx);
    const ASK_LIMIT: u8 = 100;
    const ASK_DEFAULT_TIMEOUT_S: u64 = 30;
    let ask_count: Arc<Mutex<u8>> = Arc::new(Mutex::new(0));
    let ask_command = {
        move |command: Command, data: Arc<InterpreterData>| {
            let ictx = ictx.clone();
            let ask_count = ask_count.clone();
            async move {
                if *ask_count.lock().await >= ASK_LIMIT {
                    return Err(FslError::CustomError(ErrorContext::new(
                        "ask".into(),
                        format!("Cannot use ask more than {} times in one go", SAY_LIMIT),
                    )));
                }
                *ask_count.lock().await += 1;

                sleep(Duration::from_millis(MESSAGE_DELAY_MS)).await;

                let mut values = command.take_args();
                let question = format!(
                    "{} {}",
                    ictx.author_id.mention(),
                    values.pop_front().unwrap().as_text(data).await?
                );

                ictx.channel_id.say(&ictx.http, question).await.ok();

                let mut collector = ictx
                    .channel_id
                    .await_reply(ictx.shard)
                    .timeout(Duration::from_secs(ASK_DEFAULT_TIMEOUT_S))
                    .channel_id(ictx.channel_id)
                    .author_id(ictx.author_id)
                    .stream();

                if let Some(msg) = collector.next().await {
                    Ok(Value::Text(msg.content))
                } else {
                    return Err(FslError::CustomError(ErrorContext::new(
                        "ask".into(),
                        format!("Didn't receive a message before timeout ended"),
                    )));
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
