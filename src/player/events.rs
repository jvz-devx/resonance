use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serenity::all::Http;
use serenity::model::id::GuildId;
use songbird::Songbird;
use songbird::events::{
    CoreEvent, Event, EventContext, EventHandler as SongbirdEventHandler, TrackEvent,
};
use songbird::input::{Input, YoutubeDl};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::player::media_tools::{build_ytdlp_user_args, classify_media_error};
use crate::queue::track::TrackMetadata;
use crate::state::{GuildState, LoopMode, PlaybackState};
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

static PLAYBACK_ATTEMPT_ID: AtomicU64 = AtomicU64::new(1);

const NORMALIZED_YTDL_TIMEOUT: Duration = Duration::from_secs(30);

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
    pub diagnostics: PlaybackDiagnostics,
}

/// Event handler that fires when a track encounters an error
pub struct TrackErrorHandler {
    pub ctx: PlayContext,
    pub diagnostics: PlaybackDiagnostics,
}

#[derive(Clone)]
pub struct PlaybackDiagnostics {
    pub attempt_id: u64,
    pub track_title: String,
    pub track_url: String,
    pub mode: PlaybackMode,
    pub started_at: Instant,
}

#[derive(Clone, Copy)]
pub enum PlaybackMode {
    Direct,
    Normalized,
}

impl PlaybackMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Normalized => "normalized",
        }
    }
}

pub struct TrackDiagnosticHandler {
    pub guild_id: GuildId,
    pub diagnostics: PlaybackDiagnostics,
    pub event_name: &'static str,
}

pub struct VoiceDiagnosticHandler {
    pub guild_id: GuildId,
    pub event_name: &'static str,
}

#[async_trait::async_trait]
impl SongbirdEventHandler for TrackEndHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        info!(
            "Track ended for guild {} after {:?}: attempt_id={}, mode={}, title={}, url={}",
            self.ctx.guild_id,
            self.diagnostics.started_at.elapsed(),
            self.diagnostics.attempt_id,
            self.diagnostics.mode.as_str(),
            self.diagnostics.track_title,
            self.diagnostics.track_url
        );

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
                    if let Err(e) =
                        crate::state::redis::add_to_history(pool, self.ctx.guild_id.get(), finished)
                            .await
                    {
                        warn!("Failed to save track to history: {e}");
                    }
                }
            }

            if let Err(e) = play_track(&self.ctx, &next_track, &mut state).await {
                error!("Failed to play next track: {e}");
                state.now_playing = None;
                state.current_track_handle = None;
                state.playback_state = PlaybackState::Idle;
                state.touch();

                // Notify text channel about the failure
                if let Some(channel_id) = state.text_channel_id {
                    let embed = embeds::error_embed(&format!(
                        "Failed to play **{}**. {}",
                        next_track.title,
                        e.user_message()
                    ));
                    let builder = serenity::builder::CreateMessage::new().embed(embed);
                    if let Err(e) = channel_id
                        .send_message(&self.ctx.discord_http, builder)
                        .await
                    {
                        warn!("Failed to send error message: {e}");
                    }
                }
            }

            // Persist queue changes
            if let Some(ref pool) = self.ctx.redis_pool {
                let tracks: Vec<_> = state.queue.tracks.iter().cloned().collect();
                if let Err(e) =
                    crate::state::redis::save_queue(pool, self.ctx.guild_id.get(), &tracks).await
                {
                    warn!("Failed to persist queue to Redis: {e}");
                }
                if let Err(e) = crate::state::redis::save_now_playing(
                    pool,
                    self.ctx.guild_id.get(),
                    state.now_playing.as_ref(),
                )
                .await
                {
                    warn!("Failed to persist now_playing to Redis: {e}");
                }
            }

            // Send now-playing embed to text channel
            if let Some(channel_id) = state.text_channel_id {
                if let Some(ref np) = state.now_playing {
                    let embed = embeds::now_playing_embed(np);
                    let builder = serenity::builder::CreateMessage::new().embed(embed);
                    if let Err(e) = channel_id
                        .send_message(&self.ctx.discord_http, builder)
                        .await
                    {
                        warn!("Failed to send now-playing message: {e}");
                    }
                }
            }
        } else {
            info!("Queue empty for guild {}", self.ctx.guild_id);

            // Save finished track to history
            if let Some(ref pool) = self.ctx.redis_pool {
                if let Some(ref finished) = state.now_playing {
                    if let Err(e) =
                        crate::state::redis::add_to_history(pool, self.ctx.guild_id.get(), finished)
                            .await
                    {
                        warn!("Failed to save track to history: {e}");
                    }
                }
            }

            state.now_playing = None;
            state.current_track_handle = None;
            state.playback_state = PlaybackState::Idle;
            state.touch();

            // Persist
            if let Some(ref pool) = self.ctx.redis_pool {
                if let Err(e) =
                    crate::state::redis::save_now_playing(pool, self.ctx.guild_id.get(), None).await
                {
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
        let classified_error = classify_media_error(&error_msg);
        error!(
            "Track error in guild {} after {:?}: attempt_id={}, mode={}, title={}, url={}, error={}; classified={classified_error}",
            self.ctx.guild_id,
            self.diagnostics.started_at.elapsed(),
            self.diagnostics.attempt_id,
            self.diagnostics.mode.as_str(),
            self.diagnostics.track_title,
            self.diagnostics.track_url,
            error_msg
        );

        let mut state = self.ctx.guild_state.lock().await;
        let failed_title = state
            .now_playing
            .as_ref()
            .map(|t| t.title.clone())
            .unwrap_or_default();
        state.now_playing = None;
        state.current_track_handle = None;
        state.playback_state = PlaybackState::Idle;
        state.touch();

        // Notify text channel
        if let Some(channel_id) = state.text_channel_id {
            let message = match &classified_error {
                BotError::AntiBotChallenge => {
                    "YouTube rejected this stream. Verify the POT server and try again."
                }
                BotError::RateLimited => "YouTube is rate-limiting playback right now. Skipping.",
                BotError::StreamNetwork(_) => "The stream dropped mid-playback. Skipping.",
                _ => "Playback failed mid-stream. Skipping to the next track.",
            };
            let embed = embeds::error_embed(&format!(
                "Playback error while playing **{failed_title}**. {message}"
            ));
            let builder = serenity::builder::CreateMessage::new().embed(embed);
            if let Err(e) = channel_id
                .send_message(&self.ctx.discord_http, builder)
                .await
            {
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

#[async_trait::async_trait]
impl SongbirdEventHandler for TrackDiagnosticHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        debug!(
            "Track event context for guild {} event={} attempt_id={}: {:?}",
            self.guild_id, self.event_name, self.diagnostics.attempt_id, ctx
        );
        info!(
            "Track event for guild {} after {:?}: attempt_id={}, event={}, mode={}, title={}, url={}",
            self.guild_id,
            self.diagnostics.started_at.elapsed(),
            self.diagnostics.attempt_id,
            self.event_name,
            self.diagnostics.mode.as_str(),
            self.diagnostics.track_title,
            self.diagnostics.track_url
        );
        None
    }
}

#[async_trait::async_trait]
impl SongbirdEventHandler for VoiceDiagnosticHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            EventContext::DriverConnect(data) | EventContext::DriverReconnect(data) => {
                info!(
                    "Discord voice driver event for guild {}: event={}, server={:?}",
                    self.guild_id, self.event_name, data.server
                );
            }
            EventContext::DriverDisconnect(data) => {
                warn!(
                    "Discord voice driver event for guild {}: event={}, reason={:?}",
                    self.guild_id, self.event_name, data.reason
                );
            }
            _ => {
                debug!(
                    "Discord voice driver event for guild {}: event={}, context={:?}",
                    self.guild_id, self.event_name, ctx
                );
            }
        }
        None
    }
}

pub fn register_voice_diagnostics(call: &mut songbird::Call, guild_id: GuildId) {
    call.add_global_event(
        Event::Core(CoreEvent::DriverConnect),
        VoiceDiagnosticHandler {
            guild_id,
            event_name: "connect",
        },
    );
    call.add_global_event(
        Event::Core(CoreEvent::DriverReconnect),
        VoiceDiagnosticHandler {
            guild_id,
            event_name: "reconnect",
        },
    );
    call.add_global_event(
        Event::Core(CoreEvent::DriverDisconnect),
        VoiceDiagnosticHandler {
            guild_id,
            event_name: "disconnect",
        },
    );
}

pub fn invalidate_prefetch(state: &mut GuildState, reason: &str) {
    state.invalidate_prefetch(reason);
}

pub fn schedule_prefetch(ctx: &PlayContext, state: &mut GuildState, reason: &str) {
    if !state.normalize {
        invalidate_prefetch(state, "normalize-disabled");
        return;
    }

    let Some(next_track) = state.queue.peek().cloned() else {
        invalidate_prefetch(state, "queue-empty");
        return;
    };

    if state
        .prefetched_source
        .as_ref()
        .is_some_and(|prefetch| prefetch.track_url == next_track.url)
    {
        debug!(
            "Prefetch already scheduled for guild {}: reason={}, url={}",
            ctx.guild_id, reason, next_track.url
        );
        return;
    }

    invalidate_prefetch(state, "queue-head-changed");

    let attempt_id = next_attempt_id();
    let url = next_track.url.clone();
    let title = next_track.title.clone();
    info!(
        "Scheduling normalized prefetch for guild {}: reason={}, attempt_id={}, title={}, url={}",
        ctx.guild_id, reason, attempt_id, title, url
    );

    let task_url = url.clone();
    let handle = tokio::spawn(async move {
        let started_at = Instant::now();
        let result = super::normalized_source::create_normalized_source_with_timeout(
            &task_url,
            NORMALIZED_YTDL_TIMEOUT,
            Some(attempt_id),
        )
        .await;
        match &result {
            Ok(_) => info!(
                "Normalized prefetch completed in {:?}: attempt_id={}, url={}",
                started_at.elapsed(),
                attempt_id,
                task_url
            ),
            Err(e) => warn!(
                "Normalized prefetch failed in {:?}: attempt_id={}, url={}, error={e}",
                started_at.elapsed(),
                attempt_id,
                task_url
            ),
        }
        result
    });

    state.prefetched_source = Some(crate::state::PrefetchedSource {
        track_url: url,
        attempt_id,
        started_at: Instant::now(),
        handle,
    });
}

fn next_attempt_id() -> u64 {
    PLAYBACK_ATTEMPT_ID.fetch_add(1, Ordering::Relaxed)
}

fn create_direct_source(ctx: &PlayContext, track: &TrackMetadata) -> Input {
    YoutubeDl::new(ctx.http_client.clone(), track.url.clone())
        .user_args(build_ytdlp_user_args())
        .into()
}

async fn create_playback_input(
    ctx: &PlayContext,
    track: &TrackMetadata,
    state: &mut GuildState,
    attempt_id: u64,
) -> BotResult<(Input, PlaybackMode, bool)> {
    if !state.normalize {
        debug!("Creating YoutubeDl source for: {}", track.url);
        return Ok((
            create_direct_source(ctx, track),
            PlaybackMode::Direct,
            false,
        ));
    }

    let normalized_started_at = Instant::now();
    let normalized_result = if state
        .prefetched_source
        .as_ref()
        .is_some_and(|prefetch| prefetch.track_url == track.url)
    {
        let prefetch = state.prefetched_source.take().expect("prefetch checked");
        info!(
            "Using normalized prefetch for guild {}: playback_attempt_id={}, prefetch_attempt_id={}, age={:?}, title={}, url={}",
            ctx.guild_id,
            attempt_id,
            prefetch.attempt_id,
            prefetch.started_at.elapsed(),
            track.title,
            track.url
        );
        match prefetch.handle.await {
            Ok(result) => result,
            Err(e) if e.is_cancelled() => Err(BotError::ExtractorFailed(
                "normalized prefetch was cancelled".to_string(),
            )),
            Err(e) => Err(BotError::ExtractorFailed(format!(
                "normalized prefetch task failed: {e}"
            ))),
        }
    } else {
        if state.prefetched_source.is_some() {
            invalidate_prefetch(state, "prefetch-stale-for-current-track");
        }
        info!(
            "Normalized prefetch miss for guild {}: playback_attempt_id={}, title={}, url={}",
            ctx.guild_id, attempt_id, track.title, track.url
        );
        super::normalized_source::create_normalized_source_with_timeout(
            &track.url,
            NORMALIZED_YTDL_TIMEOUT,
            Some(attempt_id),
        )
        .await
    };

    match normalized_result {
        Ok(input) => {
            info!(
                "Normalized source ready for guild {} in {:?}: playback_attempt_id={}, title={}",
                ctx.guild_id,
                normalized_started_at.elapsed(),
                attempt_id,
                track.title
            );
            Ok((input, PlaybackMode::Normalized, false))
        }
        Err(e) => {
            warn!(
                "Normalized source failed; falling back to direct playback for guild {} after {:?}: playback_attempt_id={}, title={}, error={e}",
                ctx.guild_id,
                normalized_started_at.elapsed(),
                attempt_id,
                track.title
            );
            Ok((create_direct_source(ctx, track), PlaybackMode::Direct, true))
        }
    }
}

/// Play a track via songbird, updating the guild state
pub async fn play_track(
    ctx: &PlayContext,
    track: &TrackMetadata,
    state: &mut GuildState,
) -> BotResult<()> {
    let handler_lock = ctx
        .manager
        .get(ctx.guild_id)
        .ok_or_else(|| BotError::JoinError("Not in a voice channel".to_string()))?;

    state.playback_state = PlaybackState::Starting;
    state.touch();

    let mut handler = handler_lock.lock().await;
    debug!(
        "Voice handler locked for guild {}, current_channel={:?}",
        ctx.guild_id,
        handler.current_channel()
    );

    let requested_mode = if state.normalize {
        PlaybackMode::Normalized
    } else {
        PlaybackMode::Direct
    };
    let attempt_id = next_attempt_id();
    let source_started_at = Instant::now();
    info!(
        "Preparing playback source for guild {}: attempt_id={}, requested_mode={}, title={}, url={}, queue_len={}",
        ctx.guild_id,
        attempt_id,
        requested_mode.as_str(),
        track.title,
        track.url,
        state.queue.len()
    );

    let (input, mode, used_fallback) = create_playback_input(ctx, track, state, attempt_id).await?;
    info!(
        "Prepared playback source for guild {} in {:?}: attempt_id={}, requested_mode={}, actual_mode={}, normalized_fallback={}, title={}",
        ctx.guild_id,
        source_started_at.elapsed(),
        attempt_id,
        requested_mode.as_str(),
        mode.as_str(),
        used_fallback,
        track.title
    );

    debug!("Calling play_input for track: {}", track.title);
    let submitted_at = Instant::now();
    let track_handle = handler.play_input(input);
    let diagnostics = PlaybackDiagnostics {
        attempt_id,
        track_title: track.title.clone(),
        track_url: track.url.clone(),
        mode,
        started_at: submitted_at,
    };
    info!(
        "Track submitted to driver for guild {}: attempt_id={}, mode={}, normalized_fallback={}, title={}, url={}",
        ctx.guild_id,
        attempt_id,
        mode.as_str(),
        used_fallback,
        track.title,
        track.url
    );

    for (event, event_name) in [
        (TrackEvent::Preparing, "preparing"),
        (TrackEvent::Playable, "playable"),
        (TrackEvent::Play, "play"),
    ] {
        if let Err(e) = track_handle.add_event(
            songbird::Event::Track(event),
            TrackDiagnosticHandler {
                guild_id: ctx.guild_id,
                diagnostics: diagnostics.clone(),
                event_name,
            },
        ) {
            warn!("Failed to register track {event_name} event: {e}");
        }
    }

    // Register end-of-track event for auto-advance
    if let Err(e) = track_handle.add_event(
        songbird::Event::Track(TrackEvent::End),
        TrackEndHandler {
            ctx: ctx.clone_ctx(),
            diagnostics: diagnostics.clone(),
        },
    ) {
        warn!("Failed to register track end event: {e}");
    }

    // Register error event handler
    if let Err(e) = track_handle.add_event(
        songbird::Event::Track(TrackEvent::Error),
        TrackErrorHandler {
            ctx: ctx.clone_ctx(),
            diagnostics,
        },
    ) {
        warn!("Failed to register track error event: {e}");
    }

    state.now_playing = Some(track.clone());
    state.current_track_handle = Some(track_handle);
    state.playback_state = PlaybackState::Playing;
    state.touch();
    schedule_prefetch(ctx, state, "track-started");

    Ok(())
}
