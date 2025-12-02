use funboy_core::ollama::{MAX_PREDICT, OllamaSettings};
use poise::CreateReply;
use serenity::all::UserId;

use crate::{
    Context, Error, OllamaUserSettingsMap,
    interpreter::create_custom_interpreter,
    io_format::{context_extension::ContextExtension, discord_message_format::ellipsize_if_long},
};

const ERROR_OLLAMA_UNAVAILABLE: &str = "Error: Ollama service not available.";

/// Lists out all the available ollama models
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn list_ollama_models(ctx: Context<'_>) -> Result<(), Error> {
    let ollama_generator = ctx.data().ollama_data.generator.lock().await;
    let models = ollama_generator.get_models().await;
    match models {
        Err(_) => {
            ctx.say_ephemeral(ERROR_OLLAMA_UNAVAILABLE).await?;
        }
        Ok(models) => {
            ctx.say_ephemeral(
                &models
                    .iter()
                    .fold("".to_string(), |names, model| names + &model.name + "\n"),
            )
            .await?;
        }
    }

    Ok(())
}

fn get_ollama_user_settings<'a>(
    ollama_settings_map: &'a mut OllamaUserSettingsMap,
    user_id: &UserId,
) -> &'a OllamaSettings {
    ollama_settings_map.entry(*user_id).or_default();
    ollama_settings_map.get(user_id).unwrap()
}

fn get_ollama_user_settings_mut<'a>(
    ollama_settings_map: &'a mut OllamaUserSettingsMap,
    user_id: &UserId,
) -> &'a mut OllamaSettings {
    ollama_settings_map.entry(*user_id).or_default();
    ollama_settings_map.get_mut(user_id).unwrap()
}

/// Lists out the current ollama settings
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn list_ollama_settings(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let mut ollama_settings_map = ctx.data().ollama_data.user_settings.lock().await;
    let settings = get_ollama_user_settings(&mut ollama_settings_map, &user_id);

    let current_model = ctx.data().funboy.get_ollama_model().await;

    ctx.say_ephemeral(&format!(
        "Current Model: {}\n{}",
        &current_model.unwrap_or("Default".to_string()),
        &settings.to_string()
    ))
    .await?;

    Ok(())
}

/// Sets the current ollama model
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_model(ctx: Context<'_>, model: String) -> Result<(), Error> {
    let ollama_generator = ctx.data().ollama_data.generator.lock().await;
    let models = ollama_generator.get_models().await;
    drop(ollama_generator);
    match models {
        Err(_) => {
            ctx.say_ephemeral(ERROR_OLLAMA_UNAVAILABLE).await?;
        }
        Ok(models) => {
            if models
                .iter()
                .map(|model| &model.name)
                .any(|name| *name == model)
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
    let mut ollama_settings_map = ctx.data().ollama_data.user_settings.lock().await;
    let settings = get_ollama_user_settings_mut(&mut ollama_settings_map, &user_id);

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
    let mut ollama_settings_map = ctx.data().ollama_data.user_settings.lock().await;
    let settings = get_ollama_user_settings_mut(&mut ollama_settings_map, &user_id);

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
    let mut ollama_settings_map = ctx.data().ollama_data.user_settings.lock().await;
    let settings = get_ollama_user_settings_mut(&mut ollama_settings_map, &user_id);

    settings.set_system_prompt(&system_prompt);
    ctx.say_ephemeral("Ollama system prompt updated.").await?;
    Ok(())
}

/// Resets the system prompt for ollama to it's default
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn reset_ollama_system_prompt(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let mut ollama_settings_map = ctx.data().ollama_data.user_settings.lock().await;
    let settings = get_ollama_user_settings_mut(&mut ollama_settings_map, &user_id);

    settings.reset_system_prompt();
    ctx.say_ephemeral("Ollama system prompt reset.").await?;
    Ok(())
}

/// Sets the template for ollama
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_template(ctx: Context<'_>, template: String) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let mut ollama_settings_map = ctx.data().ollama_data.user_settings.lock().await;
    let settings = get_ollama_user_settings_mut(&mut ollama_settings_map, &user_id);

    settings.set_template(&template);
    ctx.say_ephemeral("Ollama system prompt updated.").await?;
    Ok(())
}

/// Resets the template for ollama to it's default
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn reset_ollama_template(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let mut ollama_settings_map = ctx.data().ollama_data.user_settings.lock().await;
    let settings = get_ollama_user_settings_mut(&mut ollama_settings_map, &user_id);

    settings.reset_template();
    ctx.say_ephemeral("Ollama template reset.").await?;
    Ok(())
}

/// Sets the maximum amount of words (tokens) ollama can generate per prompt
#[poise::command(slash_command, prefix_command, category = "Ollama")]
pub async fn set_ollama_word_limit(ctx: Context<'_>, limit: u16) -> Result<(), Error> {
    let user_id = ctx.author().id;
    let mut ollama_settings_map = ctx.data().ollama_data.user_settings.lock().await;
    let settings = get_ollama_user_settings_mut(&mut ollama_settings_map, &user_id);

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
    let mut users_lock = ctx.data().ollama_data.users.lock().await;

    if users_lock.contains(&user_id) {
        ctx.say_ephemeral("You are already generating a prompt. Please wait until it is finished.")
            .await?;
        return Ok(());
    } else {
        users_lock.insert(user_id);
    }
    drop(users_lock);

    let interpreted_prompt = ctx
        .data()
        .funboy
        .generate(&prompt, create_custom_interpreter(&ctx))
        .await;

    let result: Result<(), Error> = {
        match interpreted_prompt {
            Ok(prompt) => {
                original_message
                    .edit(
                        ctx,
                        CreateReply::default().content(&format!(
                            "Generating prompt: **\"{}\"**",
                            ellipsize_if_long(&prompt, 200)
                        )),
                    )
                    .await?;

                let user_id = ctx.author().id;
                let mut ollama_settings_map = ctx.data().ollama_data.user_settings.lock().await;
                let settings =
                    get_ollama_user_settings_mut(&mut ollama_settings_map, &user_id).clone();
                drop(ollama_settings_map);
                let ollama_generator = ctx.data().ollama_data.generator.lock().await;
                let model = ctx.data().funboy.get_ollama_model().await;
                let response = ollama_generator.generate(&prompt, &settings, model).await;
                match response {
                    Err(e) => {
                        ctx.say_ephemeral(&format!("Error: {}", e)).await?;
                    }
                    Ok(gen_res) => {
                        ctx.say_long(&format!("{}{}", &prompt, gen_res.response), false)
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

    let mut users = ctx.data().ollama_data.users.lock().await;
    users.remove(&user_id);

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
