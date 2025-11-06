use funboy_core::{
    FunboyError,
    template_database::{KeySize, Limit, OrderBy, SortOrder},
};
use poise::ChoiceParameter;
use serenity::all::{ComponentInteraction, EditInteractionResponse};

use crate::{
    Context, Error,
    components::{
        CANCEL_BUTTON_ID, CONFIRM_BUTTON_ID, create_confirmation_interaction, edit_interaction,
    },
    io_format::{
        context_extension::ContextExtension,
        discord_message_format::{
            SeperatedListOptions, StringVecToRef, ellipsize_if_long, format_as_item_seperated_list,
            format_as_numeric_list, split_by_whitespace_unless_quoted,
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
                        ellipsize_if_long(&result.updated_to_string(), 1000)
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
                        ellipsize_if_long(&result.ignored_to_string(), 1000)
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
                    &format!("Deleted template `{}`", ellipsize_if_long(template, 1000)),
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
                        ellipsize_if_long(template, 1000)
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

#[poise::command(slash_command, prefix_command)]
pub async fn delete_templates(ctx: Context<'_>, names: String) -> Result<(), Error> {
    let templates = split_by_whitespace_unless_quoted(&names);

    let interaction_text = format!(
        "Are you sure you want to delete `{}`? All of {} substitutes will be deleted as well.",
        ellipsize_if_long(&names, 1000),
        if templates.len() > 1 { "their" } else { "it's" }
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
                        format_as_item_seperated_list(
                            subs,
                            &appended_text,
                            SeperatedListOptions::default(),
                        )
                    })),
                )
                .await?;
            }

            if sub_record.ignored.len() > 0 {
                let appended_text = format!("\nalready in `{}`", template);

                ctx.say_list(
                    &sub_record.ignored.to_ref(),
                    true,
                    Some(Box::new(move |items| {
                        format_as_item_seperated_list(
                            items,
                            &appended_text,
                            SeperatedListOptions::default(),
                        )
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
    delete_by_id: Option<bool>,
) -> Result<(), Error> {
    let delete_by_id = match delete_by_id {
        Some(delete_by_id) => delete_by_id,
        None => false,
    };

    let result = if delete_as_single_sub.is_some_and(|is_true| is_true) {
        if delete_by_id {
            match subs.parse::<KeySize>() {
                Ok(id) => ctx.data().funboy.delete_substitutes_by_id(&[id]).await,
                Err(_) => Err(FunboyError::UserInput(
                    "ID must be a valid number.".to_string(),
                )),
            }
        } else {
            ctx.data()
                .get_funboy()
                .delete_substitutes(&template, &[&subs])
                .await
        }
    } else {
        let subs: Vec<&str> = split_by_whitespace_unless_quoted(&subs);

        if delete_by_id {
            let ids: Result<Vec<KeySize>, _> = subs.iter().map(|s| s.parse::<KeySize>()).collect();
            match ids {
                Ok(ids) => ctx.data().funboy.delete_substitutes_by_id(&ids).await,
                Err(_) => Err(FunboyError::UserInput(
                    "Id must be a valid number.".to_string(),
                )),
            }
        } else {
            ctx.data()
                .get_funboy()
                .delete_substitutes(&template, &subs)
                .await
        }
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
                        format_as_item_seperated_list(
                            subs,
                            &appended_text,
                            SeperatedListOptions::default(),
                        )
                    })),
                )
                .await?;
            }

            if sub_record.ignored.len() > 0 {
                let appended_text = format!("\nnot present in `{}`", template);

                ctx.say_list(
                    &sub_record.ignored.to_ref(),
                    true,
                    Some(Box::new(move |items| {
                        format_as_item_seperated_list(
                            items,
                            &appended_text,
                            SeperatedListOptions::default(),
                        )
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
    ID,
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
                return Ok(());
            }

            let subs: Vec<String> = if matches!(list_style, Some(ListStyle::ID)) {
                subs.iter()
                    .map(|sub| {
                        format!(
                            "\nID: {}\nValue: {}{}\n",
                            sub.id,
                            if sub.name.len() > 100 { "\n" } else { "" },
                            sub.name,
                        )
                    })
                    .collect()
            } else {
                subs.iter().map(|sub| sub.name.clone()).collect()
            };

            let subs = subs.to_ref();

            let list_style = if list_style.is_none() {
                ListStyle::Default
            } else {
                list_style.unwrap()
            };

            match list_style {
                ListStyle::Default => {
                    ctx.say_list(
                        &subs,
                        true,
                        Some(Box::new(|items| {
                            format_as_item_seperated_list(
                                items,
                                "",
                                SeperatedListOptions::default(),
                            )
                        })),
                    )
                    .await?;
                }
                ListStyle::Numeric => {
                    ctx.say_list(&subs, true, Some(Box::new(format_as_numeric_list)))
                        .await?;
                }
                ListStyle::ID => {
                    ctx.say_list(
                        &subs,
                        true,
                        Some(Box::new(|items| {
                            format_as_item_seperated_list(
                                items,
                                "",
                                SeperatedListOptions::as_id_list(),
                            )
                        })),
                    )
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
                return Ok(());
            }

            let templates: Vec<String> = if matches!(list_style, Some(ListStyle::ID)) {
                templates
                    .iter()
                    .map(|template| format!("\nID: {}\nValue: {}\n", template.id, template.name,))
                    .collect()
            } else {
                templates
                    .iter()
                    .map(|template| template.name.clone())
                    .collect()
            };

            let templates = templates.to_ref();

            let list_style = if list_style.is_none() {
                ListStyle::Default
            } else {
                list_style.unwrap()
            };

            match list_style {
                ListStyle::Default => {
                    ctx.say_list(
                        &templates,
                        true,
                        Some(Box::new(|templates| {
                            format_as_item_seperated_list(
                                templates,
                                "",
                                SeperatedListOptions::default(),
                            )
                        })),
                    )
                    .await?;
                }
                ListStyle::Numeric => {
                    ctx.say_list(&templates, true, Some(Box::new(format_as_numeric_list)))
                        .await?;
                }
                ListStyle::ID => {
                    ctx.say_list(
                        &templates,
                        true,
                        Some(Box::new(|items| {
                            format_as_item_seperated_list(
                                items,
                                "",
                                SeperatedListOptions::as_id_list(),
                            )
                        })),
                    )
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
