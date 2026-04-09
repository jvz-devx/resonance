pub mod redis;

use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId, MessageId, UserId};
use serenity::prelude::TypeMapKey;
use songbird::tracks::TrackHandle;
use tokio::sync::Mutex;

use crate::queue::track::TrackMetadata;
use crate::queue::QueueManager;

pub const DEFAULT_NORMALIZE: bool = true;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LoopMode {
    Off,
    Track,
    Queue,
}

impl Default for LoopMode {
    fn default() -> Self {
        Self::Off
    }
}

impl std::fmt::Display for LoopMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => write!(f, "off"),
            Self::Track => write!(f, "track"),
            Self::Queue => write!(f, "queue"),
        }
    }
}

impl LoopMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "off" => Some(Self::Off),
            "track" => Some(Self::Track),
            "queue" => Some(Self::Queue),
            _ => None,
        }
    }
}

/// Per-guild state holding queue, playback info, and settings
pub struct GuildState {
    pub queue: QueueManager,
    pub now_playing: Option<TrackMetadata>,
    pub loop_mode: LoopMode,
    pub normalize: bool,
    pub current_track_handle: Option<TrackHandle>,
    pub text_channel_id: Option<ChannelId>,
    pub last_activity: Instant,
}

impl GuildState {
    pub fn new() -> Self {
        Self {
            queue: QueueManager::new(),
            now_playing: None,
            loop_mode: LoopMode::Off,
            normalize: DEFAULT_NORMALIZE,
            current_track_handle: None,
            text_channel_id: None,
            last_activity: Instant::now(),
        }
    }

    /// Mark activity (resets idle timer)
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Check if the guild has been idle for the given duration
    pub fn is_idle_for(&self, duration: std::time::Duration) -> bool {
        self.now_playing.is_none() && self.last_activity.elapsed() >= duration
    }
}

impl Default for GuildState {
    fn default() -> Self {
        Self::new()
    }
}

/// Pending search result awaiting user reaction
pub struct PendingSearch {
    pub user_id: UserId,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    pub results: Vec<(String, String, Option<std::time::Duration>)>, // (title, url, duration)
    pub expires_at: Instant,
}

// --- TypeMap keys for serenity's shared data ---

pub struct GuildStates;
impl TypeMapKey for GuildStates {
    type Value = Arc<DashMap<GuildId, Arc<Mutex<GuildState>>>>;
}

pub struct RedisPoolKey;
impl TypeMapKey for RedisPoolKey {
    type Value = deadpool_redis::Pool;
}

pub struct HttpClientKey;
impl TypeMapKey for HttpClientKey {
    type Value = reqwest::Client;
}

pub struct PendingSearches;
impl TypeMapKey for PendingSearches {
    type Value = Arc<DashMap<MessageId, PendingSearch>>;
}

pub struct IdleTimeoutKey;
impl TypeMapKey for IdleTimeoutKey {
    type Value = u64;
}

// --- Safe accessors for TypeMap data (no unwrap/expect) ---

use crate::utils::error::{BotError, BotResult};
use serenity::all::Context;
use songbird::Songbird;

/// Safely retrieve the songbird manager from context
pub async fn get_songbird(ctx: &Context) -> BotResult<Arc<Songbird>> {
    songbird::get(ctx)
        .await
        .ok_or_else(|| BotError::StateMissing("Songbird voice manager not registered".into()))
}

/// Safely retrieve guild states from the TypeMap
pub async fn get_guild_states(
    ctx: &Context,
) -> BotResult<Arc<DashMap<GuildId, Arc<Mutex<GuildState>>>>> {
    let data = ctx.data.read().await;
    data.get::<GuildStates>()
        .cloned()
        .ok_or_else(|| BotError::StateMissing("GuildStates not found in TypeMap".into()))
}

/// Safely retrieve the HTTP client from the TypeMap
pub async fn get_http_client(ctx: &Context) -> BotResult<reqwest::Client> {
    let data = ctx.data.read().await;
    data.get::<HttpClientKey>()
        .cloned()
        .ok_or_else(|| BotError::StateMissing("HttpClient not found in TypeMap".into()))
}

/// Retrieve the Redis pool (returns None if Redis is not configured — not an error)
pub async fn get_redis_pool(ctx: &Context) -> Option<deadpool_redis::Pool> {
    let data = ctx.data.read().await;
    data.get::<RedisPoolKey>().cloned()
}

/// Get an existing guild state, or create one and populate it from Redis if available.
///
/// This is the only way to obtain guild state — it ensures Redis-backed settings
/// are loaded on first access.
pub async fn get_or_load_guild_state(
    ctx: &Context,
    guild_id: GuildId,
) -> BotResult<Arc<Mutex<GuildState>>> {
    let states = get_guild_states(ctx).await?;

    if let Some(existing) = states.get(&guild_id) {
        return Ok(existing.value().clone());
    }

    let mut new_state = GuildState::new();

    if let Some(pool) = get_redis_pool(ctx).await {
        match crate::state::redis::load_settings(&pool, guild_id.get()).await {
            Ok(settings) => {
                new_state.loop_mode = settings.loop_mode;
                new_state.normalize = settings.normalize;
            }
            Err(e) => tracing::warn!("Failed to load guild settings from Redis: {e}"),
        }
    }

    let arc = Arc::new(Mutex::new(new_state));
    let inserted = states
        .entry(guild_id)
        .or_insert_with(|| arc.clone())
        .value()
        .clone();
    Ok(inserted)
}

/// Retrieve the configured idle timeout in seconds (default 300)
pub async fn get_idle_timeout(ctx: &Context) -> u64 {
    let data = ctx.data.read().await;
    data.get::<IdleTimeoutKey>().copied().unwrap_or(300)
}

/// Retrieve pending searches map
pub async fn get_pending_searches(
    ctx: &Context,
) -> BotResult<Arc<DashMap<MessageId, PendingSearch>>> {
    let data = ctx.data.read().await;
    data.get::<PendingSearches>()
        .cloned()
        .ok_or_else(|| BotError::StateMissing("PendingSearches not found in TypeMap".into()))
}
