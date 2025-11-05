use std::{collections::HashMap, sync::Arc};

use dotenvy::dotenv;
use funboy_core::{Funboy, template_database::TemplateDatabase};
use poise::serenity_prelude as serenity;
use sqlx::PgPool;

mod commands;
mod components;
mod io_format;

struct Data {
    funboy: Funboy,
} // User data, which is stored and accessible in all command invocations
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

impl Data {
    pub fn get_funboy(&self) -> &Funboy {
        &self.funboy
    }
}

#[poise::command(prefix_command)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    let intents = serenity::GatewayIntents::non_privileged();

    let db_url = std::env::var("DATABASE_URL").expect("missing DATABASE_URL");

    let pool = Arc::new(
        PgPool::connect(&db_url)
            .await
            .expect("failed to connect to database"),
    );

    TemplateDatabase::migrate(&pool)
        .await
        .expect("sqlx migration failed");

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                register(),
                commands::templates::generate(),
                commands::templates::rename_template(),
                commands::templates::add_subs(),
                commands::templates::copy_subs(),
                commands::templates::replace_sub(),
                commands::templates::delete_subs(),
                commands::templates::delete_template(),
                commands::templates::list_subs(),
                commands::templates::list_templates(),
            ],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                //poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    funboy: Funboy::new(TemplateDatabase::new(pool.clone())),
                })
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;
    client.unwrap().start().await.unwrap();
}
