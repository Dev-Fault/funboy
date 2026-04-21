use std::sync::Arc;

use clap::Parser;
use fsl_interpreter::FslInterpreter;
use funboy_core::{
    Funboy, Permission, Request, Role,
    commands::{CommandError, CommandResult, OllamaAction, parse_command_args},
    database::Platform,
    format::{LIST_STYLE_NONE, ListStyle},
};
use matrix_sdk::{Room, attachment::AttachmentConfig, ruma::OwnedUserId};
use tokio::sync::Mutex;

use crate::MatrixUser;

#[derive(Parser, Debug, Clone)]
pub enum ImageAction {
    Embed { url: String },
}

#[derive(Parser, Debug, Clone)]
pub enum Command {
    Generate {
        #[arg(short, long)]
        file: bool,

        #[arg(short, long)]
        ollama: bool,

        #[arg(trailing_var_arg = true)]
        input: Vec<String>,
    },
    Add {
        template: String,

        #[arg(short, long)]
        single: bool,

        #[arg(short, long)]
        file: bool,

        #[arg(trailing_var_arg = true)]
        substitutes: Vec<String>,
    },
    Delete {
        template: String,

        #[arg(short, long)]
        single: bool,

        #[arg(short, long)]
        id: bool,

        #[arg(trailing_var_arg = true)]
        substitutes: Vec<String>,
    },
    List {
        template: Option<String>,

        #[arg(short, long, default_value = None)]
        search_term: Option<String>,

        #[arg(short, long, value_parser = clap::value_parser!(ListStyle), default_value = LIST_STYLE_NONE)]
        list_style: ListStyle,
    },
    Copy {
        from_template: String,
        to_template: String,
    },
    Rename {
        from_template: String,
        to_template: String,
    },
    Replace {
        substitute: String,
        with_substitute: String,

        #[arg(short, long)]
        template: Option<String>,

        #[arg(short, long)]
        id: bool,
    },
    Ollama {
        #[command(subcommand)]
        action: OllamaAction,
    },
    Image {
        #[command(subcommand)]
        action: ImageAction,
    },
    SetRole {
        user: String,
        role: Role,
    },
    Grant {
        user: String,

        #[arg(trailing_var_arg = true)]
        permissions: Vec<Permission>,
    },
    Revoke {
        user: String,

        #[arg(trailing_var_arg = true)]
        permissions: Vec<Permission>,
    },
}

pub async fn interpret_matrix_commands(
    funboy: &Funboy<MatrixUser>,
    interpreter: Arc<Mutex<FslInterpreter>>,
    matrix_user: MatrixUser,
    room: Room,
    input: &str,
) -> Result<CommandResult, CommandError> {
    let user_ctx = funboy.users.get_or_insert(matrix_user.clone()).await;
    let user_permissions = funboy.users.get_permissions(matrix_user.clone()).await;
    let room_id = room.room_id().to_owned();
    let user_id = matrix_user.clone();

    let args = parse_command_args(input);

    match Command::try_parse_from(args) {
        Ok(command) => match command {
            Command::Image { action } => match action {
                ImageAction::Embed { url } => {
                    if user_permissions.can_use_files() {
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
                    } else {
                        Err(CommandError::LackingPermission(Permission::File))
                    }
                }
            },
            Command::Generate {
                file: false,
                input,
                ollama,
            } => {
                funboy
                    .generate_command(Platform::Matrix, user_id, interpreter, input, false, ollama)
                    .await
            }
            Command::Generate { file: true, .. } => {
                if user_permissions.can_use_files() && user_permissions.can_generate() {
                    let mut pending_requests = user_ctx.pending_requests.lock().await;
                    pending_requests.push(Request::GenerateFile);
                    Ok(CommandResult::Text(format!(
                        "Attach the file you want to upload."
                    )))
                } else {
                    Err(CommandError::LackingPermissions(
                        user_permissions.get_lacking(&[Permission::File, Permission::Generate]),
                    ))
                }
            }
            Command::Add {
                file: false,
                template,
                single,
                substitutes,
            } => {
                let substitutes = substitutes.join(" ");
                funboy
                    .add_command(user_id, Platform::Matrix, template, substitutes, single)
                    .await
            }
            Command::Add {
                template,
                file: true,
                ..
            } => {
                if user_permissions.can_use_files() && user_permissions.can_update() {
                    let mut pending_requests = user_ctx.pending_requests.lock().await;
                    pending_requests.push(Request::UploadSub(template));
                    Ok(CommandResult::Text(format!(
                        "Attach the file you want to add as a substitute.",
                    )))
                } else {
                    Err(CommandError::LackingPermissions(
                        user_permissions.get_lacking(&[Permission::File, Permission::Update]),
                    ))
                }
            }
            Command::Delete {
                template,
                single,
                id,
                substitutes,
            } => {
                let substitutes = substitutes.join(" ");
                funboy
                    .delete_command(user_id, Platform::Matrix, template, substitutes, single, id)
                    .await
            }
            Command::List {
                template,
                search_term,
                list_style,
            } => funboy.list_command(template, search_term, list_style).await,
            Command::Copy {
                from_template,
                to_template,
            } => {
                funboy
                    .copy_command(user_id, from_template, to_template)
                    .await
            }
            Command::Rename {
                from_template,
                to_template,
            } => {
                funboy
                    .rename_command(user_id, from_template, to_template)
                    .await
            }
            Command::Replace {
                substitute,
                with_substitute,
                template,
                id,
            } => {
                funboy
                    .replace_command(user_id, template, substitute, with_substitute, id)
                    .await
            }
            Command::Ollama { action } => {
                funboy
                    .ollama_command(user_id, Platform::Matrix, action)
                    .await
            }
            Command::Grant { user, permissions } => {
                let receiver_id = find_user(&user, room).await?;
                let receiver = MatrixUser::new(room_id, receiver_id);

                funboy.grant_command(user_id, receiver, permissions).await
            }
            Command::Revoke { user, permissions } => {
                let receiver_id = find_user(&user, room).await?;
                let receiver = MatrixUser::new(room_id, receiver_id);

                funboy.revoke_command(user_id, receiver, permissions).await
            }
            Command::SetRole { user, role } => {
                let receiver_id = find_user(&user, room).await?;
                let receiver = MatrixUser::new(room_id, receiver_id);

                funboy.set_role(user_id, receiver, role).await
            }
        },
        Err(e) => Err(CommandError::UnknownCommand(e.to_string())),
    }
}

pub async fn find_user(user: &str, room: Room) -> Result<OwnedUserId, String> {
    match OwnedUserId::try_from(user) {
        Ok(receiver_id) => {
            if room.get_member(&receiver_id).await.ok().is_some() {
                Ok(receiver_id)
            } else {
                return Err(format!("No user named {} in room", user));
            }
        }
        Err(_) => {
            return Err(format!("No user named {} in room", user));
        }
    }
}
