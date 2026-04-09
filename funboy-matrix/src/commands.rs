use clap::Parser;
use funboy_cli::{BotData, CommandError, CommandResult, Permissions};
use matrix_sdk::{
    Room,
    attachment::AttachmentConfig,
    ruma::{
        api::client,
        events::{message::OriginalSyncMessageEvent, room::message::TextMessageEventContent},
    },
};

#[derive(Parser, Debug)]
enum ImageAction {
    Embed { url: String },
}

#[derive(Parser, Debug)]
enum MatrixCommand {
    Image {
        #[command(subcommand)]
        action: ImageAction,
    },
}

pub async fn interpret_matrix_commands(
    bot_data: &BotData,
    room: Room,
    input: &str,
) -> Result<CommandResult, CommandError> {
    let input = input.trim();

    if input.is_empty() {
        return Ok(CommandResult::None);
    }

    let args: Vec<&str> = input.split_whitespace().collect();

    let mut full_args = vec!["funboy"];
    full_args.extend(&args);

    let funboy = &bot_data.funboy;
    let ollama_settings = &bot_data.ollama_settings;

    dbg!(input);
    match MatrixCommand::try_parse_from(full_args) {
        Ok(command) => match command {
            MatrixCommand::Image { action } => match action {
                ImageAction::Embed { url } => {
                    let Ok(bytes) = reqwest::get(&url).await else {
                        return Err(CommandError::ExecutionFailed(
                            "invalid image url".to_string(),
                        ));
                    };
                    let Ok(bytes) = bytes.bytes().await else {
                        return Err(CommandError::ExecutionFailed(
                            "invalid image url".to_string(),
                        ));
                    };
                    let (mime, extension) = if url.contains("png") {
                        ("image/png", "png")
                    } else if url.contains("gif") {
                        ("image/gif", "gif")
                    } else if url.contains("webp") {
                        ("image/webp", "webp")
                    } else {
                        ("image/jpeg", "jpeg")
                    };
                    let mime = mime.parse::<mime::Mime>().unwrap();
                    match room
                        .send_attachment(
                            &format!("image.{}", extension),
                            &mime,
                            bytes.to_vec(),
                            AttachmentConfig::new(),
                        )
                        .await
                    {
                        Ok(_) => Ok(CommandResult::None),
                        Err(_) => Err(CommandError::ExecutionFailed(
                            "failed to upload image".to_string(),
                        )),
                    }
                }
            },
        },
        Err(e) => Err(CommandError::UnknownCommand(e.to_string())),
    }
}
