use std::collections::HashMap;

use crate::{
    Context, Error,
    io_format::{context_extension::ContextExtension, discord_message_format::extract_image_urls},
};

use funboy_core::interpreter::documentation::get_command_documentation;
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

/// Display help information for commands
#[poise::command(slash_command, prefix_command, category = "Utility")]
pub async fn fsl_help(ctx: Context<'_>, command_name: Option<String>) -> Result<(), Error> {
    match command_name {
        Some(command_name) => {
            if let Some(command) = get_command_documentation()
                .iter()
                .find(|info| info.name == command_name)
            {
                let mut examples = command
                    .examples
                    .iter()
                    .map(|example| "**".to_string() + example + "**\n")
                    .collect::<String>();

                examples.pop();

                let embed = CreateEmbed::new()
                    .title(command.name.clone())
                    .description(format!(
                        "Description: {}\n\nExamples:\n{}",
                        command.description, examples
                    ));

                ctx.send(CreateReply::default().embed(embed).ephemeral(true))
                    .await?;
            } else {
                ctx.say_ephemeral(&format!("No command named **{}** was found.", command_name))
                    .await?;
            }
        }
        None => {
            let title = "FSL - Funboy Scripting Language";
            let description = "The FSL language is a simple scripting language that you can embed in text when using the /generate command to manipulate text.\n
        When using the **/generate** command to indicate you are using FSL commands place any code inside of curly braces such as in this example: **{print(\"Hello, world!\")}**\n
        The generate command will then interpret the code inside the curly braces and replace the curly braces and text within with the generated output.\n
        As an example: **/generate Hello {print(\", world!\")}** will output: **Hello, World!**\n
        The FSL language recognizes a few different types of data: Int (whole number), Float (decimal number), Text (indicated by surrounding data in quotes), Identifier (text that is not surrounded by quotes), and List which are an aggregate of any of the preceeding types and can be created using the Copy command.\n
        To use the language familiarize yourself with the commands by typing **/help_fsl command_name** to get more information on a specific command.";

            let mut list_of_commands: String = get_command_documentation()
                .iter()
                .map(|info| info.name.to_string() + ", ")
                .collect::<String>();

            list_of_commands.pop();
            list_of_commands.pop();

            let embed = CreateEmbed::new()
                .title(title)
                .description(format!("{}\n\nCommands: {}", description, list_of_commands));

            ctx.send(CreateReply::default().embed(embed).ephemeral(true))
                .await?;
        }
    }

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
