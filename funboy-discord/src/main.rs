use dotenvy::dotenv;
use funboy_core::{Funboy, template_database::TemplateDatabase};
use poise::serenity_prelude as serenity;
use sqlx::PgPool;

struct Data {
    pub funboy: Funboy,
} // User data, which is stored and accessible in all command invocations
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

#[poise::command(slash_command, prefix_command)]
async fn generate(ctx: Context<'_>, input: String) -> Result<(), Error> {
    let output = ctx.data().funboy.generate(&input).await;

    match output {
        Ok(output) => ctx.say(output).await?,
        Err(e) => ctx.say(e).await?,
    };
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
async fn add_sub(ctx: Context<'_>, template: String, sub: String) -> Result<(), Error> {
    let result = ctx.data().funboy.add_substitutes(&template, &[&sub]).await;

    match result {
        Ok(subs) => ctx.say(format!("Added sub {}", subs[0].name)).await?,
        Err(e) => ctx.say(e).await?,
    };
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
async fn delete_sub(ctx: Context<'_>, template: String, sub: String) -> Result<(), Error> {
    let result = ctx
        .data()
        .funboy
        .delete_substitutes(&template, &[&sub])
        .await;

    match result {
        Ok(_) => ctx.say(format!("Deleted sub {}", sub)).await?,
        Err(e) => ctx.say(e).await?,
    };
    Ok(())
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    let intents = serenity::GatewayIntents::non_privileged();

    let db_url = std::env::var("DATABASE_URL").expect("missing DATABASE_URL");

    let pool = PgPool::connect(&db_url)
        .await
        .expect("failed to connect to database");

    let template_db = TemplateDatabase::new(pool)
        .await
        .expect("failed to create a connection to the template database");

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![generate(), add_sub(), delete_sub()],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    funboy: Funboy::new(template_db),
                })
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;
    client.unwrap().start().await.unwrap();
}
