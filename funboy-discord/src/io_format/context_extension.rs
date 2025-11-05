use std::time::Duration;

use crate::{Context, Error};

use poise::{CreateReply, ReplyHandle};
use tokio::time::sleep;

use super::discord_message_format::{DISCORD_CHARACTER_LIMIT, split_long_string, split_message};

pub const MAX_MESSAGE_CHAIN_SIZE: usize = DISCORD_CHARACTER_LIMIT * 4;
pub const WARN_MESSAGE_SIZE_EXCEEDED: &str = "Message was too large to send.";
pub const WARN_EMPTY_MESSAGE: &str = "Message was empty.";

pub type ListFormatter = Box<dyn Fn(&[&str]) -> Vec<String> + Send + Sync>;

pub trait ContextExtension {
    async fn say_list(
        &self,
        message: &[&str],
        ephemeral: bool,
        formatter: Option<ListFormatter>,
    ) -> Result<(), Error>;

    async fn say_ephemeral(&self, message: &str) -> Result<ReplyHandle<'_>, Error>;

    async fn say_long(&self, message: &str, ephemeral: bool) -> Result<(), Error>;
}

const MESSAGE_DELAY_MS: u64 = 500;
impl<'a> ContextExtension for Context<'a> {
    async fn say_list(
        &self,
        message: &[&str],
        ephemeral: bool,
        formatter: Option<ListFormatter>,
    ) -> Result<(), Error> {
        let mut size: usize = 0;

        for string in message {
            size = size.saturating_add(string.len());
        }

        if !ephemeral && size > MAX_MESSAGE_CHAIN_SIZE {
            self.say_ephemeral(WARN_MESSAGE_SIZE_EXCEEDED).await?;
            return Ok(());
        } else if size == 0 {
            self.say_ephemeral(WARN_EMPTY_MESSAGE).await?;
            return Ok(());
        }

        let formatted_message: Vec<String>;
        let message = match formatter {
            Some(formatter) => {
                formatted_message = formatter(message);
                &formatted_message
                    .iter()
                    .map(|msg| msg.as_str())
                    .collect::<Vec<&str>>()[..]
            }
            None => message,
        };

        for (i, split_message) in split_message(message).iter().enumerate() {
            self.defer_ephemeral().await?;
            self.send(
                CreateReply::default()
                    .content(split_message)
                    .ephemeral(ephemeral),
            )
            .await?;
            if i != 0 {
                sleep(Duration::from_millis(MESSAGE_DELAY_MS)).await;
            }
        }

        Ok(())
    }

    async fn say_ephemeral(&self, message: &str) -> Result<ReplyHandle<'_>, Error> {
        let reply_handle = if message.is_empty() {
            self.send(
                CreateReply::default()
                    .content(WARN_EMPTY_MESSAGE)
                    .ephemeral(true),
            )
            .await?
        } else {
            self.send(CreateReply::default().content(message).ephemeral(true))
                .await?
        };

        Ok(reply_handle)
    }

    async fn say_long(&self, message: &str, ephemeral: bool) -> Result<(), Error> {
        if !ephemeral && message.len() > MAX_MESSAGE_CHAIN_SIZE {
            self.say_ephemeral(WARN_MESSAGE_SIZE_EXCEEDED).await?;
            return Ok(());
        } else if message.is_empty() {
            self.say_ephemeral(WARN_EMPTY_MESSAGE).await?;
            return Ok(());
        }

        for m in split_long_string(message) {
            self.send(CreateReply::default().content(m).ephemeral(ephemeral))
                .await?;
        }
        Ok(())
    }
}
