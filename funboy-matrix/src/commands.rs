use funboy_cli::{Command, CommandError, CommandResult, ImageAction};
use funboy_core::{Funboy, Request};
use matrix_sdk::{
    Room, attachment::AttachmentConfig, ruma::events::room::message::RoomMessageEventContent,
};

use crate::MatrixUser;

pub async fn interpret_matrix_commands(
    funboy: &Funboy<MatrixUser>,
    matrix_user: MatrixUser,
    room: Room,
    command: Command,
) -> Result<CommandResult, CommandError> {
    match command {
        Command::Image { action } => match action {
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
        Command::Generate { file: false, .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Generate { file: true, .. } => {
            let user_ctx = funboy.get_user_ctx(matrix_user).await;
            let mut pending_requests = user_ctx.pending_requests.lock().await;
            pending_requests.push(Request::GenerateFile);
            room.send(RoomMessageEventContent::text_plain(
                "Attach the file you want to upload.",
            ))
            .await
            .unwrap();
            Ok(CommandResult::None)
        }
        Command::Add { file: false, .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Add {
            template,
            file: true,
            ..
        } => {
            let user_ctx = funboy.get_user_ctx(matrix_user).await;
            let mut pending_requests = user_ctx.pending_requests.lock().await;
            pending_requests.push(Request::UploadSub(template));
            room.send(RoomMessageEventContent::text_plain(
                "Attach the file you want to add as a substitute.",
            ))
            .await
            .unwrap();
            Ok(CommandResult::None)
        }
        Command::Delete { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::List { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Copy { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Rename { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Replace { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Ollama { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Context { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Exit => Err(CommandError::UnhandledCommand(command)),
    }
}
