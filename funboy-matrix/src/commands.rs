use funboy_cli::{Command, CommandError, CommandResult, ImageAction};
use funboy_core::{Funboy, Permission, Request};
use matrix_sdk::{
    Room,
    attachment::AttachmentConfig,
    ruma::{OwnedUserId, events::room::message::RoomMessageEventContent},
};

use crate::MatrixUser;

pub async fn interpret_matrix_commands(
    funboy: &Funboy<MatrixUser>,
    matrix_user: MatrixUser,
    room: Room,
    command: Command,
) -> Result<CommandResult, CommandError> {
    let user_ctx = funboy.users.get_or_insert(matrix_user.clone()).await;
    let user_permissions = funboy.users.get_permissions(matrix_user).await;
    let room_id = room.room_id().to_owned();

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
            if user_permissions.can_use_files() && user_permissions.can_generate() {
                let mut pending_requests = user_ctx.pending_requests.lock().await;
                pending_requests.push(Request::GenerateFile);
                room.send(RoomMessageEventContent::text_plain(
                    "Attach the file you want to upload.",
                ))
                .await
                .unwrap();
                Ok(CommandResult::None)
            } else {
                Err(CommandError::LackingPermissions(
                    user_permissions.get_lacking(&[Permission::File, Permission::Generate]),
                ))
            }
        }
        Command::Add { file: false, .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Add {
            template,
            file: true,
            ..
        } => {
            if user_permissions.can_use_files() && user_permissions.can_update() {
                let mut pending_requests = user_ctx.pending_requests.lock().await;
                pending_requests.push(Request::UploadSub(template));
                room.send(RoomMessageEventContent::text_plain(
                    "Attach the file you want to add as a substitute.",
                ))
                .await
                .unwrap();
                Ok(CommandResult::None)
            } else {
                Err(CommandError::LackingPermissions(
                    user_permissions.get_lacking(&[Permission::File, Permission::Update]),
                ))
            }
        }
        Command::Delete { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::List { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Copy { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Rename { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Replace { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Ollama { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Context { .. } => Err(CommandError::UnhandledCommand(command)),
        Command::Exit => Err(CommandError::UnhandledCommand(command)),
        Command::Grant { user, permissions } => {
            if user_permissions.can_grant() {
                let user_id = match OwnedUserId::try_from(user.as_str()) {
                    Ok(user_id) => user_id,
                    Err(_) => {
                        room.send(RoomMessageEventContent::text_plain(format!(
                            "No user named {} in room",
                            user
                        )))
                        .await
                        .unwrap();
                        return Ok(CommandResult::None);
                    }
                };

                let matrix_user = MatrixUser::new(room_id, user_id);
                let mut users = funboy.users.clone();
                let result = users
                    .grant_permissions(matrix_user.clone(), &permissions)
                    .await;

                match result {
                    Ok(_) => {
                        room.send(RoomMessageEventContent::text_plain(&format!(
                            "Granted {} permissions to {}",
                            permissions
                                .iter()
                                .map(|p| p.to_string())
                                .collect::<Vec<String>>()
                                .join(", "),
                            matrix_user.user_id,
                        )))
                        .await
                        .unwrap();
                    }
                    Err(e) => {
                        room.send(RoomMessageEventContent::text_plain(e.to_string()))
                            .await
                            .unwrap();
                    }
                }

                println!("Granted {:?} permissions from user ", permissions);

                Ok(CommandResult::None)
            } else {
                Err(CommandError::LackingPermission(Permission::Grant))
            }
        }
        Command::Revoke { user, permissions } => {
            if user_permissions.can_revoke() {
                let user_id = match OwnedUserId::try_from(user.as_str()) {
                    Ok(user_id) => user_id,
                    Err(_) => {
                        room.send(RoomMessageEventContent::text_plain(format!(
                            "No user named {} in room",
                            user
                        )))
                        .await
                        .unwrap();
                        return Ok(CommandResult::None);
                    }
                };

                let matrix_user = MatrixUser::new(room_id, user_id);
                let mut users = funboy.users.clone();
                let result = users
                    .revoke_permissions(matrix_user.clone(), &permissions)
                    .await;

                match result {
                    Ok(_) => {
                        room.send(RoomMessageEventContent::text_plain(&format!(
                            "Revoked {} permissions from {}",
                            permissions
                                .iter()
                                .map(|p| p.to_string())
                                .collect::<Vec<String>>()
                                .join(", "),
                            matrix_user.user_id,
                        )))
                        .await
                        .unwrap();
                    }
                    Err(e) => {
                        room.send(RoomMessageEventContent::text_plain(e.to_string()))
                            .await
                            .unwrap();
                    }
                }
                println!("Revoked {:?} permissions from user ", permissions);

                Ok(CommandResult::None)
            } else {
                Err(CommandError::LackingPermission(Permission::Grant))
            }
        }
    }
}
