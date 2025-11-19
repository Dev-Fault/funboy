use std::{sync::Arc, time::Duration};

use crate::{
    Data, HttpClient,
    components::{TrackComponent, create_track_button},
    io_format::context_extension::ContextExtension,
};
use poise::{CreateReply, serenity_prelude::async_trait};
use serenity::all::{
    CacheHttp, CreateActionRow, CreateInteractionResponse, CreateInteractionResponseMessage,
    EditInteractionResponse,
};
use songbird::{
    CoreEvent, Songbird,
    events::{Event, EventContext, EventHandler as VoiceEventHandler, TrackEvent},
    input::{Compose, YoutubeDl},
    tracks::{LoopState, PlayMode, TrackHandle},
};
use std::collections::HashMap;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{Context, Error, HttpKey};

const TRACK_LIMIT: usize = 10;

const NOT_INITIALIZED: &str = "Songbird Voice client placed in at initialisation.";
const NOT_IN_VOICE_CHANNEL: &str = "Not in a voice channel.";
const JOINED_CHANNEL_NOTIF: &str = "Joined voice channel.";
const LEFT_CHANNEL_NOTIF: &str = "Left voice channel.";
const STOP_AUDIO_NOTIF: &str = "Stopping all audio.";
const NON_EXISTANT_TRACK_ERROR: &str = "Error: Track no longer exists.";
const NO_TRACKS_PLAYING_NOTIF: &str = "No tracks are currently playing.";

pub const PLAY_PAUSE: &str = "Play/Pause";
pub const STOP: &str = "Stop";
pub const VOLUME_UP: &str = "Volume_Up";
pub const VOLUME_DOWN: &str = "Volume_Down";
pub const LOOP: &str = "Loop";
pub const TRACK_COMMANDS: [&str; 5] = [PLAY_PAUSE, STOP, VOLUME_UP, VOLUME_DOWN, LOOP];

#[allow(dead_code)]
#[derive(Debug)]
pub struct Track {
    name: String,
    handle: TrackHandle,
    duration: Duration,
}

impl Track {
    pub async fn get_description(&self) -> String {
        let track_status;
        if let Ok(status) = self.handle.get_info().await {
            track_status = format!(
                "Playing: **{}** Volume: **{} / 1** Looping: **{}**",
                status.playing == PlayMode::Play,
                status.volume,
                status.loops == LoopState::Infinite
            );
        } else {
            track_status = "This track has been stopped and can no longer be played.".to_string();
        }
        format!("Track: **{}** \n{}", &self.name, track_status)
    }
}

type TrackMap = HashMap<Uuid, Track>;

#[derive(Debug)]
pub struct TrackList {
    track_map: TrackMap,
}

impl TrackList {
    pub fn new() -> Self {
        TrackList {
            track_map: TrackMap::new(),
        }
    }

    pub fn add_track(&mut self, track: Track) {
        self.track_map.insert(track.handle.uuid(), track);
    }

    pub fn remove_track(&mut self, track_id: &Uuid) {
        let track = self.track_map.remove(&track_id);
        if let Some(track) = track {
            let _ = track.handle.stop();
        }
    }

    pub fn get_tracks(&mut self) -> Arc<[&mut Track]> {
        self.track_map.values_mut().collect()
    }

    pub fn get_track_count(&self) -> usize {
        self.track_map.values().len()
    }

    pub fn get_track(&mut self, id: &str) -> Option<&mut Track> {
        for (uuid, track) in &mut self.track_map {
            if uuid.to_string().eq(id) {
                return Some(track);
            }
        }
        None
    }

    pub fn clear(&mut self) {
        for track in self.track_map.values() {
            let _ = track.handle.stop();
        }
        self.track_map.clear();
    }
}

#[derive(Debug)]
pub struct TrackEndHandler {
    track_list: Arc<Mutex<TrackList>>,
}

#[async_trait]
impl songbird::EventHandler for TrackEndHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(track_list) = ctx {
            for (_, handle) in *track_list {
                self.track_list.lock().await.remove_track(&handle.uuid());
            }
        } else if let EventContext::DriverDisconnect(_) = ctx {
            self.track_list.lock().await.clear();
        }

        None
    }
}

struct TrackErrorNotifier;

#[async_trait]
impl VoiceEventHandler for TrackErrorNotifier {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(track_list) = ctx {
            for (state, handle) in *track_list {
                println!(
                    "Track {:?} encountered an error: {:?}.",
                    handle.uuid(),
                    state.playing
                );
            }
        }

        None
    }
}

/// Join bot to current voice channel
#[poise::command(slash_command, prefix_command, category = "Sound")]
pub async fn join_voice(ctx: Context<'_>) -> Result<(), Error> {
    let (guild_id, channel_id) = {
        let guild = ctx.guild().unwrap();
        let channel_id = guild
            .voice_states
            .get(&ctx.author().id)
            .and_then(|voice_state| voice_state.channel_id);

        (guild.id, channel_id)
    };

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            ctx.say(NOT_IN_VOICE_CHANNEL).await?;

            return Ok(());
        }
    };

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect(NOT_INITIALIZED)
        .clone();

    if let Ok(handler_lock) = manager.join(guild_id, connect_to).await {
        let mut handler = handler_lock.lock().await;

        handler.add_global_event(TrackEvent::Error.into(), TrackErrorNotifier);
        handler.add_global_event(
            CoreEvent::DriverDisconnect.into(),
            TrackEndHandler {
                track_list: ctx.data().track_list.clone(),
            },
        );

        ctx.say(JOINED_CHANNEL_NOTIF).await?;
    }

    Ok(())
}

/// Disconnect bot from voice channel
#[poise::command(slash_command, prefix_command, category = "Sound")]
pub async fn leave_voice(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();

    let manager = get_songbird_manager(ctx).await;

    let has_handler = manager.get(guild_id).is_some();

    if has_handler {
        if let Err(e) = manager.remove(guild_id).await {
            ctx.say(format!("Failed: {:?}", e)).await?;
        }

        ctx.say(LEFT_CHANNEL_NOTIF).await?;
    } else {
        ctx.say(NOT_IN_VOICE_CHANNEL).await?;
    }

    Ok(())
}

/// Play audio track from url or search query
///
/// Example usage: **/play_track** url_or_query: **https://www.youtube.com/watch?v=a3mxLL7nX1E**
/// Example usage: **/play_track** url_or_query: **Back In Black**
#[poise::command(slash_command, prefix_command, category = "Sound")]
pub async fn play_track(ctx: Context<'_>, url_or_query: String) -> Result<(), Error> {
    let lock = match ctx.data().track_player_lock.try_lock() {
        Ok(gaurd) => gaurd,
        Err(_) => {
            ctx.say_ephemeral("Busy queuing track. Please wait.")
                .await?;
            return Ok(());
        }
    };

    let track_count = ctx.data().track_list.lock().await.get_track_count();
    if track_count >= TRACK_LIMIT {
        ctx.say_ephemeral(&format!("Max track limit of {} reached.", TRACK_LIMIT))
            .await?;
        return Ok(());
    }

    let yt_dlp_cookies_path = &ctx.data().yt_dlp_cookies_path;
    let mut ytdl_args = vec!["-f".to_string(), "bestaudio/best".to_string()];

    if let Some(path) = yt_dlp_cookies_path {
        ytdl_args.push("--cookies".to_string());
        ytdl_args.push(path.clone());
    }

    let is_url = !url_or_query.starts_with("http");

    let http_client = get_http_client(ctx).await;

    let manager = get_songbird_manager(ctx).await;

    if let Some(handler_lock) = manager.get(ctx.guild_id().unwrap()) {
        ctx.say_ephemeral("Queuing track...").await?;
        ctx.defer().await?;
        let mut handler = handler_lock.lock().await;

        let mut src = if is_url {
            YoutubeDl::new_search(http_client, url_or_query.clone()).user_args(ytdl_args)
        } else {
            YoutubeDl::new(http_client, url_or_query.clone()).user_args(ytdl_args)
        };

        let metadata = src.aux_metadata().await?;

        let track_duration = metadata.duration.unwrap_or(Duration::from_secs(0));
        let track_name = metadata.title.unwrap_or(url_or_query.clone());

        let track_handle = handler.play_input(src.clone().into());

        let _ = track_handle.add_event(
            TrackEvent::End.into(),
            TrackEndHandler {
                track_list: ctx.data().track_list.clone(),
            },
        );

        let _ = track_handle.add_event(
            TrackEvent::Error.into(),
            TrackEndHandler {
                track_list: ctx.data().track_list.clone(),
            },
        );

        ctx.data().track_list.lock().await.add_track(Track {
            name: track_name,
            handle: track_handle,
            duration: track_duration,
        });

        ctx.send(CreateReply::default().content(format!("Playing track **{}**", &url_or_query)))
            .await?;
    } else {
        ctx.say(NOT_IN_VOICE_CHANNEL).await?;
    }

    drop(lock);

    display_track_controls(ctx).await?;

    Ok(())
}

/// Stop all currently playing audio tracks
#[poise::command(slash_command, prefix_command, category = "Sound")]
pub async fn stop_tracks(ctx: Context<'_>) -> Result<(), Error> {
    ctx.data().track_list.lock().await.clear();

    let manager = get_songbird_manager(ctx).await;

    if let Some(handler_lock) = manager.get(ctx.guild_id().unwrap()) {
        let mut handler = handler_lock.lock().await;
        handler.stop();

        ctx.say(STOP_AUDIO_NOTIF).await?;
    } else {
        ctx.say(NOT_IN_VOICE_CHANNEL).await?;
    }

    Ok(())
}

pub async fn display_track_controls(ctx: Context<'_>) -> Result<(), Error> {
    let manager = get_songbird_manager(ctx).await;

    if let Some(handler_lock) = manager.get(ctx.guild_id().unwrap()) {
        let mut _handler = handler_lock.lock().await;

        let mut track_list = ctx.data().track_list.lock().await;
        let tracks = track_list.get_tracks();

        if tracks.len() == 0 {
            ctx.say_ephemeral(NO_TRACKS_PLAYING_NOTIF).await?;
            return Ok(());
        }

        for track in tracks.iter() {
            let mut buttons = Vec::new();
            for command in TRACK_COMMANDS {
                buttons.push(create_track_button(track.handle.uuid(), command));
            }

            ctx.send(
                CreateReply::default()
                    .content(track.get_description().await)
                    .ephemeral(true)
                    .components(vec![CreateActionRow::Buttons(buttons)]),
            )
            .await?;
        }
    } else {
        ctx.say(NOT_IN_VOICE_CHANNEL).await?;
    }

    Ok(())
}

/// Show currently playing audio tracks
#[poise::command(slash_command, prefix_command, category = "Sound")]
pub async fn list_tracks(ctx: Context<'_>) -> Result<(), Error> {
    display_track_controls(ctx).await?;
    Ok(())
}

pub async fn on_track_button_click(
    ctx: &poise::serenity_prelude::Context,
    track_component: TrackComponent,
    data: &Data,
) -> Result<(), Error> {
    if let Some(track) = data
        .track_list
        .lock()
        .await
        .get_track(track_component.get_track_id())
    {
        if let Ok(track_state) = track.handle.get_info().await {
            match track_component.get_track_command() {
                PLAY_PAUSE => match track_state.playing {
                    songbird::tracks::PlayMode::Play => {
                        let _ = track.handle.pause();
                    }
                    songbird::tracks::PlayMode::Pause => {
                        let _ = track.handle.play();
                    }
                    _ => {}
                },
                STOP => {
                    let _ = track.handle.stop();
                }
                VOLUME_UP => {
                    let new_volume = track_state.volume + 0.25;
                    if new_volume <= 1.0 {
                        let _ = track.handle.set_volume(new_volume);
                    }
                }
                VOLUME_DOWN => {
                    let new_volume = track_state.volume - 0.25;
                    if new_volume >= 0.0 {
                        let _ = track.handle.set_volume(new_volume);
                    }
                }
                LOOP => match track_state.loops {
                    songbird::tracks::LoopState::Infinite => {
                        let _ = track.handle.disable_loop();
                    }
                    songbird::tracks::LoopState::Finite(_) => {
                        let _ = track.handle.enable_loop();
                    }
                },
                _ => {}
            }
        }

        track_component
            .get_interaction()
            .create_response(ctx.http(), CreateInteractionResponse::Acknowledge)
            .await?;

        if track_component.get_track_command() == STOP {
            track_component
                .get_interaction()
                .edit_response(
                    ctx.http(),
                    EditInteractionResponse::default()
                        .content(track.get_description().await)
                        .components(vec![]),
                )
                .await?;
        } else {
            track_component
                .get_interaction()
                .edit_response(
                    ctx.http(),
                    EditInteractionResponse::default().content(track.get_description().await),
                )
                .await?;
        }
    } else {
        track_component
            .get_interaction()
            .create_response(
                ctx.http(),
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(NON_EXISTANT_TRACK_ERROR)
                        .components(vec![])
                        .ephemeral(true),
                ),
            )
            .await?;
    }

    Ok(())
}

async fn get_songbird_manager(ctx: Context<'_>) -> Arc<Songbird> {
    songbird::get(ctx.serenity_context())
        .await
        .expect(NOT_INITIALIZED)
        .clone()
}

async fn get_http_client(ctx: Context<'_>) -> HttpClient {
    let client: HttpClient = {
        let data = ctx.serenity_context().data.read().await;
        data.get::<HttpKey>()
            .cloned()
            .expect("Guaranteed to exist in the typemap.")
    };
    client
}
