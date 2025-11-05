use funboy_core::template_database::{Limit, OrderBy, SortOrder};
use poise::ChoiceParameter;
use serenity::all::{ComponentInteraction, EditInteractionResponse};

use crate::{
    Context, Error,
    components::{CANCEL_BUTTON_ID, CONFIRM_BUTTON_ID, create_confirmation_interaction},
    io_format::{
        context_extension::ContextExtension,
        discord_message_format::{
            ellipsize_if_long, format_as_item_seperated_list, format_as_numeric_list,
            split_by_whitespace_unless_quoted,
        },
    },
};

#[poise::command(slash_command, prefix_command)]
pub async fn generate(ctx: Context<'_>, input: String) -> Result<(), Error> {
    let output = ctx.data().get_funboy().generate(&input).await;

    match output {
        Ok(output) => {
            ctx.say_long(&output, false).await?;
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    };
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn delete_template(ctx: Context<'_>, template: String) -> Result<(), Error> {
    async fn edit_response(
        ctx: Context<'_>,
        interaction: ComponentInteraction,
        content: &str,
    ) -> Result<(), Error> {
        interaction
            .edit_response(
                ctx.http(),
                EditInteractionResponse::new()
                    .content(content)
                    .components(vec![]),
            )
            .await?;
        Ok(())
    }

    let interaction_text = format!(
        "Are you sure you want to delete `{}` all of it's substitutes will be deleted as well?",
        template
    );

    match create_confirmation_interaction(ctx, &interaction_text, 30).await? {
        Some(interaction) => match interaction.data.custom_id.as_str() {
            CANCEL_BUTTON_ID => {
                interaction
                    .create_response(
                        ctx.http(),
                        serenity::all::CreateInteractionResponse::Acknowledge,
                    )
                    .await?;

                edit_response(ctx, interaction, "Command to remove template canceled.").await?;

                Ok(())
            }
            CONFIRM_BUTTON_ID => {
                interaction
                    .create_response(
                        ctx.http(),
                        serenity::all::CreateInteractionResponse::Acknowledge,
                    )
                    .await?;

                match ctx.data().funboy.delete_template(&template).await {
                    Ok(result) => match result {
                        Some(_) => {
                            edit_response(
                                ctx,
                                interaction,
                                &format!("Deleted template `{}`", template),
                            )
                            .await?;
                        }
                        None => {
                            edit_response(
                                ctx,
                                interaction,
                                &format!("Template `{}` does not exist.", template),
                            )
                            .await?;
                        }
                    },
                    Err(e) => {
                        edit_response(ctx, interaction, e.to_string().as_str()).await?;
                    }
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

#[poise::command(slash_command, prefix_command)]
pub async fn rename_template(ctx: Context<'_>, from: String, to: String) -> Result<(), Error> {
    match ctx.data().funboy.rename_template(&from, &to).await {
        Ok(template) => match template {
            Some(_) => {
                ctx.say(&format!("Renamed template `{}` to `{}`", from, to))
                    .await?;
            }
            None => {
                ctx.say(&format!("Failed to rename template `{}`", from,))
                    .await?;
            }
        },
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    };
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn replace_sub(
    ctx: Context<'_>,
    template: String,
    from: String,
    to: String,
) -> Result<(), Error> {
    match ctx
        .data()
        .funboy
        .replace_substitute(&template, &from, &to)
        .await
    {
        Ok(template) => match template {
            Some(_) => {
                ctx.say_long(
                    &format!(
                        "Renamed substitute `{}` to `{}`",
                        ellipsize_if_long(&from, 255),
                        ellipsize_if_long(&to, 255)
                    ),
                    false,
                )
                .await?;
            }
            None => {
                ctx.say_long(
                    &format!(
                        "Failed to rename substitute `{}`",
                        ellipsize_if_long(&from, 255)
                    ),
                    true,
                )
                .await?;
            }
        },
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    };
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn add_subs(
    ctx: Context<'_>,
    template: String,
    subs: String,
    add_as_single_sub: Option<bool>,
) -> Result<(), Error> {
    let result = if add_as_single_sub.is_some_and(|is_true| is_true) {
        ctx.data()
            .get_funboy()
            .add_substitutes(&template, &[&subs])
            .await
    } else {
        let subs: Vec<&str> = split_by_whitespace_unless_quoted(&subs);
        ctx.data()
            .get_funboy()
            .add_substitutes(&template, &subs)
            .await
    };

    match result {
        Ok(sub_record) => {
            if sub_record.updated.len() > 0 {
                let subs: Vec<&str> = sub_record.updated.iter().map(|s| s.name.as_str()).collect();
                let appended_text = format!("\nadded to `{}`", template);

                ctx.say_list(
                    &subs,
                    true,
                    Some(Box::new(move |subs| {
                        format_as_item_seperated_list(subs, &appended_text)
                    })),
                )
                .await?;
            }

            if sub_record.ignored.len() > 0 {
                let appended_text = format!("\nalready in `{}`", template);

                ctx.say_list(
                    &sub_record.ignored,
                    true,
                    Some(Box::new(move |items| {
                        format_as_item_seperated_list(items, &appended_text)
                    })),
                )
                .await?;
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    };
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn copy_subs(
    ctx: Context<'_>,
    from_template: String,
    to_template: String,
) -> Result<(), Error> {
    let result = ctx
        .data()
        .get_funboy()
        .copy_substitutes(&from_template, &to_template)
        .await;

    match result {
        Ok(_) => {
            ctx.say_ephemeral(&format!(
                "Copied substitutes from `{}` to `{}`",
                from_template, to_template
            ))
            .await?;
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    };
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn delete_subs(
    ctx: Context<'_>,
    template: String,
    subs: String,
    delete_as_single_sub: Option<bool>,
) -> Result<(), Error> {
    let result = if delete_as_single_sub.is_some_and(|is_true| is_true) {
        ctx.data()
            .get_funboy()
            .delete_substitutes(&template, &[&subs])
            .await
    } else {
        let subs: Vec<&str> = split_by_whitespace_unless_quoted(&subs);
        ctx.data()
            .get_funboy()
            .delete_substitutes(&template, &subs)
            .await
    };

    match result {
        Ok(sub_record) => {
            if sub_record.updated.len() > 0 {
                let subs: Vec<&str> = sub_record.updated.iter().map(|s| s.name.as_str()).collect();
                let appended_text = format!("\ndeleted from `{}`", template);

                ctx.say_list(
                    &subs,
                    true,
                    Some(Box::new(move |subs| {
                        format_as_item_seperated_list(subs, &appended_text)
                    })),
                )
                .await?;
            }

            if sub_record.ignored.len() > 0 {
                let appended_text = format!("\nnot present in `{}`", template);

                ctx.say_list(
                    &sub_record.ignored,
                    true,
                    Some(Box::new(move |items| {
                        format_as_item_seperated_list(items, &appended_text)
                    })),
                )
                .await?;
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    };
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceParameter)]
pub enum ListStyle {
    Default,
    Numeric,
}

#[poise::command(slash_command, prefix_command)]
pub async fn list_subs(
    ctx: Context<'_>,
    template: String,
    search_term: Option<String>,
    list_style: Option<ListStyle>,
) -> Result<(), Error> {
    let result = ctx
        .data()
        .funboy
        .get_substitutes(
            &template,
            search_term.as_deref(),
            OrderBy::NameIgnoreCase(SortOrder::Ascending),
            Limit::Count(1000),
        )
        .await;
    match result {
        Ok(subs) => {
            if subs.len() == 0 {
                ctx.say_ephemeral(&format!("No substitutes found in `{}`", template))
                    .await?;
            }
            let list_style = if list_style.is_none() {
                ListStyle::Default
            } else {
                list_style.unwrap()
            };

            let subs: Vec<&str> = subs.iter().map(|sub| sub.name.as_str()).collect();

            match list_style {
                ListStyle::Default => {
                    if subs.len() > 0 {
                        ctx.say_list(
                            &subs,
                            true,
                            Some(Box::new(|items| format_as_item_seperated_list(items, ""))),
                        )
                        .await?;
                    }
                }
                ListStyle::Numeric => {
                    ctx.say_list(&subs, true, Some(Box::new(format_as_numeric_list)))
                        .await?;
                }
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    };
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn list_templates(
    ctx: Context<'_>,
    search_term: Option<String>,
    list_style: Option<ListStyle>,
) -> Result<(), Error> {
    let result = ctx
        .data()
        .funboy
        .get_templates(
            search_term.as_deref(),
            OrderBy::NameIgnoreCase(SortOrder::Ascending),
            Limit::Count(1000),
        )
        .await;
    match result {
        Ok(templates) => {
            if templates.len() == 0 {
                ctx.say_ephemeral(&format!("No templates found.")).await?;
            }

            let list_style = if list_style.is_none() {
                ListStyle::Default
            } else {
                list_style.unwrap()
            };

            let templates: Vec<&str> = templates
                .iter()
                .map(|template| template.name.as_str())
                .collect();

            match list_style {
                ListStyle::Default => {
                    ctx.say_list(
                        &templates,
                        true,
                        Some(Box::new(|templates| {
                            format_as_item_seperated_list(templates, "")
                        })),
                    )
                    .await?;
                }
                ListStyle::Numeric => {
                    ctx.say_list(&templates, true, Some(Box::new(format_as_numeric_list)))
                        .await?;
                }
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    };
    Ok(())
}
