use poise::CreateReply;
use serenity::all::{ComponentInteraction, CreateActionRow, CreateButton};
use uuid::Uuid;

use crate::{Context, Error};

pub const TRACK_BUTTON_ID: &str = "track";
pub const CANCEL_BUTTON_ID: &str = "cancel";
pub const CONFIRM_BUTTON_ID: &str = "confirm";

pub enum CustomComponent {
    TrackComponent,
    None,
}

impl CustomComponent {
    pub fn from(component_interaction: &ComponentInteraction) -> Self {
        if component_interaction
            .data
            .custom_id
            .starts_with(TRACK_BUTTON_ID)
        {
            return CustomComponent::TrackComponent;
        } else {
            return CustomComponent::None;
        }
    }
}

pub struct TrackComponent {
    interaction: ComponentInteraction,
    track_id: String,
    track_command: String,
}

impl TrackComponent {
    pub fn new(component_interaction: ComponentInteraction) -> Self {
        let track_id = component_interaction
            .data
            .custom_id
            .split_whitespace()
            .nth(1)
            .expect("Track button id should contain track id.")
            .to_string();

        let track_command = component_interaction
            .data
            .custom_id
            .split_whitespace()
            .nth(2)
            .expect("Track button id should contain track command.")
            .to_string();

        TrackComponent {
            interaction: component_interaction,
            track_id,
            track_command,
        }
    }

    pub fn get_interaction(&self) -> &ComponentInteraction {
        &self.interaction
    }

    pub fn get_track_id(&self) -> &str {
        self.track_id.as_str()
    }

    pub fn get_track_command(&self) -> &str {
        self.track_command.as_str()
    }
}

pub fn create_track_button(track_id: Uuid, command: &str) -> CreateButton {
    CreateButton::new(format!("{} {} {}", TRACK_BUTTON_ID, track_id, command))
        .label(command.replace("_", " "))
}

pub fn create_cancel_button() -> CreateButton {
    CreateButton::new(CANCEL_BUTTON_ID)
        .style(serenity::all::ButtonStyle::Danger)
        .label("Cancel")
}

pub fn create_confirm_button() -> CreateButton {
    CreateButton::new(CONFIRM_BUTTON_ID)
        .style(serenity::all::ButtonStyle::Success)
        .label("Confirm")
}

pub async fn create_confirmation_interaction<'a>(
    ctx: Context<'a>,
    interaction_msg: &str,
    timeout_secs: u64,
) -> Result<Option<ComponentInteraction>, Error> {
    let action_row =
        CreateActionRow::Buttons(vec![create_cancel_button(), create_confirm_button()]);

    let interaction = ctx
        .send(
            CreateReply::default()
                .content(interaction_msg)
                .ephemeral(true)
                .components(vec![action_row]),
        )
        .await?;

    Ok(interaction
        .message()
        .await?
        .await_component_interaction(ctx)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .await)
}
