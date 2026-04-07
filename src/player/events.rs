use std::sync::Arc;

use serenity::all::Http;
use serenity::model::id::GuildId;
use songbird::events::{Event, EventContext, EventHandler as SongbirdEventHandler, TrackEvent};
use songbird::input::YoutubeDl;
use songbird::Songbird;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::queue::track::TrackMetadata;
use crate::state::{GuildState, LoopMode};
use crate::utils::embeds;

/// Event handler that fires when a track ends — plays the next track from queue
pub struct TrackEndHandler {
    pub guild_id: GuildId,
    pub guild_state: Arc<Mutex<GuildState>>,
    pub manager: Arc<Songbird>,
    pub http_client: reqwest::Client,
    pub discord_http: Arc<Http>,
    pub redis_pool: Option<deadpool_redis::Pool>,
}

#[async_trait::async_trait]
impl SongbirdEventHandler for TrackEndHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        info!("Track ended for guild {}", self.guild_id);

        let mut state = self.guild_state.lock().await;

        // Handle loop modes
        if let Some(ref current) = state.now_playing.clone() {
            match state.loop_mode {
                LoopMode::Track => {
                    debug!("Loop track: replaying current track");
                    if let Err(e) = play_track(
                        &self.manager,
                        self.guild_id,
                        current,
                        &self.http_client,
                        &mut state,
                        self.guild_state.clone(),
                        self.manager.clone(),
                        self.http_client.clone(),
                        self.discord_http.clone(),
                        self.redis_pool.clone(),
                    )
                    .await
                    {
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
            if let Some(ref pool) = self.redis_pool {
                if let Some(ref finished) = state.now_playing {
                    if let Err(e) = crate::state::redis::add_to_history(pool, self.guild_id.get(), finished).await {
                        warn!("Failed to save track to history: {e}");
                    }
                }
            }

            if let Err(e) = play_track(
                &self.manager,
                self.guild_id,
                &next_track,
                &self.http_client,
                &mut state,
                self.guild_state.clone(),
                self.manager.clone(),
                self.http_client.clone(),
                self.discord_http.clone(),
                self.redis_pool.clone(),
            )
            .await
            {
                error!("Failed to play next track: {e}");
                state.now_playing = None;
                state.current_track_handle = None;
            }

            // Persist queue changes
            if let Some(ref pool) = self.redis_pool {
                let tracks: Vec<_> = state.queue.tracks.iter().cloned().collect();
                if let Err(e) = crate::state::redis::save_queue(pool, self.guild_id.get(), &tracks).await {
                    warn!("Failed to persist queue to Redis: {e}");
                }
                if let Err(e) = crate::state::redis::save_now_playing(pool, self.guild_id.get(), state.now_playing.as_ref()).await {
                    warn!("Failed to persist now_playing to Redis: {e}");
                }
            }

            // Send now-playing embed to text channel
            if let Some(channel_id) = state.text_channel_id {
                if let Some(ref np) = state.now_playing {
                    let embed = embeds::now_playing_embed(np);
                    let builder = serenity::builder::CreateMessage::new().embed(embed);
                    if let Err(e) = channel_id.send_message(&self.discord_http, builder).await {
                        warn!("Failed to send now-playing message: {e}");
                    }
                }
            }
        } else {
            info!("Queue empty for guild {}", self.guild_id);

            // Save finished track to history
            if let Some(ref pool) = self.redis_pool {
                if let Some(ref finished) = state.now_playing {
                    if let Err(e) = crate::state::redis::add_to_history(pool, self.guild_id.get(), finished).await {
                        warn!("Failed to save track to history: {e}");
                    }
                }
            }

            state.now_playing = None;
            state.current_track_handle = None;
            state.touch();

            // Persist
            if let Some(ref pool) = self.redis_pool {
                if let Err(e) = crate::state::redis::save_now_playing(pool, self.guild_id.get(), None).await {
                    warn!("Failed to clear now_playing in Redis: {e}");
                }
            }
        }

        None
    }
}

/// Play a track via songbird, updating the guild state
pub async fn play_track(
    manager: &Arc<Songbird>,
    guild_id: GuildId,
    track: &TrackMetadata,
    http_client: &reqwest::Client,
    state: &mut GuildState,
    guild_state_arc: Arc<Mutex<GuildState>>,
    manager_clone: Arc<Songbird>,
    http_clone: reqwest::Client,
    discord_http: Arc<Http>,
    redis_pool: Option<deadpool_redis::Pool>,
) -> Result<(), String> {
    let handler_lock = manager
        .get(guild_id)
        .ok_or_else(|| "Not in a voice channel".to_string())?;

    let mut handler = handler_lock.lock().await;

    let source = YoutubeDl::new(http_client.clone(), track.url.clone());
    let track_handle = handler.play_input(source.into());

    // Register end-of-track event for auto-advance
    if let Err(e) = track_handle.add_event(
        songbird::Event::Track(TrackEvent::End),
        TrackEndHandler {
            guild_id,
            guild_state: guild_state_arc,
            manager: manager_clone,
            http_client: http_clone,
            discord_http,
            redis_pool,
        },
    ) {
        warn!("Failed to register track end event: {e}");
    }

    state.now_playing = Some(track.clone());
    state.current_track_handle = Some(track_handle);
    state.touch();

    Ok(())
}
