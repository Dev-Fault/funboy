use funboy_core::Funboy;

use crate::{
    Context, Error,
    io_format::{
        context_extension::ContextExtension,
        discord_message_format::split_by_whitespace_unless_quoted,
    },
};

#[poise::command(slash_command, prefix_command)]
pub async fn random_number(ctx: Context<'_>, min: String, max: String) -> Result<(), Error> {
    let number = Funboy::random_number(&min, &max, false);
    match number {
        Ok(number) => {
            ctx.say(number).await?;
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn random_entry(ctx: Context<'_>, entries: String) -> Result<(), Error> {
    let entries = split_by_whitespace_unless_quoted(&entries);
    let entry = Funboy::random_entry(&entries);
    match entry {
        Ok(entry) => {
            ctx.say(entry).await?;
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }
    Ok(())
}
