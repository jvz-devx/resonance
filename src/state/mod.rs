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

/// Helper to get or create a guild state entry
pub fn get_or_create_guild_state(
    states: &DashMap<GuildId, Arc<Mutex<GuildState>>>,
    guild_id: GuildId,
) -> Arc<Mutex<GuildState>> {
    states
        .entry(guild_id)
        .or_insert_with(|| Arc::new(Mutex::new(GuildState::new())))
        .value()
        .clone()
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
