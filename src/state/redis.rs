use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool;

use crate::queue::track::TrackMetadata;
use crate::state::LoopMode;
use crate::utils::error::{BotError, BotResult};

const KEY_PREFIX: &str = "musicbot";

fn queue_key(guild_id: u64) -> String {
    format!("{KEY_PREFIX}:queue:{guild_id}")
}

fn nowplaying_key(guild_id: u64) -> String {
    format!("{KEY_PREFIX}:nowplaying:{guild_id}")
}

fn settings_key(guild_id: u64) -> String {
    format!("{KEY_PREFIX}:settings:{guild_id}")
}

fn history_key(guild_id: u64) -> String {
    format!("{KEY_PREFIX}:history:{guild_id}")
}

/// Save the entire queue to Redis (replaces existing)
pub async fn save_queue(pool: &Pool, guild_id: u64, tracks: &[TrackMetadata]) -> BotResult<()> {
    let mut conn = pool.get().await?;
    let key = queue_key(guild_id);

    // Delete existing queue then push all
    let _: () = deadpool_redis::redis::cmd("DEL")
        .arg(&key)
        .query_async(&mut conn)
        .await?;

    if !tracks.is_empty() {
        let serialized: Vec<String> = tracks
            .iter()
            .filter_map(|t| serde_json::to_string(t).ok())
            .collect();

        let _: () = conn.rpush(&key, serialized).await?;
    }

    Ok(())
}

#[allow(dead_code)]
/// Load the queue from Redis
pub async fn load_queue(pool: &Pool, guild_id: u64) -> BotResult<Vec<TrackMetadata>> {
    let mut conn = pool.get().await?;
    let key = queue_key(guild_id);

    let items: Vec<String> = conn.lrange(&key, 0, -1).await?;

    let tracks: Vec<TrackMetadata> = items
        .iter()
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect();

    Ok(tracks)
}

/// Save the currently playing track
pub async fn save_now_playing(
    pool: &Pool,
    guild_id: u64,
    track: Option<&TrackMetadata>,
) -> BotResult<()> {
    let mut conn = pool.get().await?;
    let key = nowplaying_key(guild_id);

    match track {
        Some(t) => {
            let json = serde_json::to_string(t)
                .map_err(|e| BotError::Other(format!("Failed to serialize track: {e}")))?;
            let _: () = conn.set(&key, json).await?;
        }
        None => {
            let _: () = conn.del(&key).await?;
        }
    }

    Ok(())
}

#[allow(dead_code)]
/// Load the currently playing track
pub async fn load_now_playing(pool: &Pool, guild_id: u64) -> BotResult<Option<TrackMetadata>> {
    let mut conn = pool.get().await?;
    let key = nowplaying_key(guild_id);

    let json: Option<String> = conn.get(&key).await?;

    Ok(json.and_then(|s| serde_json::from_str(&s).ok()))
}

/// Save guild settings (loop mode, etc.)
pub async fn save_settings(pool: &Pool, guild_id: u64, loop_mode: &LoopMode) -> BotResult<()> {
    let mut conn = pool.get().await?;
    let key = settings_key(guild_id);

    let _: () = conn.hset(&key, "loop_mode", loop_mode.to_string()).await?;

    Ok(())
}

#[allow(dead_code)]
/// Load guild settings
pub async fn load_loop_mode(pool: &Pool, guild_id: u64) -> BotResult<LoopMode> {
    let mut conn = pool.get().await?;
    let key = settings_key(guild_id);

    let mode: Option<String> = conn.hget(&key, "loop_mode").await?;

    // Default to LoopMode::Off if no setting stored or unrecognized value
    Ok(mode
        .and_then(|s| LoopMode::from_str(&s))
        .unwrap_or(LoopMode::Off))
}

/// Add a track to play history (capped at 100)
pub async fn add_to_history(pool: &Pool, guild_id: u64, track: &TrackMetadata) -> BotResult<()> {
    let mut conn = pool.get().await?;
    let key = history_key(guild_id);

    let json = serde_json::to_string(track)
        .map_err(|e| BotError::Other(format!("Failed to serialize track for history: {e}")))?;
    let _: () = conn.lpush(&key, &json).await?;
    let _: () = conn.ltrim(&key, 0, 99).await?;

    Ok(())
}

#[allow(dead_code)]
/// Load play history
pub async fn load_history(pool: &Pool, guild_id: u64) -> BotResult<Vec<TrackMetadata>> {
    let mut conn = pool.get().await?;
    let key = history_key(guild_id);

    let items: Vec<String> = conn.lrange(&key, 0, -1).await?;

    let tracks: Vec<TrackMetadata> = items
        .iter()
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect();

    Ok(tracks)
}

#[allow(dead_code)]
/// Clear all Redis state for a guild
pub async fn clear_guild(pool: &Pool, guild_id: u64) -> BotResult<()> {
    let mut conn = pool.get().await?;

    let _: () = deadpool_redis::redis::cmd("DEL")
        .arg(queue_key(guild_id))
        .arg(nowplaying_key(guild_id))
        .arg(settings_key(guild_id))
        .query_async(&mut conn)
        .await?;

    Ok(())
}

/// Create the Redis connection pool from URL
pub fn create_pool(redis_url: &str) -> Result<Pool, String> {
    let cfg = deadpool_redis::Config::from_url(redis_url);
    cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .map_err(|e| format!("Failed to create Redis pool: {e}"))
}
