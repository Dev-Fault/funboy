use funboy_core::commands::{
    CommandResult, OllamaAction, OllamaEnableTool, OllamaListOption, OllamaResetOption,
    OllamaSetOption,
};
use funboy_core::database::Platform;

use crate::commands::templates::generate_message;
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

async fn reset_ollama(ctx: Context<'_>, option: OllamaResetOption) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let user_id = ctx.author().id;
    let result = funboy
        .ollama_command(
            DiscordUserId(user_id),
            Platform::Discord,
            OllamaAction::Reset { option },
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

async fn list_ollama(ctx: Context<'_>, option: OllamaListOption) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let user_id = ctx.author().id;
    let result = funboy
        .ollama_command(
            DiscordUserId(user_id),
            Platform::Discord,
            OllamaAction::List { option },
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

/// Lists out all the available ollama models
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn list_ollama_models(ctx: Context<'_>) -> Result<(), Error> {
    list_ollama(ctx, OllamaListOption::Models).await
}

/// Lists out the current ollama settings
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn list_ollama_settings(ctx: Context<'_>) -> Result<(), Error> {
    list_ollama(ctx, OllamaListOption::Settings).await
}

/// Sets the current ollama model
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_model(ctx: Context<'_>, model: String) -> Result<(), Error> {
    set_ollama(ctx, OllamaSetOption::Model { model: model }).await
}

/// Sets the ollama model parameters
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_parameters(
    ctx: Context<'_>,
    temperature: Option<f32>,
    repeat_penalty: Option<f32>,
    top_k: Option<u32>,
    top_p: Option<f32>,
) -> Result<(), Error> {
    set_ollama(
        ctx,
        OllamaSetOption::Parameters {
            temperature,
            repeat_penalty,
            top_k,
            top_p,
        },
    )
    .await
}

/// Resets the ollama model parameters to their defaults
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn reset_ollama_parameters(ctx: Context<'_>) -> Result<(), Error> {
    reset_ollama(ctx, OllamaResetOption::Parameters).await
}

/// Sets the system prompt for ollama
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_system_prompt(
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

/// Resets the system prompt for ollama to it's default
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn reset_ollama_system_prompt(ctx: Context<'_>) -> Result<(), Error> {
    reset_ollama(ctx, OllamaResetOption::SystemPrompt).await
}

/// Sets the template for ollama
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_template(ctx: Context<'_>, template: String) -> Result<(), Error> {
    set_ollama(
        ctx,
        OllamaSetOption::Template {
            template: vec![template],
        },
    )
    .await
}

/// Resets the template for ollama to it's default
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn reset_ollama_template(ctx: Context<'_>) -> Result<(), Error> {
    reset_ollama(ctx, OllamaResetOption::Template).await
}

/// Clears ollama history
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn clear_ollama_history(ctx: Context<'_>) -> Result<(), Error> {
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

/// Clears ollama history
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn toggle_ollama_web_search(ctx: Context<'_>) -> Result<(), Error> {
    let funboy = ctx.data().funboy.clone();
    let user_id = DiscordUserId(ctx.author().id);
    let result = funboy
        .ollama_command(
            user_id,
            Platform::Discord,
            OllamaAction::Toggle {
                tool: OllamaEnableTool::WebSearch,
            },
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

/// Sets the maximum amount of words (tokens) ollama can generate per prompt
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_output_limit(ctx: Context<'_>, limit: u16) -> Result<(), Error> {
    set_ollama(ctx, OllamaSetOption::OutputLimit { limit: limit }).await
}

/// Generates text like the generate command but sends the text as a prompt to ollama
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn generate_ollama(ctx: Context<'_>, prompt: String) -> Result<(), Error> {
    generate_message(ctx, prompt, true).await
}
