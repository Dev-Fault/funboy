use std::str::FromStr;

use clap::Parser;
use fsl_core::FslInterpreter;
use funboy_core::{
    Funboy, Request,
    commands::{
        AddArgs, CommandError, CommandResult, CopyArgs, DeleteArgs, FslArgs, GenerateArgs,
        ListArgs, OllamaArgs, RenameArgs, ReplaceArgs, parse_command_args,
    },
    database::Platform,
    permissions::{Permission, Permissions, Role},
};
use matrix_sdk::{
    Room, attachment::AttachmentConfig, reqwest::header::CONTENT_TYPE, ruma::OwnedUserId,
};
use mime::Mime;

use crate::{MatrixUser, send_msg_with_mixed_content};

#[derive(Parser, Debug, Clone)]
pub enum MatrixCommand {
    Generate {
        #[command(flatten)]
        args: GenerateArgs,
    },
    Fsl {
        #[command(flatten)]
        args: FslArgs,
    },
    Add {
        #[command(flatten)]
        args: AddArgs,
    },
    Delete {
        #[command(flatten)]
        args: DeleteArgs,
    },
    List {
        #[command(flatten)]
        args: ListArgs,
    },
    Copy {
        #[command(flatten)]
        args: CopyArgs,
    },
    Rename {
        #[command(flatten)]
        args: RenameArgs,
    },
    Replace {
        #[command(flatten)]
        args: ReplaceArgs,
    },
    Ollama {
        #[command(flatten)]
        args: OllamaArgs,
    },
    Attach {
        url: String,
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
    Cancel,
}

pub async fn attach_url(
    url: &str,
    user_permissions: &Permissions,
    room: Room,
) -> Result<CommandResult, CommandError> {
    if user_permissions.can_use_files() {
        let Ok(response) = matrix_sdk::reqwest::get(url).await else {
            return Err(CommandError::ExecutionFailed("invalid url".to_string()));
        };
        let headers = response.headers();

        match headers.get(CONTENT_TYPE) {
            Some(content_type) => {
                let Ok(content_type) = content_type.to_str() else {
                    return Err(CommandError::ExecutionFailed(format!(
                        "couldn't get content type of url"
                    )));
                };
                let Ok(content_type) = Mime::from_str(content_type) else {
                    return Err(CommandError::ExecutionFailed(format!(
                        "couldn't get content type of url"
                    )));
                };
                match content_type.type_() {
                    mime::AUDIO | mime::VIDEO | mime::IMAGE | mime::TEXT => {
                        let bytes = response.bytes().await.unwrap();

                        match room
                            .send_attachment(
                                &format!("{}", url),
                                &content_type,
                                bytes.to_vec(),
                                AttachmentConfig::new(),
                            )
                            .await
                        {
                            Ok(_) => Ok(CommandResult::None),
                            Err(_) => Err(CommandError::ExecutionFailed(
                                "failed to send attachment".to_string(),
                            )),
                        }
                    }
                    _ => Err(CommandError::ExecutionFailed(format!(
                        "invalid attachment type"
                    ))),
                }
            }
            None => Err(CommandError::ExecutionFailed(format!(
                "couldn't get content type of url"
            ))),
        }
    } else {
        Err(CommandError::LackingPermission(Permission::File))
    }
}

pub async fn interpret_matrix_commands(
    funboy: &Funboy<MatrixUser>,
    interpreter: &FslInterpreter,
    matrix_user: MatrixUser,
    room: Room,
    input: &str,
) -> Result<CommandResult, CommandError> {
    let user_ctx = funboy.users.get_or_insert(matrix_user.clone()).await?;
    let user_permissions = funboy.users.get_permissions(matrix_user.clone()).await?;
    let room_id = room.room_id().to_owned();
    let user_id = matrix_user.clone();

    let args = parse_command_args(input);

    match MatrixCommand::try_parse_from(args) {
        Ok(command) => match command {
            MatrixCommand::Attach { url } => attach_url(&url, &user_permissions, room).await,
            MatrixCommand::Generate { args } => {
                let GenerateArgs {
                    file,
                    ollama,
                    input,
                } = args;

                if file {
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
                } else {
                    let result = funboy
                        .generate_command(
                            Platform::Matrix,
                            user_id,
                            &interpreter,
                            input,
                            false,
                            ollama,
                        )
                        .await;

                    match result {
                        Ok(output) => match output {
                            CommandResult::Text(output) => {
                                send_msg_with_mixed_content(&output, &user_permissions, room).await;
                                Ok(CommandResult::None)
                            }
                            _ => Ok(CommandResult::None),
                        },
                        Err(e) => Err(e),
                    }
                }
            }
            MatrixCommand::Add { args } => {
                let AddArgs {
                    template,
                    single,
                    file,
                    substitutes,
                } = args;
                if file {
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
                } else {
                    let substitutes = substitutes.join(" ");
                    funboy
                        .add_command(user_id, Platform::Matrix, template, substitutes, single)
                        .await
                }
            }
            MatrixCommand::Delete { args } => {
                let DeleteArgs {
                    template,
                    single,
                    id,
                    substitutes,
                } = args;
                if substitutes.len() == 0 {
                    let mut pending_requests = user_ctx.pending_requests.lock().await;
                    pending_requests.push(Request::DeleteTemplate(template.clone()));
                    return Ok(CommandResult::Text(format!(
                        "Are you sure you want to delete {} (yes/no)? All of it's substitutes will also be deleted.",
                        template
                    )));
                } else {
                    let substitutes = substitutes.join(" ");
                    funboy
                        .delete_command(
                            user_id,
                            Platform::Matrix,
                            template,
                            substitutes,
                            single,
                            id,
                        )
                        .await
                }
            }
            MatrixCommand::List { args } => {
                let ListArgs {
                    template,
                    search_term,
                    list_style,
                } = args;
                funboy.list_command(template, search_term, list_style).await
            }
            MatrixCommand::Copy { args } => {
                let CopyArgs {
                    from_template,
                    to_template,
                } = args;
                funboy
                    .copy_command(user_id, from_template, to_template)
                    .await
            }
            MatrixCommand::Rename { args } => {
                let RenameArgs {
                    from_template,
                    to_template,
                } = args;
                funboy
                    .rename_command(user_id, from_template, to_template)
                    .await
            }
            MatrixCommand::Replace { args } => {
                let ReplaceArgs {
                    substitute,
                    with_substitute,
                    template,
                    id,
                } = args;
                funboy
                    .replace_command(user_id, template, substitute, with_substitute, id)
                    .await
            }
            MatrixCommand::Ollama { args } => {
                let OllamaArgs { action } = args;
                funboy
                    .ollama_command(user_id, Platform::Matrix, action)
                    .await
            }
            MatrixCommand::Grant { user, permissions } => {
                let receiver_id = find_user(&user, room).await?;
                let receiver = MatrixUser::new(room_id, receiver_id);

                funboy.grant_command(user_id, receiver, permissions).await
            }
            MatrixCommand::Revoke { user, permissions } => {
                let receiver_id = find_user(&user, room).await?;
                let receiver = MatrixUser::new(room_id, receiver_id);

                funboy.revoke_command(user_id, receiver, permissions).await
            }
            MatrixCommand::SetRole { user, role } => {
                let receiver_id = find_user(&user, room).await?;
                let receiver = MatrixUser::new(room_id, receiver_id);

                funboy.set_role(user_id, receiver, role).await
            }
            MatrixCommand::Cancel => funboy.cancel_command(user_id).await,
            MatrixCommand::Fsl { args } => {
                let FslArgs { input } = args;
                funboy.fsl_command(user_id, input, interpreter).await
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
