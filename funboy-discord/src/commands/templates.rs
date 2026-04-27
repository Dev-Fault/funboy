use funboy_core::{
    commands::{self, CommandResult},
    database::Platform,
    format::{
        ListStyle, ONE_HUNDRED, TWO_THOUSAND, TruncateEllipsize, parse_bot_args, split_message,
    },
};
use poise::{ChoiceParameter, CreateReply};
use serenity::all::{Attachment, ComponentInteraction, CreateAttachment};

use crate::{
    Context, DiscordUserId, Error,
    components::{
        CANCEL_BUTTON_ID, CONFIRM_BUTTON_ID, create_confirmation_interaction, edit_interaction,
    },
    context_extension::ContextExtension,
    interpreter::create_custom_interpreter,
};

pub async fn generate_message(
    ctx: Context<'_>,
    message: String,
    ollama: bool,
) -> Result<(), Error> {
    let start = std::time::Instant::now();
    let user_id = ctx.author().id;
    let http = ctx.http();
    let channel_id = ctx.channel_id();
    let funboy = ctx.data().funboy.clone();
    let interpreter = create_custom_interpreter(&ctx);

    ctx.defer().await?;

    let result = funboy
        .generate_command(
            Platform::Discord,
            DiscordUserId(user_id),
            interpreter,
            vec![message],
            false,
            ollama,
        )
        .await;

    // Don't use ctx if the webhook token expired or is close to expiring
    let ctx_window_over = start.elapsed() > std::time::Duration::from_secs(60 * 10);

    match result {
        Ok(result) => match result {
            commands::CommandResult::Text(output) => {
                if !output.trim().is_empty() {
                    if ctx_window_over {
                        for m in split_message(&output, TWO_THOUSAND) {
                            channel_id.say(&http, m).await?;
                        }
                    } else {
                        ctx.say_long(&output, false).await?;
                    }
                } else {
                    if ctx_window_over {
                        return Ok(());
                    } else {
                        ctx.say_ephemeral("Generation Complete").await?;
                    }
                }
            }
            commands::CommandResult::None => {}
        },
        Err(e) => {
            eprintln!("{:?}", e);
            if ctx_window_over {
                channel_id.say(&http, &e.to_string()).await?;
            } else {
                ctx.say_ephemeral(&e.to_string()).await?;
            }
        }
    }

    Ok(())
}

/// Generates text by preforming template substitution and interpreting fsl code
///
/// ## Templates
/// Templates are any text preceded or optionally followed by a template character.
/// The character `^` replaces the template with a random substitute.
///
/// **Examples:** `^noun` `^noun^` `^verb^ed` (note: `verb` is the template, `ed` is not)
///
/// **Given templates:**
/// - `noun`: "fox", "dog"
/// - `adj`: "quick", "lazy"
/// - `color`: "brown"
/// - `verb`: "jump"
///
/// **Example:** `/generate The ^adj ^color ^noun ^verb^ed over the ^adj ^noun`
/// - Possible output: "The quick brown fox jumped over the lazy dog"
/// - Possible output: "The lazy brown dog jumped over the quick fox"
/// ## Template aliases
/// The character `+` replaces the template with a random substitute **once** — all subsequent uses refer to the same substitute.
///
/// **Examples:** `+name` `+name+` `+name-1` `+name-1+` (aliases defined with `-`)
///
/// **Example:** `/generate +name-1 is female. +name-2 is male. +name-1 is short. +name-2 is tall.`
/// - Possible output: "Jane is female. John is male. Jane is short. John is tall."
/// ## Embedded code
/// Code between `{}` is executed as FSL (Funboy Scripting Language) code.
///
/// **Example:** `/generate The following text is reversed: {print(reverse("reversed"))}`
/// - Output: "The following text is reversed: desrever"
///
/// For more FSL information, use `/help_fsl`
#[poise::command(slash_command, prefix_command, category = "Templates")]
pub async fn generate(ctx: Context<'_>, input: String) -> Result<(), Error> {
    generate_message(ctx, input, false).await
}

/// Generates text by preforming template substitution and intepreting fsl code from a file
///  
/// See /generate command for more info
#[poise::command(slash_command, prefix_command, category = "Templates")]
pub async fn generate_file(ctx: Context<'_>, file: Attachment) -> Result<(), Error> {
    let bytes = file.download().await?;
    let input = String::from_utf8(bytes);

    let input = match input {
        Ok(i) => i,
        Err(_) => {
            ctx.say_ephemeral("Only text files are allowed (file must be valid UTF-8).")
                .await?;
            return Ok(());
        }
    };

    generate_message(ctx, input, false).await
}

/// Adds substitutes to a template
///
///
/// Substitutes are space-separated words or quoted phrases. Use quotes for multi-word substitutes.
///
/// **Examples:**
/// - `/add_subs noun cat dog bird` — adds three single-word substitutes
/// - `/add_subs noun "hot dog" "cold pizza"` — adds two multi-word substitutes
///
/// ## Single substitute mode
/// Use `single: true` for large or complex substitutes, especially those containing quotes.
///
/// **Example:** `/add_subs | template: quote | subs: this substitute contains "a quote" in it | single: true` - adds a single substitute with quotes inside
///
/// This treats the entire input as a single substitute allowing spaces and quotes inside the substitute.
#[poise::command(slash_command, prefix_command, category = "Templates")]
pub async fn add(
    ctx: Context<'_>,
    template: String,
    substitutes: String,
    single: Option<bool>,
) -> Result<(), Error> {
    let single = single.unwrap_or(false);
    let funboy = ctx.data().funboy.clone();
    let result = funboy
        .add_command(
            DiscordUserId(ctx.author().id),
            Platform::Discord,
            template,
            substitutes,
            single,
        )
        .await;

    match result {
        Ok(result) => {
            if let CommandResult::Text(output) = result {
                ctx.say_long(&output, false).await?;
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }
    Ok(())
}

/// Deletes substitutes from a template
///
///
/// Substitutes can be deleted by name or by ID, space-separated.
///
/// ## Delete by name
/// - **Example:** `/delete_subs noun cat dog` — removes "cat" and "dog" from the `noun` template
/// - **Example:** `/delete_subs name "hot dog"` — removes the "hot dog" substitute
///
/// ## Delete by ID
/// - **Example:** `/delete_subs template: noun | subs: 0 2 5 | id: true` — removes substitutes with IDs: 0, 2, and 5
///
/// This is useful when substitutes are large and difficult to write out fully inside the command.
/// Note: IDs of substitutes can be obtained by using the `/list_subs` command with the ID list style.
///
/// ## Single substitute mode
/// Use `single: true` to treat the entire input as a single substitute name or ID.
/// Useful for complex substitute names containing spaces or quotes.
///
/// **Example:** `/delete_subs template: sentence | subs: This is one substitute containing "spaces and quotes inside it" | single: true`
#[poise::command(slash_command, prefix_command, category = "Templates")]
pub async fn delete(
    ctx: Context<'_>,
    template: String,
    substitutes: String,
    single: Option<bool>,
    id: Option<bool>,
) -> Result<(), Error> {
    let single = single.unwrap_or(false);
    let id = id.unwrap_or(false);
    let funboy = ctx.data().funboy.clone();

    let result = funboy
        .delete_command(
            DiscordUserId(ctx.author().id),
            Platform::Discord,
            template,
            substitutes,
            single,
            id,
        )
        .await;

    match result {
        Ok(result) => {
            if let CommandResult::Text(output) = result {
                ctx.say_long(&output, false).await?;
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }
    Ok(())
}

/// Adds a single substitute from a file
///
/// **Example:** `/upload_sub essay [essay.txt]` — uploads file `essay.txt` and adds it as a single substitute to the `essay`
#[poise::command(slash_command, prefix_command, category = "Templates")]
pub async fn upload_sub(
    ctx: Context<'_>,
    template: String,
    #[description = "Upload a text file"] sub_file: Attachment,
) -> Result<(), Error> {
    let bytes = sub_file.download().await?;
    let sub = String::from_utf8(bytes);

    let sub = match sub {
        Ok(s) => s,
        Err(_) => {
            ctx.say_ephemeral("Only text files are allowed (file must be valid UTF-8).")
                .await?;
            return Ok(());
        }
    };

    let result = ctx.data().funboy.add_substitutes(&template, &[&sub]).await;
    match result {
        Ok(_) => {
            ctx.say(&format!(
                "Added substitute from file {} to `{}`",
                &sub_file.filename.truncate_with_ellipse(ONE_HUNDRED),
                template
            ))
            .await?;
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }

    Ok(())
}

/// Copies all substitutes from one template to another
///
/// **Example:** `/copy_subs food noun` — copies all substitutes from `food` to `noun`
#[poise::command(slash_command, prefix_command, category = "Templates")]
pub async fn copy_subs(
    ctx: Context<'_>,
    from_template: String,
    to_template: String,
) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let result = funboy
        .copy_command(DiscordUserId(ctx.author().id), from_template, to_template)
        .await;
    match result {
        Ok(result) => {
            if let CommandResult::Text(output) = result {
                ctx.say(&output).await?;
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }
    Ok(())
}

/// Replaces a substitute in a template with another value
///
/// Substitutes can be replaced by name or by ID.
///
/// ## Replace by name
/// - **Example:** `/replace_sub noun cat dog` — replaces the "cat" substitute with "dog"
/// - **Example:** `/replace_sub name "hot dog" "cold pizza"` — replaces "hot dog" with "cold pizza"
///
/// ## Replace by ID
/// - **Example:** `/replace_sub noun 0 "new substitute" id: true` — replaces the substitute with id 0
/// Note: ID's of substitutes can be obtained by using the `/list_subs` command with the ID list style.
#[poise::command(slash_command, prefix_command, category = "Templates")]
pub async fn replace(
    ctx: Context<'_>,
    template: Option<String>,
    id: Option<bool>,
    from: String,
    to: String,
) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let result = funboy
        .replace_command(
            DiscordUserId(ctx.author().id),
            template,
            from,
            to,
            id.unwrap_or(false),
        )
        .await;
    match result {
        Ok(result) => {
            if let CommandResult::Text(output) = result {
                ctx.say_long(&output, false).await?;
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }
    Ok(())
}

async fn delete_multiple_templates(
    ctx: Context<'_>,
    templates_to_delete: &[&str],
    interaction: &ComponentInteraction,
) -> Result<(), Error> {
    match ctx
        .data()
        .funboy
        .delete_templates(templates_to_delete)
        .await
    {
        Ok(result) => {
            if result.updated.len() > 0 {
                edit_interaction(
                    ctx,
                    &interaction,
                    &format!(
                        "Deleted templates `{}`",
                        &result.updated_to_string().truncate_with_ellipse(1000)
                    ),
                    true,
                )
                .await?;
            }
            if result.ignored.len() > 0 {
                edit_interaction(
                    ctx,
                    &interaction,
                    &format!(
                        "Templates `{}` do not exist.",
                        &result.ignored_to_string().truncate_with_ellipse(1000)
                    ),
                    true,
                )
                .await?;
            }
            Ok(())
        }
        Err(e) => {
            edit_interaction(ctx, &interaction, e.to_string().as_str(), true).await?;
            Ok(())
        }
    }
}

async fn delete_single_template(
    ctx: Context<'_>,
    template: &str,
    interaction: &ComponentInteraction,
) -> Result<(), Error> {
    match ctx.data().funboy.delete_template(&template).await {
        Ok(result) => match result {
            Some(_) => {
                edit_interaction(
                    ctx,
                    &interaction,
                    &format!(
                        "Deleted template `{}`",
                        template.truncate_with_ellipse(1000)
                    ),
                    true,
                )
                .await?;
                Ok(())
            }
            None => {
                edit_interaction(
                    ctx,
                    &interaction,
                    &format!(
                        "Template `{}` does not exist.",
                        template.truncate_with_ellipse(1000)
                    ),
                    true,
                )
                .await?;
                Ok(())
            }
        },
        Err(e) => {
            edit_interaction(ctx, &interaction, e.to_string().as_str(), true).await?;
            Ok(())
        }
    }
}

/// Deletes a template or templates
///
/// Template names are space-separated.
///
/// **Example:** `/delete_templates noun verb adjective` — deletes all three templates
///
/// This action cannot be undone.
#[poise::command(slash_command, prefix_command, category = "Templates")]
pub async fn delete_templates(ctx: Context<'_>, names: String) -> Result<(), Error> {
    let templates = parse_bot_args(&names);
    let templates = match templates {
        Ok(templates) => templates,
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
            return Ok(());
        }
    };

    let interaction_text = format!(
        "Are you sure you want to delete `{}`? All of {} substitutes will be deleted as well.",
        &names.truncate_with_ellipse(1000),
        if templates.len() > 1 { "their" } else { "it's" }
    );

    match create_confirmation_interaction(ctx, &interaction_text, 30, false).await? {
        Some(interaction) => match interaction.data.custom_id.as_str() {
            CANCEL_BUTTON_ID => {
                interaction
                    .create_response(
                        ctx.http(),
                        serenity::all::CreateInteractionResponse::Acknowledge,
                    )
                    .await?;

                edit_interaction(
                    ctx,
                    &interaction,
                    "Command to remove templates canceled.",
                    true,
                )
                .await?;

                Ok(())
            }
            CONFIRM_BUTTON_ID => {
                interaction
                    .create_response(
                        ctx.http(),
                        serenity::all::CreateInteractionResponse::Acknowledge,
                    )
                    .await?;
                if templates.len() > 1 {
                    delete_multiple_templates(ctx, &templates, &interaction).await?;
                } else {
                    delete_single_template(ctx, &names, &interaction).await?;
                };

                Ok(())
            }
            _ => {
                panic!("Incorrect id for remove template confirmation interaction.")
            }
        },
        None => {
            ctx.say_ephemeral("Timeout: Command to remove template canceled.")
                .await?;
            Ok(())
        }
    }
}

/// Renames a template
///
/// **Example:** `/rename_template noun thing` — renames the `noun` template to `thing`
///
/// All substitutes under the previous name will now be under the new name
#[poise::command(slash_command, prefix_command, category = "Templates")]
pub async fn rename_template(ctx: Context<'_>, from: String, to: String) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let result = funboy
        .rename_command(DiscordUserId(ctx.author().id), from, to)
        .await;
    match result {
        Ok(result) => {
            if let CommandResult::Text(output) = result {
                ctx.say(&output).await?;
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceParameter)]
pub enum DiscordListStyle {
    Default,
    Numeric,
    Id,
    File,
}

impl Into<ListStyle> for DiscordListStyle {
    fn into(self) -> ListStyle {
        match self {
            DiscordListStyle::Default => ListStyle::CommaSeparatedBlocks,
            DiscordListStyle::Numeric => ListStyle::Numeric,
            DiscordListStyle::Id => ListStyle::Id,
            DiscordListStyle::File => ListStyle::Id,
        }
    }
}

/// Lists all substitutes in a template
///
/// **Example:** `/list_subs noun` — displays all substitutes for the `noun` template
///
/// ## Search
/// Use `search_term` to filter results.
///
/// **Example:** `/list_subs noun search_term: dog` — shows only substitutes containing "dog"
///
/// ## List styles
/// - `Default` — standard comma separated format
/// - `Numeric` — numbered list
/// - `ID` — shows substitute IDs
/// - `File` — uploads text file containing substitutes and their IDs
///
/// **Example:** `/list_subs noun list_style: ID` — displays substitutes with their IDs
#[poise::command(slash_command, prefix_command, category = "Templates")]
pub async fn list_subs(
    ctx: Context<'_>,
    template: String,
    search_term: Option<String>,
    list_style: Option<DiscordListStyle>,
) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let list_style = list_style.unwrap_or(DiscordListStyle::Default);
    let result = funboy
        .list_command(Some(template), search_term, list_style.into())
        .await;
    match result {
        Ok(result) => {
            if let CommandResult::Text(output) = result {
                if matches!(list_style, DiscordListStyle::File) {
                    ctx.send(
                        CreateReply::default()
                            .attachment(CreateAttachment::bytes(output, "message.txt")),
                    )
                    .await?;
                } else {
                    ctx.say_long(&output, false).await?;
                }
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }
    Ok(())
}

/// Lists all templates
///
/// **Example:** `/list_templates` — displays all templates
///
/// ## Search
/// Use `search_term` to filter results.
///
/// **Example:** `/list_templates search_term: noun` — shows only templates containing "noun"
///
/// ## List styles
/// - `Default` — standard format
/// - `Numeric` — numbered list
/// - `ID` — shows template IDs
/// - `File` — uploads text file containing substitutes and their IDs
///
/// **Example:** `/list_templates list_style: ID` — displays templates with their IDs
#[poise::command(slash_command, prefix_command, category = "Templates")]
pub async fn list_templates(
    ctx: Context<'_>,
    search_term: Option<String>,
    list_style: Option<DiscordListStyle>,
) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let list_style = list_style.unwrap_or(DiscordListStyle::Default);
    let result = funboy
        .list_command(None, search_term, list_style.into())
        .await;
    match result {
        Ok(result) => {
            if let CommandResult::Text(output) = result {
                if matches!(list_style, DiscordListStyle::File) {
                    ctx.send(
                        CreateReply::default()
                            .attachment(CreateAttachment::bytes(output, "message.txt")),
                    )
                    .await?;
                } else {
                    ctx.say_long(&output, false).await?;
                }
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }
    Ok(())
}
