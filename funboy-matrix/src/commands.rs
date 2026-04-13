use clap::Parser;
use funboy_cli::{CommandError, CommandResult};
use funboy_core::{Funboy, RequestCode};
use matrix_sdk::{
    Room, attachment::AttachmentConfig, ruma::events::room::message::RoomMessageEventContent,
};

use crate::MatrixUserId;

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
    Generate {
        #[arg(short, long)]
        file: bool,
    },
}

pub enum Request {
    GenerateFile,
}

impl Into<RequestCode> for Request {
    fn into(self) -> RequestCode {
        match self {
            Request::GenerateFile => 0,
        }
    }
}

impl From<RequestCode> for Request {
    fn from(value: RequestCode) -> Self {
        match value {
            0 => Request::GenerateFile,
            _ => panic!("invalid request code"),
        }
    }
}

pub async fn interpret_matrix_commands(
    funboy: &Funboy<MatrixUserId>,
    user_id: MatrixUserId,
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
            MatrixCommand::Generate { file } => {
                if file {
                    let user_ctx = funboy.get_user_ctx(user_id).await;
                    let mut pending_requests = user_ctx.pending_requests.lock().await;
                    pending_requests.push(Request::GenerateFile.into());
                    room.send(RoomMessageEventContent::text_plain(
                        "Attach the file you want to upload.",
                    ))
                    .await
                    .unwrap();
                    Ok(CommandResult::None)
                } else {
                    Err(CommandError::UnknownCommand("".to_string()))
                }
            }
        },
        Err(e) => Err(CommandError::UnknownCommand(e.to_string())),
    }
}
