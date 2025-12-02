use funboy_core::Funboy;

use crate::{
    Context, Error,
    io_format::{
        context_extension::ContextExtension,
        discord_message_format::split_by_whitespace_unless_quoted,
    },
};

/// Generates a random number between or including the min and max provided
#[poise::command(slash_command, prefix_command, category = "Random")]
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

/// Randomly selects an item from the list given
///
/// Entries are seperated by spaces and multi-word entries can be enclosed in quotes like "hot dog"
#[poise::command(slash_command, prefix_command, category = "Random")]
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
