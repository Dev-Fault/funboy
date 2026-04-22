use std::sync::Arc;

use funboy_cli::{
    CliCommandResult, FunboyEnv, Id, Mode, enter_interactive_generation, enter_interpreter,
    get_funboy, interpret_bot_commands, interpreter::create_interpreter,
};
use funboy_core::{
    commands::CommandResult,
    database::Platform,
    ollama::{MAX_PREDICT, OllamaSettings},
};
use rustyline::DefaultEditor;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> rustyline::Result<()> {
    let env = FunboyEnv::new();
    let funboy = Arc::new(get_funboy::<Id>(&env, Platform::Cli).await);

    let users = funboy.users.clone();
    if let Err(e) = users.grant_all_permissions(Id(0)).await {
        eprintln!("{e}");
    }

    funboy.set_ollama_model(env.default_ollama_model).await;
    let rl = Arc::new(Mutex::new(DefaultEditor::new()?));
    let mut ollama_settings = OllamaSettings::default();
    ollama_settings.set_output_limit(MAX_PREDICT);

    loop {
        let mut rl_lock = rl.lock().await;
        let readline = rl_lock.readline(">> ");
        match readline {
            Ok(line) => {
                rl_lock.add_history_entry(&line)?;
                drop(rl_lock);
                match interpret_bot_commands(
                    Id(0),
                    &funboy,
                    create_interpreter(funboy.clone(), rl.clone()).await,
                    &line,
                )
                .await
                {
                    Ok(output) => match output {
                        CliCommandResult::CommandResult(CommandResult::Text(text)) => {
                            println!("{}", text)
                        }
                        CliCommandResult::Mode(context) => match context {
                            Mode::Generate => {
                                enter_interactive_generation(funboy.clone(), rl.clone()).await?;
                            }
                            Mode::FSL => {
                                enter_interpreter(funboy.clone(), rl.clone()).await?;
                            }
                        },
                        CliCommandResult::CommandResult(CommandResult::None) => {
                            continue;
                        }
                        CliCommandResult::Exit => {
                            break;
                        }
                    },
                    Err(e) => println!("{}", e.to_string()),
                }
            }
            Err(e) => {
                eprintln!("{}", e);
                break;
            }
        }
    }

    Ok(())
}
