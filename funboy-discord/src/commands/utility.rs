use std::collections::HashMap;

use crate::{
    Context, Error,
    io_format::{
        context_extension::ContextExtension,
        discord_message_format::{DISCORD_CHARACTER_LIMIT, extract_image_urls},
    },
};

use poise::{
    CreateReply,
    serenity_prelude::{self as serenity, ChannelId, CreateEmbed, CreateMessage},
};
use tokio::sync::OnceCell;
#[derive(PartialEq, Eq)]
struct CommandInfo<'a> {
    pub name: &'a String,
    pub description: &'a Option<String>,
    pub help_text: &'a Option<String>,
}

static HELP_MESSAGES: OnceCell<Vec<String>> = OnceCell::const_new();
static HELP_MESSAGES_WITH_DESCRIPTIONS: OnceCell<Vec<String>> = OnceCell::const_new();

async fn generate_help_messages<'a>(ctx: Context<'_>, show_descriptions: bool) -> Vec<String> {
    let commands = &ctx.framework().options().commands;
    let mut command_map = HashMap::<&str, Vec<CommandInfo>>::new();
    for command in commands {
        let command_info = CommandInfo {
            name: &command.name,
            description: &command.description,
            help_text: &command.help_text,
        };

        if let Some(category) = &command.category {
            let category = category.as_str();
            if !command_map.contains_key(category) {
                command_map.insert(category, vec![]);
            }
            let commands = command_map.get_mut(category).unwrap();
            if !commands.contains(&command_info) {
                commands.push(command_info);
            }
        }
    }

    let mut help_messages: Vec<String> = Vec::new();
    help_messages.push(String::new());
    let mut msg_i = 0;
    let mut keys: Vec<&&str> = command_map.keys().collect();
    keys.sort();
    for key in keys {
        let mut help_message = String::new();
        help_message.push_str(&format!("**{}**\n", key));

        for value in command_map.get(key).unwrap() {
            help_message.push_str(&format!("- /{}\n", value.name));

            if show_descriptions {
                if let Some(description) = value.description.as_ref() {
                    help_message.push_str(&format!("\t- {}\n", description));
                };
            }
        }

        if help_message.len() > DISCORD_CHARACTER_LIMIT {
            let mut messages: Vec<String> = Vec::new();
            messages.push(String::new());
            let mut i = 0;
            for line in help_message.split_inclusive('\n') {
                if line.len() + messages[i].len() < DISCORD_CHARACTER_LIMIT {
                    let msg = messages.get_mut(i).unwrap();
                    msg.push_str(line);
                } else {
                    messages.push(line.to_string());
                    i += 1;
                }
            }

            for msg in messages {
                help_messages.push(msg);
            }
        } else if help_messages[msg_i].len() + help_message.len() > DISCORD_CHARACTER_LIMIT {
            help_messages.push(help_message);
            msg_i += 1;
        } else {
            let msg = help_messages.get_mut(msg_i).unwrap();
            msg.push_str(&help_message);
        }
    }

    help_messages
}

/// Lists out all available commands optionally showing their descriptions
#[poise::command(slash_command, prefix_command, category = "Utility")]
pub async fn help(ctx: Context<'_>, show_descriptions: Option<bool>) -> Result<(), Error> {
    let show_descriptions = show_descriptions.unwrap_or(false);
    let help_messages = if show_descriptions {
        HELP_MESSAGES_WITH_DESCRIPTIONS
            .get_or_init(|| generate_help_messages(ctx, true))
            .await
    } else {
        HELP_MESSAGES
            .get_or_init(|| generate_help_messages(ctx, false))
            .await
    };

    for message in help_messages {
        ctx.say_ephemeral(&message).await?;
    }

    ctx.say_ephemeral("Use `/help_command` for more detailed information on a command")
        .await?;

    Ok(())
}

/// Get detailed information on an individual command
#[poise::command(slash_command, prefix_command, category = "Utility")]
pub async fn help_command(ctx: Context<'_>, command: String) -> Result<(), Error> {
    let commands = &ctx.framework().options().commands;
    match commands.iter().find(|c| c.name == command) {
        Some(command) => {
            if command
                .help_text
                .as_ref()
                .is_some_and(|text| !text.is_empty())
            {
                ctx.say_long(
                    &format!(
                        "# {}\n{}\n{}",
                        command.name,
                        command
                            .description
                            .as_ref()
                            .unwrap_or(&format!("No description available for {}.", command.name)),
                        command.help_text.as_ref().unwrap()
                    ),
                    true,
                )
                .await?;
            } else {
                ctx.say_ephemeral(&format!(
                    "{}",
                    command.description.as_ref().unwrap_or(&format!(
                        "No available information for command {}.",
                        command.name
                    ))
                ))
                .await?;
            }
            Ok(())
        }
        None => {
            ctx.say_ephemeral(&format!("No command named {} exists", command))
                .await?;
            Ok(())
        }
    }
}

#[poise::command(prefix_command)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

/// Moves pinned bot messages to the selected channel and creates an embed for them
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
