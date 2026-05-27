use clap::{Parser, arg};
use funboy_core::{
    Request,
    commands::{
        AddArgs, CommandError, CommandResult, CopyArgs, DeleteArgs, FslArgs, GenerateArgs,
        ListArgs, OllamaArgs, RenameArgs, ReplaceArgs, parse_command_args,
    },
    database::Platform,
    permissions::{Permission, Role},
};
use serenity::all::{Attachment, Message, UserId};

use crate::{Data, DiscordUserId, interpreter::interpreter_from_serenity};

#[derive(Parser, Debug, Clone)]
pub enum DiscordCommand {
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
    Register,
    Cancel,
}

async fn get_input_from_attatchment(
    attachment: Option<&Attachment>,
) -> Result<String, CommandError> {
    let Some(attatchment) = attachment else {
        return Err(CommandError::ExecutionFailed(
            "Please attatch a file you want to generate".into(),
        ));
    };
    let Ok(bytes) = attatchment.download().await else {
        return Err(CommandError::ExecutionFailed(
            "Failed to download file".into(),
        ));
    };
    let Ok(input) = String::from_utf8(bytes) else {
        return Err(CommandError::ExecutionFailed(
            "Only text files are allowed (file must be valid UTF-8).".into(),
        ));
    };
    Ok(input)
}

pub async fn handle_request(
    request: Request,
    ctx: &serenity::prelude::Context,
    data: &Data,
    message: &Message,
) -> Result<CommandResult, CommandError> {
    let funboy = data.funboy.clone();
    let interpreter = interpreter_from_serenity(ctx, data, message).await;
    let user_id = DiscordUserId(message.author.id);

    match request {
        Request::GenerateFile => {
            let input = get_input_from_attatchment(message.attachments.get(0)).await?;
            let output = funboy.generate(input, &interpreter).await?;
            Ok(CommandResult::Text(output))
        }
        Request::UploadSub(template) => {
            let sub = get_input_from_attatchment(message.attachments.get(0)).await?;
            funboy
                .add_command(user_id, Platform::Discord, template, sub, true)
                .await
        }
        Request::DeleteTemplate(template) => {
            if message.content.to_lowercase() == "yes" {
                funboy
                    .delete_command(
                        user_id.clone(),
                        Platform::Matrix,
                        template,
                        String::new(),
                        false,
                        false,
                    )
                    .await
            } else {
                Ok(CommandResult::None)
            }
        }
    }
}

pub async fn handle_prefix_command(
    ctx: &serenity::prelude::Context,
    data: &Data,
    message: &Message,
) -> Result<CommandResult, CommandError> {
    let funboy = data.funboy.clone();
    let interpreter = interpreter_from_serenity(ctx, data, message).await;
    let user_id = DiscordUserId(message.author.id);
    let user_ctx = funboy.users.get_or_insert(user_id.clone()).await?;
    let user_permissions = funboy.users.get_permissions(user_id.clone()).await?;

    let input = message.content.trim_start_matches("!");
    let args = parse_command_args(input);

    match DiscordCommand::try_parse_from(args) {
        Ok(command) => match command {
            DiscordCommand::Generate { args } => {
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
                    funboy
                        .generate_command(
                            Platform::Matrix,
                            user_id,
                            &interpreter,
                            input,
                            false,
                            ollama,
                        )
                        .await
                }
            }
            DiscordCommand::Add { args } => {
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
            DiscordCommand::Delete { args } => {
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
                            Platform::Discord,
                            template,
                            substitutes,
                            single,
                            id,
                        )
                        .await
                }
            }
            DiscordCommand::List { args } => {
                let ListArgs {
                    template,
                    search_term,
                    list_style,
                } = args;
                funboy.list_command(template, search_term, list_style).await
            }
            DiscordCommand::Copy { args } => {
                let CopyArgs {
                    from_template,
                    to_template,
                } = args;
                funboy
                    .copy_command(user_id, from_template, to_template)
                    .await
            }
            DiscordCommand::Rename { args } => {
                let RenameArgs {
                    from_template,
                    to_template,
                } = args;
                funboy
                    .rename_command(user_id, from_template, to_template)
                    .await
            }
            DiscordCommand::Replace { args } => {
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
            DiscordCommand::Ollama { args } => {
                let OllamaArgs { action } = args;
                funboy
                    .ollama_command(user_id, Platform::Matrix, action)
                    .await
            }
            DiscordCommand::Grant { user, permissions } => {
                let receiver = DiscordUserId(find_user(&ctx, &message, &user).await?);

                funboy.grant_command(user_id, receiver, permissions).await
            }
            DiscordCommand::Revoke { user, permissions } => {
                let receiver = DiscordUserId(find_user(&ctx, &message, &user).await?);

                funboy.revoke_command(user_id, receiver, permissions).await
            }
            DiscordCommand::SetRole { user, role } => {
                let receiver = DiscordUserId(find_user(&ctx, &message, &user).await?);

                funboy.set_role(user_id, receiver, role).await
            }
            DiscordCommand::Cancel => funboy.cancel_command(user_id).await,
            DiscordCommand::Register => {
                // register already handled by poise
                Ok(CommandResult::None)
            }
            DiscordCommand::Fsl { args } => {
                let FslArgs { input } = args;
                funboy.fsl_command(user_id, input, &interpreter).await
            }
        },
        Err(e) => Err(CommandError::UnknownCommand(e.to_string())),
    }
}

async fn find_user(
    ctx: &serenity::prelude::Context,
    message: &Message,
    user: &str,
) -> Result<UserId, CommandError> {
    let Some(guild) = message.guild_id else {
        return Err(CommandError::ExecutionFailed(
            "must be called inside of guild".into(),
        ));
    };
    if let Some(user_id) = serenity::utils::parse_user_mention(user) {
        return Ok(user_id);
    }
    let receiver = guild.search_members(&ctx.http, &user, Some(1)).await;
    let receiver = match receiver {
        Ok(receiver) => match receiver.get(0) {
            Some(member) => Some(member.user.id),
            None => None,
        },
        Err(_) => None,
    };
    match receiver {
        Some(receiver) => Ok(receiver),
        None => Err(CommandError::ExecutionFailed(format!(
            "no user {} found in guild",
            user
        ))),
    }
}
