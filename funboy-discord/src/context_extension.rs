use std::time::Duration;

use crate::{Context, Error};

use funboy_core::format::{TWO_THOUSAND, split_message, split_messages};
use poise::{CreateReply, ReplyHandle};
use tokio::time::sleep;

pub const BOT_MAX_MESSAGE_SIZE: usize = TWO_THOUSAND * 4;
pub const WARN_MESSAGE_SIZE_EXCEEDED: &str = "Message was too large to send.";
pub const WARN_EMPTY_MESSAGE: &str = "Message was empty.";

#[allow(dead_code)]
pub trait ContextExtension {
    async fn say_list(&self, message: &[&str], ephemeral: bool) -> Result<(), Error>;

    async fn say_ephemeral(&self, message: &str) -> Result<ReplyHandle<'_>, Error>;

    async fn say_long(&self, message: &str, ephemeral: bool) -> Result<(), Error>;

    async fn edit_long<'b>(
        &self,
        original_message: ReplyHandle<'b>,
        message: &str,
        ephemeral: bool,
    ) -> Result<(), Error>;
}

impl<'a> ContextExtension for Context<'a> {
    async fn say_list(&self, list: &[&str], ephemeral: bool) -> Result<(), Error> {
        let mut size: usize = 0;

        for string in list {
            size = size.saturating_add(string.len());
        }

        if !ephemeral && size > BOT_MAX_MESSAGE_SIZE {
            self.say_ephemeral(WARN_MESSAGE_SIZE_EXCEEDED).await?;
            return Ok(());
        } else if size == 0 {
            self.say_ephemeral(WARN_EMPTY_MESSAGE).await?;
            return Ok(());
        }

        for (i, split_message) in split_messages(list, TWO_THOUSAND).iter().enumerate() {
            if ephemeral {
                self.defer_ephemeral().await?;
            } else {
                self.defer().await?;
            }
            self.send(
                CreateReply::default()
                    .content(split_message)
                    .ephemeral(ephemeral),
            )
            .await?;
            if i != 0 {
                sleep(Duration::from_millis(200)).await;
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
        if !ephemeral && message.len() > BOT_MAX_MESSAGE_SIZE {
            self.say_ephemeral(WARN_MESSAGE_SIZE_EXCEEDED).await?;
            return Ok(());
        } else if message.is_empty() {
            self.say_ephemeral(WARN_EMPTY_MESSAGE).await?;
            return Ok(());
        }

        for m in split_message(message, TWO_THOUSAND) {
            self.send(CreateReply::default().content(m).ephemeral(ephemeral))
                .await?;
        }
        Ok(())
    }

    async fn edit_long<'b>(
        &self,
        original_message: ReplyHandle<'b>,
        message: &str,
        ephemeral: bool,
    ) -> Result<(), Error> {
        if !ephemeral && message.len() > BOT_MAX_MESSAGE_SIZE {
            self.say_ephemeral(WARN_MESSAGE_SIZE_EXCEEDED).await?;
            return Ok(());
        } else if message.is_empty() {
            self.say_ephemeral(WARN_EMPTY_MESSAGE).await?;
            return Ok(());
        }

        for (i, m) in split_message(message, TWO_THOUSAND).iter().enumerate() {
            if i == 0 {
                original_message
                    .edit(
                        *self,
                        CreateReply::default().content(*m).ephemeral(ephemeral),
                    )
                    .await?;
            } else {
                self.send(CreateReply::default().content(*m).ephemeral(ephemeral))
                    .await?;
            }
        }
        Ok(())
    }
}
