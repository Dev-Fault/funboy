use clap::ValueEnum;
use funboy_core::commands::{
    CommandResult, OllamaAction, OllamaListOption, OllamaResetOption, OllamaSetOption,
    OllamaToggleTool,
};
use funboy_core::database::Platform;
use poise::ChoiceParameter;

use crate::impl_choice;
use crate::{Context, DiscordUserId, Error, context_extension::ContextExtension};

async fn set_ollama(ctx: Context<'_>, option: OllamaSetOption) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let user_id = ctx.author().id;
    let result = funboy
        .ollama_command(
            DiscordUserId(user_id),
            Platform::Discord,
            OllamaAction::Set { option },
        )
        .await;

    match result {
        Ok(CommandResult::Text(result)) => {
            ctx.say_ephemeral(&result.to_string()).await?;
        }
        Ok(CommandResult::None) => {}
        Err(e) => {
            eprintln!("{}", e.to_string());
        }
    }
    Ok(())
}

impl_choice!(OllamaListOption, OllamaListChoice);
pub struct OllamaListChoice(OllamaListOption);
impl_choice!(OllamaToggleTool, OllamaToggleChoice);
pub struct OllamaToggleChoice(OllamaToggleTool);
impl_choice!(OllamaResetOption, OllamaResetChoice);
pub struct OllamaResetChoice(OllamaResetOption);

#[poise::command(
    slash_command,
    category = "Ollama",
    subcommands(
        "ollama_clear",
        "ollama_list",
        "ollama_toggle",
        "ollama_reset",
        "ollama_set"
    )
)]
pub async fn ollama(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(
    slash_command,
    prefix_command,
    category = "Ollama",
    subcommands(
        "ollama_set_model",
        "ollama_set_template",
        "ollama_set_output_limit",
        "ollama_set_system_prompt",
        "ollama_set_temperature",
        "ollama_set_repeat_penalty",
        "ollama_set_top_k",
        "ollama_set_top_p",
    ),
    rename = "set"
)]
pub async fn ollama_set(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Sets the current ollama model
#[poise::command(slash_command, prefix_command, category = "Ollama", rename = "model")]
pub async fn ollama_set_model(ctx: Context<'_>, model: String) -> Result<(), Error> {
    set_ollama(ctx, OllamaSetOption::Model { model: model }).await
}

/// Sets the system prompt for ollama
#[poise::command(
    slash_command,
    prefix_command,
    category = "Ollama",
    rename = "system-prompt"
)]
pub async fn ollama_set_system_prompt(
    ctx: Context<'_>,
    system_prompt: String,
) -> Result<(), Error> {
    set_ollama(
        ctx,
        OllamaSetOption::SystemPrompt {
            system_prompt: vec![system_prompt],
        },
    )
    .await
}

/// Sets the template for ollama
#[poise::command(
    slash_command,
    prefix_command,
    category = "Ollama",
    rename = "template"
)]
pub async fn ollama_set_template(ctx: Context<'_>, template: String) -> Result<(), Error> {
    set_ollama(
        ctx,
        OllamaSetOption::Template {
            template: vec![template],
        },
    )
    .await
}

/// Sets the maximum amount of tokens ollama can generate per prompt
#[poise::command(
    slash_command,
    prefix_command,
    category = "Ollama",
    rename = "output-limit"
)]
pub async fn ollama_set_output_limit(ctx: Context<'_>, limit: u16) -> Result<(), Error> {
    set_ollama(ctx, OllamaSetOption::OutputLimit { limit: limit }).await
}

/// Resets an ollama setting to it's default value
#[poise::command(slash_command, prefix_command, category = "Ollama", rename = "reset")]
async fn ollama_reset(ctx: Context<'_>, option: OllamaResetChoice) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let user_id = ctx.author().id;
    let result = funboy
        .ollama_command(
            DiscordUserId(user_id),
            Platform::Discord,
            OllamaAction::Reset { option: option.0 },
        )
        .await;

    match result {
        Ok(CommandResult::Text(result)) => {
            ctx.say_ephemeral(&result.to_string()).await?;
        }
        Ok(CommandResult::None) => {}
        Err(e) => {
            eprintln!("{}", e.to_string());
        }
    }
    Ok(())
}

/// Toggles ollama tool on or off
#[poise::command(slash_command, prefix_command, category = "Ollama", rename = "toggle")]
pub async fn ollama_toggle(ctx: Context<'_>, tool: OllamaToggleChoice) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let user_id = DiscordUserId(ctx.author().id);
    let result = funboy
        .ollama_command(
            user_id,
            Platform::Discord,
            OllamaAction::Toggle { tool: tool.0 },
        )
        .await;

    match result {
        Ok(result) => {
            if let CommandResult::Text(message) = result {
                ctx.say_ephemeral(&message).await?;
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }

    Ok(())
}

/// Lists ollama settings
#[poise::command(slash_command, prefix_command, category = "Ollama", rename = "list")]
pub async fn ollama_list(ctx: Context<'_>, choice: OllamaListChoice) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let user_id = ctx.author().id;
    let result = funboy
        .ollama_command(
            DiscordUserId(user_id),
            Platform::Discord,
            OllamaAction::List { option: choice.0 },
        )
        .await;

    match result {
        Ok(CommandResult::Text(result)) => {
            ctx.say_ephemeral(&result.to_string()).await?;
        }
        Ok(CommandResult::None) => {}
        Err(e) => {
            eprintln!("{}", e.to_string());
        }
    }
    Ok(())
}

/// Clears ollama chat history
#[poise::command(slash_command, prefix_command, category = "Ollama", rename = "clear")]
pub async fn ollama_clear(ctx: Context<'_>) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let user_id = DiscordUserId(ctx.author().id);
    let result = funboy
        .ollama_command(user_id, Platform::Discord, OllamaAction::Clear)
        .await;

    match result {
        Ok(result) => {
            if let CommandResult::Text(message) = result {
                ctx.say_ephemeral(&message).await?;
            }
        }
        Err(e) => {
            ctx.say_ephemeral(&e.to_string()).await?;
        }
    }

    Ok(())
}

/// Sets ollama temperature
#[poise::command(
    slash_command,
    prefix_command,
    category = "Ollama",
    rename = "temperature"
)]
pub async fn ollama_set_temperature(ctx: Context<'_>, temperature: f32) -> Result<(), Error> {
    set_ollama(ctx, OllamaSetOption::Temperature { temperature }).await
}

/// Sets ollama repeat penalty
#[poise::command(
    slash_command,
    prefix_command,
    category = "Ollama",
    rename = "repeat-penalty"
)]
pub async fn ollama_set_repeat_penalty(ctx: Context<'_>, repeat_penalty: f32) -> Result<(), Error> {
    set_ollama(
        ctx,
        OllamaSetOption::RepeatPenalty {
            repeat_penalty: repeat_penalty,
        },
    )
    .await
}

/// Sets ollama top k
#[poise::command(slash_command, prefix_command, category = "Ollama", rename = "top-k")]
pub async fn ollama_set_top_k(ctx: Context<'_>, top_k: u32) -> Result<(), Error> {
    set_ollama(ctx, OllamaSetOption::TopK { top_k }).await
}

/// Sets ollama top p
#[poise::command(slash_command, prefix_command, category = "Ollama", rename = "top-p")]
pub async fn ollama_set_top_p(ctx: Context<'_>, top_p: f32) -> Result<(), Error> {
    set_ollama(ctx, OllamaSetOption::TopP { top_p }).await
}
