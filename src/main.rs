mod bot;
mod commands;
mod player;
mod queue;
mod state;
mod utils;
mod youtube;

use std::sync::Arc;

use dashmap::DashMap;
use serenity::prelude::*;
use songbird::SerenityInit;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::state::{GuildStates, HttpClientKey, IdleTimeoutKey, PendingSearches, RedisPoolKey};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env file (non-fatal if missing)
    dotenvy::dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Read configuration
    let token = std::env::var("DISCORD_TOKEN").map_err(|_| {
        "DISCORD_TOKEN environment variable is not set. \
         Create a .env file or export DISCORD_TOKEN=<your token>"
    })?;

    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    let idle_timeout_secs: u64 = std::env::var("IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);

    // Create Redis pool (non-fatal — bot works without Redis, just no persistence)
    let redis_pool = match state::redis::create_pool(&redis_url) {
        Ok(pool) => {
            info!("Redis connection pool created (url: {redis_url})");
            Some(pool)
        }
        Err(e) => {
            warn!("Failed to create Redis pool: {e}");
            warn!("Continuing without Redis — queue state will not survive restarts");
            None
        }
    };

    // Create shared HTTP client for songbird/ytdl
    let http_client = reqwest::Client::new();

    // Configure gateway intents (no privileged intents needed — slash commands only)
    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_VOICE_STATES
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILD_MESSAGE_REACTIONS;

    // Build the client
    let mut client = Client::builder(&token, intents)
        .activity(serenity::gateway::ActivityData::listening("/play"))
        .status(serenity::model::user::OnlineStatus::Online)
        .event_handler(bot::Handler)
        .register_songbird()
        .await
        .map_err(|e| format!("Failed to create Discord client: {e}"))?;

    // Insert shared state into TypeMap
    {
        let mut data = client.data.write().await;
        data.insert::<GuildStates>(Arc::new(DashMap::new()));
        data.insert::<PendingSearches>(Arc::new(DashMap::new()));
        data.insert::<HttpClientKey>(http_client);
        data.insert::<IdleTimeoutKey>(idle_timeout_secs);
        if let Some(pool) = redis_pool {
            data.insert::<RedisPoolKey>(pool);
        }
    }

    info!("Starting Resonance (idle timeout: {idle_timeout_secs}s)...");

    // Run the bot with graceful shutdown on Ctrl+C
    let shard_manager = client.shard_manager.clone();

    tokio::select! {
        result = client.start() => {
            if let Err(e) = result {
                error!("Client error: {e}");
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal, disconnecting...");
            shard_manager.shutdown_all().await;
            info!("Shutdown complete.");
        }
    }

    Ok(())
}
