use funboy_core::format::TruncateEllipsize;
use funboy_core::ollama::MAX_PREDICT;
use poise::CreateReply;

use crate::{
    Context, DiscordUserId, Error, context_extension::ContextExtension,
    interpreter::create_custom_interpreter,
};

const ERROR_OLLAMA_UNAVAILABLE: &str = "Error: Ollama service not available.";

/// Lists out all the available ollama models
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn list_ollama_models(ctx: Context<'_>) -> Result<(), Error> {
    let models = ctx.data().funboy.get_ollama_models().await;
    match models {
        Err(_) => {
            ctx.say_ephemeral(ERROR_OLLAMA_UNAVAILABLE).await?;
        }
        Ok(models) => {
            ctx.say_ephemeral(
                &models
                    .iter()
                    .fold("".to_string(), |names, model| names + &model + "\n"),
            )
            .await?;
        }
    }

    Ok(())
}

/// Lists out the current ollama settings
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn list_ollama_settings(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let user_ctx = ctx
        .data()
        .funboy
        .users
        .get_or_insert(DiscordUserId(user_id))
        .await;
    let settings = user_ctx.ollama_settings.lock().await;

    let current_model = ctx.data().funboy.get_ollama_model().await;

    ctx.say_ephemeral(&format!(
        "Current Model: {}\n{}",
        &current_model.unwrap_or("Unset".to_string()),
        &settings.to_string()
    ))
    .await?;

    Ok(())
}

/// Sets the current ollama model
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_model(ctx: Context<'_>, model: String) -> Result<(), Error> {
    let models = ctx.data().funboy.get_ollama_models().await;
    match models {
        Err(_) => {
            ctx.say_ephemeral(ERROR_OLLAMA_UNAVAILABLE).await?;
        }
        Ok(models) => {
            if models
                .iter()
                .map(|model| model)
                .any(|name| name.as_str() == model.as_str())
            {
                ctx.data()
                    .funboy
                    .set_ollama_model(Some(model.clone()))
                    .await;
                ctx.say_ephemeral(&format!("Set ollama model to: \"{}\"", model))
                    .await?;
            } else {
                ctx.say_ephemeral(&format!(
                    "Error: \"{}\" is not an avialable ollama model.",
                    model
                ))
                .await?;
            }
        }
    }
    Ok(())
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
    let user_id = ctx.author().id;
    let user_ctx = ctx
        .data()
        .funboy
        .users
        .get_or_insert(DiscordUserId(user_id))
        .await;
    let mut settings = user_ctx.ollama_settings.lock().await;

    if let Some(temperature) = temperature {
        settings.set_temperature(temperature);
    }
    if let Some(repeat_penalty) = repeat_penalty {
        settings.set_repeat_penalty(repeat_penalty);
    }
    if let Some(top_k) = top_k {
        settings.set_top_k(top_k);
    }
    if let Some(top_p) = top_p {
        settings.set_top_p(top_p);
    }
    ctx.say_ephemeral("Ollama parameters updated.").await?;
    Ok(())
}

/// Resets the ollama model parameters to their defaults
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn reset_ollama_parameters(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let user_ctx = ctx
        .data()
        .funboy
        .users
        .get_or_insert(DiscordUserId(user_id))
        .await;
    let mut settings = user_ctx.ollama_settings.lock().await;

    settings.reset_parameters();
    ctx.say_ephemeral("Ollama parameters reset.").await?;
    Ok(())
}

/// Sets the system prompt for ollama
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_system_prompt(
    ctx: Context<'_>,
    system_prompt: String,
) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let user_ctx = ctx
        .data()
        .funboy
        .users
        .get_or_insert(DiscordUserId(user_id))
        .await;
    let mut settings = user_ctx.ollama_settings.lock().await;

    settings.set_system_prompt(&system_prompt);
    ctx.say_ephemeral("Ollama system prompt updated.").await?;
    Ok(())
}

/// Resets the system prompt for ollama to it's default
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn reset_ollama_system_prompt(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let user_ctx = ctx
        .data()
        .funboy
        .users
        .get_or_insert(DiscordUserId(user_id))
        .await;
    let mut settings = user_ctx.ollama_settings.lock().await;

    settings.reset_system_prompt();
    ctx.say_ephemeral("Ollama system prompt reset.").await?;
    Ok(())
}

/// Sets the template for ollama
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_template(ctx: Context<'_>, template: String) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let user_ctx = ctx
        .data()
        .funboy
        .users
        .get_or_insert(DiscordUserId(user_id))
        .await;
    let mut settings = user_ctx.ollama_settings.lock().await;

    settings.set_template(&template);
    ctx.say_ephemeral("Ollama system prompt updated.").await?;
    Ok(())
}

/// Resets the template for ollama to it's default
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn reset_ollama_template(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let user_ctx = ctx
        .data()
        .funboy
        .users
        .get_or_insert(DiscordUserId(user_id))
        .await;
    let mut settings = user_ctx.ollama_settings.lock().await;

    settings.reset_template();
    ctx.say_ephemeral("Ollama template reset.").await?;
    Ok(())
}

/// Sets the maximum amount of words (tokens) ollama can generate per prompt
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_word_limit(ctx: Context<'_>, limit: u16) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let user_ctx = ctx
        .data()
        .funboy
        .users
        .get_or_insert(DiscordUserId(user_id))
        .await;
    let mut settings = user_ctx.ollama_settings.lock().await;

    if settings.set_output_limit(limit) {
        ctx.say_ephemeral("Ollama parameters updated.").await?;
    } else {
        ctx.say_ephemeral(&format!(
            "Error: Cannot exceed maximum output limit of {}.",
            MAX_PREDICT
        ))
        .await?;
    }
    Ok(())
}

/// Generates text like the generate command but sends the text as a prompt to ollama
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn generate_ollama(ctx: Context<'_>, prompt: String) -> Result<(), Error> {
    let original_message = ctx.say("Generating...").await?;

    let user_id = ctx.author().id;

    let interpreted_prompt = ctx
        .data()
        .funboy
        .user_generate(
            DiscordUserId(user_id),
            &prompt,
            create_custom_interpreter(&ctx),
        )
        .await;

    let result: Result<(), Error> = {
        match interpreted_prompt {
            Ok(prompt) => {
                original_message
                    .edit(
                        ctx,
                        CreateReply::default().content(&format!(
                            "Generating prompt: **\"{}\"**",
                            &prompt.truncate_with_ellipse(200)
                        )),
                    )
                    .await?;

                let response = ctx
                    .data()
                    .funboy
                    .user_generate_ollama(
                        DiscordUserId(user_id),
                        &prompt,
                        create_custom_interpreter(&ctx),
                    )
                    .await;
                match response {
                    Err(e) => {
                        ctx.say_ephemeral(&format!("Error: {}", e.to_string()))
                            .await?;
                    }
                    Ok(response) => {
                        ctx.say_long(
                            &format!("{}{}", response.prompt, response.generated_text),
                            false,
                        )
                        .await?;
                    }
                }
                Ok(())
            }
            Err(e) => {
                ctx.say_ephemeral(&e.to_string()).await?;
                Ok(())
            }
        }
    };

    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("{}", e);
            ctx.say_ephemeral("Error: Ollama generation failed.")
                .await?;
            Ok(())
        }
    }
}
