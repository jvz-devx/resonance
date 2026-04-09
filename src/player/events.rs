use std::sync::Arc;

use serenity::all::Http;
use serenity::model::id::GuildId;
use songbird::events::{Event, EventContext, EventHandler as SongbirdEventHandler, TrackEvent};
use songbird::input::{Input, YoutubeDl};
use songbird::Songbird;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::queue::track::TrackMetadata;
use crate::state::{GuildState, LoopMode};
use crate::utils::embeds;

/// Shared context for playing tracks, avoiding long parameter lists
pub struct PlayContext {
    pub manager: Arc<Songbird>,
    pub guild_id: GuildId,
    pub guild_state: Arc<Mutex<GuildState>>,
    pub http_client: reqwest::Client,
    pub discord_http: Arc<Http>,
    pub redis_pool: Option<deadpool_redis::Pool>,
}

impl PlayContext {
    fn clone_ctx(&self) -> PlayContext {
        PlayContext {
            manager: self.manager.clone(),
            guild_id: self.guild_id,
            guild_state: self.guild_state.clone(),
            http_client: self.http_client.clone(),
            discord_http: self.discord_http.clone(),
            redis_pool: self.redis_pool.clone(),
        }
    }
}

/// Event handler that fires when a track ends — plays the next track from queue
pub struct TrackEndHandler {
    pub ctx: PlayContext,
}

/// Event handler that fires when a track encounters an error
pub struct TrackErrorHandler {
    pub ctx: PlayContext,
}

#[async_trait::async_trait]
impl SongbirdEventHandler for TrackEndHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        info!("Track ended for guild {}", self.ctx.guild_id);

        let mut state = self.ctx.guild_state.lock().await;

        // Handle loop modes
        if let Some(ref current) = state.now_playing.clone() {
            match state.loop_mode {
                LoopMode::Track => {
                    debug!("Loop track: replaying current track");
                    if let Err(e) = play_track(&self.ctx, current, &mut state).await {
                        error!("Failed to replay track: {e}");
                    }
                    return None;
                }
                LoopMode::Queue => {
                    debug!("Loop queue: pushing current to back");
                    state.queue.enqueue(current.clone());
                }
                LoopMode::Off => {}
            }
        }

        // Try to play the next track
        if let Some(next_track) = state.queue.dequeue() {
            info!("Playing next track: {}", next_track.title);

            // Save finished track to history
            if let Some(ref pool) = self.ctx.redis_pool {
                if let Some(ref finished) = state.now_playing {
                    if let Err(e) = crate::state::redis::add_to_history(pool, self.ctx.guild_id.get(), finished).await {
                        warn!("Failed to save track to history: {e}");
                    }
                }
            }

            if let Err(e) = play_track(&self.ctx, &next_track, &mut state).await {
                error!("Failed to play next track: {e}");
                state.now_playing = None;
                state.current_track_handle = None;

                // Notify text channel about the failure
                if let Some(channel_id) = state.text_channel_id {
                    let embed = embeds::error_embed(&format!("Failed to play **{}**: {e}", next_track.title));
                    let builder = serenity::builder::CreateMessage::new().embed(embed);
                    if let Err(e) = channel_id.send_message(&self.ctx.discord_http, builder).await {
                        warn!("Failed to send error message: {e}");
                    }
                }
            }

            // Persist queue changes
            if let Some(ref pool) = self.ctx.redis_pool {
                let tracks: Vec<_> = state.queue.tracks.iter().cloned().collect();
                if let Err(e) = crate::state::redis::save_queue(pool, self.ctx.guild_id.get(), &tracks).await {
                    warn!("Failed to persist queue to Redis: {e}");
                }
                if let Err(e) = crate::state::redis::save_now_playing(pool, self.ctx.guild_id.get(), state.now_playing.as_ref()).await {
                    warn!("Failed to persist now_playing to Redis: {e}");
                }
            }

            // Send now-playing embed to text channel
            if let Some(channel_id) = state.text_channel_id {
                if let Some(ref np) = state.now_playing {
                    let embed = embeds::now_playing_embed(np);
                    let builder = serenity::builder::CreateMessage::new().embed(embed);
                    if let Err(e) = channel_id.send_message(&self.ctx.discord_http, builder).await {
                        warn!("Failed to send now-playing message: {e}");
                    }
                }
            }
        } else {
            info!("Queue empty for guild {}", self.ctx.guild_id);

            // Save finished track to history
            if let Some(ref pool) = self.ctx.redis_pool {
                if let Some(ref finished) = state.now_playing {
                    if let Err(e) = crate::state::redis::add_to_history(pool, self.ctx.guild_id.get(), finished).await {
                        warn!("Failed to save track to history: {e}");
                    }
                }
            }

            state.now_playing = None;
            state.current_track_handle = None;
            state.touch();

            // Persist
            if let Some(ref pool) = self.ctx.redis_pool {
                if let Err(e) = crate::state::redis::save_now_playing(pool, self.ctx.guild_id.get(), None).await {
                    warn!("Failed to clear now_playing in Redis: {e}");
                }
            }
        }

        None
    }
}

#[async_trait::async_trait]
impl SongbirdEventHandler for TrackErrorHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let error_msg = if let EventContext::Track(track_ctx) = ctx {
            format!("{:?}", track_ctx)
        } else {
            "Unknown error".to_string()
        };
        error!("Track error in guild {}: {error_msg}", self.ctx.guild_id);

        let mut state = self.ctx.guild_state.lock().await;
        let failed_title = state.now_playing.as_ref().map(|t| t.title.clone()).unwrap_or_default();
        state.now_playing = None;
        state.current_track_handle = None;

        // Notify text channel
        if let Some(channel_id) = state.text_channel_id {
            let embed = embeds::error_embed(&format!("An error occurred while playing **{failed_title}**. Skipping to next track."));
            let builder = serenity::builder::CreateMessage::new().embed(embed);
            if let Err(e) = channel_id.send_message(&self.ctx.discord_http, builder).await {
                warn!("Failed to send track error message: {e}");
            }
        }

        // Try to play next track
        if let Some(next_track) = state.queue.dequeue() {
            info!("Skipping to next track after error: {}", next_track.title);
            if let Err(e) = play_track(&self.ctx, &next_track, &mut state).await {
                error!("Failed to play next track after error: {e}");
            }
        }

        None
    }
}

/// Play a track via songbird, updating the guild state
pub async fn play_track(
    ctx: &PlayContext,
    track: &TrackMetadata,
    state: &mut GuildState,
) -> Result<(), String> {
    let handler_lock = ctx.manager
        .get(ctx.guild_id)
        .ok_or_else(|| "Not in a voice channel".to_string())?;

    let mut handler = handler_lock.lock().await;
    debug!("Voice handler locked for guild {}, current_channel={:?}", ctx.guild_id, handler.current_channel());

    let input: Input = if state.normalize {
        debug!("Creating normalized source for: {}", track.url);
        super::normalized_source::create_normalized_source(&track.url)
            .await
            .map_err(|e| format!("Normalized source failed: {e}"))?
    } else {
        debug!("Creating YoutubeDl source for: {}", track.url);
        YoutubeDl::new(ctx.http_client.clone(), track.url.clone()).into()
    };

    debug!("Calling play_input for track: {}", track.title);
    let track_handle = handler.play_input(input);
    debug!("Track submitted to driver for guild {}", ctx.guild_id);

    // Register end-of-track event for auto-advance
    if let Err(e) = track_handle.add_event(
        songbird::Event::Track(TrackEvent::End),
        TrackEndHandler {
            ctx: ctx.clone_ctx(),
        },
    ) {
        warn!("Failed to register track end event: {e}");
    }

    // Register error event handler
    if let Err(e) = track_handle.add_event(
        songbird::Event::Track(TrackEvent::Error),
        TrackErrorHandler {
            ctx: ctx.clone_ctx(),
        },
    ) {
        warn!("Failed to register track error event: {e}");
    }

    state.now_playing = Some(track.clone());
    state.current_track_handle = Some(track_handle);
    state.touch();

    Ok(())
}
