use std::collections::HashMap;

use crate::{
    Context, Error,
    io_format::{context_extension::ContextExtension, discord_message_format::extract_image_urls},
};

use poise::{
    CreateReply,
    serenity_prelude::{self as serenity, ChannelId, CreateEmbed, CreateMessage},
};
#[derive(PartialEq, Eq)]
struct CommandInfo<'a> {
    pub name: &'a String,
    pub description: &'a Option<String>,
}

/// List all available commands
#[poise::command(slash_command, prefix_command, category = "Utility")]
pub async fn help(ctx: Context<'_>, show_descriptions: Option<bool>) -> Result<(), Error> {
    let commands = &ctx.framework().options().commands;

    let empty = "Miscellaneous".to_string();
    let mut help_text = String::new();
    let mut command_map = HashMap::<&str, Vec<CommandInfo>>::new();
    for command in commands {
        let command_info = CommandInfo {
            name: &command.name,
            description: &command.description,
        };
        let category = command.category.as_ref().unwrap_or(&empty).as_str();
        if !command.hide_in_help {
            if !command_map.contains_key(category) {
                command_map.insert(category, vec![]);
            }
            let commands = command_map.get_mut(category).unwrap();
            if !commands.contains(&command_info) {
                commands.push(command_info);
            }
        }
    }

    let mut keys: Vec<&&str> = command_map.keys().collect();
    keys.sort();
    for key in keys {
        help_text.push_str(&format!("**{}**\n", key));
        for value in command_map.get(key).unwrap() {
            help_text.push_str(&format!("- /{}\n", value.name));
            if show_descriptions.is_some_and(|show| show) {
                if let Some(description) = value.description.as_ref() {
                    help_text.push_str(&format!("\t- {}\n", description))
                };
            }
        }
    }

    ctx.say_long(&help_text, true).await?;

    Ok(())
}

#[poise::command(prefix_command, hide_in_help = true)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

/// Move pinned messages posted by the bot to a selected channel
///
/// Example usage: **/move_bot_pins** to_channel: **my-channel**
#[poise::command(slash_command, prefix_command, category = "Utility")]
pub async fn move_bot_pins(ctx: Context<'_>, to_channel: String) -> Result<(), Error> {
    if let Some(to_id) = get_channel_id(ctx, &to_channel).await? {
        let pins = ctx.channel_id().pins(ctx.http()).await?;
        for pin in pins {
            let bot_user = ctx.http().get_current_user().await?;
            if pin.author.name == bot_user.name {
                let mut embed = CreateEmbed::new()
                    .title(&pin.author.name)
                    .description(&pin.content)
                    .url(pin.link());

                let image_urls = extract_image_urls(&pin.content);

                if image_urls.len() == 1 {
                    embed = embed.image(image_urls[0]);
                    ctx.defer().await?;
                    to_id
                        .send_message(&ctx.http(), CreateMessage::new().embed(embed))
                        .await?;
                } else {
                    ctx.defer().await?;
                    to_id
                        .send_message(&ctx.http(), CreateMessage::new().embed(embed))
                        .await?;

                    for image_url in extract_image_urls(&pin.content) {
                        ctx.defer().await?;
                        to_id
                            .send_message(
                                &ctx.http(),
                                CreateMessage::new().embed(CreateEmbed::new().image(image_url)),
                            )
                            .await?;
                    }
                }

                pin.unpin(ctx.http()).await?;
            }
        }
        ctx.defer().await?;
        ctx.send(CreateReply::default().content(format!(
            "Succesfully moved pins to channel **{}**.",
            to_channel
        )))
        .await?;
    } else {
        ctx.say(format!(
            "Error: Could not find channel with name **{}**.",
            to_channel
        ))
        .await?;
    }
    Ok(())
}

async fn get_channel_id(ctx: Context<'_>, channel_name: &str) -> Result<Option<ChannelId>, Error> {
    match ctx.guild_id() {
        Some(guild_id) => {
            let guild = guild_id.to_partial_guild(ctx).await?;
            for (channel_id, channel) in guild.channels(ctx).await?.iter() {
                if channel.name() == channel_name {
                    return Ok(Some(*channel_id));
                }
            }
            Ok(None)
        }
        None => Ok(None),
    }
}

/// Display the age of a users account.
#[poise::command(slash_command, prefix_command, category = "Utility")]
pub async fn age(
    ctx: Context<'_>,
    #[description = "Selected user"] user: Option<serenity::User>,
) -> Result<(), Error> {
    let u = user.as_ref().unwrap_or_else(|| ctx.author());
    let response = format!("{}'s account was created at {}.", u.name, u.created_at());
    ctx.say(response).await?;
    Ok(())
}
