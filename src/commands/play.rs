use serenity::all::{CommandInteraction, Context, ResolvedValue};
use serenity::builder::{
    CreateInteractionResponse, CreateInteractionResponseMessage, EditInteractionResponse,
};
use tracing::{debug, error, info};

use crate::player::events::{play_track, PlayContext};
use crate::queue::track::TrackMetadata;
use crate::state;
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};
use crate::youtube::search;

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;
    info!("Play command in guild {guild_id} by {}", command.user.name);

    // Get the query argument
    let query = command
        .data
        .options()
        .into_iter()
        .find(|o| o.name == "query")
        .and_then(|o| match o.value {
            ResolvedValue::String(s) => Some(s.to_string()),
            _ => None,
        })
        .ok_or(BotError::Other("Missing query argument".into()))?;

    info!("Query: {query}");

    // Defer response since this might take a while
    let defer = CreateInteractionResponse::Defer(
        CreateInteractionResponseMessage::new(),
    );
    command.create_response(&ctx.http, defer).await?;

    // Ensure bot is in voice channel (auto-join)
    let guild = guild_id
        .to_guild_cached(&ctx.cache)
        .ok_or(BotError::NotInGuild)?
        .clone();

    let user_channel = guild
        .voice_states
        .get(&command.user.id)
        .and_then(|vs| vs.channel_id)
        .ok_or(BotError::NotInVoice)?;

    info!("User {} is in voice channel {user_channel}", command.user.name);

    let manager = state::get_songbird(ctx).await?;

    // Remove stale connection before joining
    if let Some(call) = manager.get(guild_id) {
        let current = call.lock().await.current_channel();
        debug!("Existing voice connection for guild {guild_id}: channel={current:?}");
        if current.is_none() {
            info!("Removing stale voice connection for guild {guild_id}");
            let _ = manager.remove(guild_id).await;
        }
    }

    // Join if not already in a channel
    if manager.get(guild_id).is_none() {
        info!("Joining voice channel {user_channel} in guild {guild_id}");
        match manager.join(guild_id, user_channel).await {
            Ok(call) => {
                info!("Successfully joined voice channel {user_channel}");
                debug!("Call lock obtained: {:?}", call.lock().await.current_channel());
            }
            Err(e) => {
                error!("Failed to join voice channel {user_channel}: {e:?}");
                let _ = manager.remove(guild_id).await;
                return Err(BotError::JoinError(format!("{e:?}")));
            }
        }
    } else {
        info!("Already in a voice channel for guild {guild_id}");
    }

    // Resolve the query to a URL + metadata
    info!("Resolving query: {query}");
    let (title, url, duration) = match search::resolve_query(&query).await {
        Ok(result) => {
            info!("Resolved to: {} ({})", result.0, result.1);
            result
        }
        Err(e) => {
            error!("Failed to resolve query '{query}': {e}");
            return Err(e);
        }
    };

    let track = TrackMetadata::new(
        title,
        url,
        duration,
        None,
        command.user.id,
        command.user.name.clone(),
    );

    // Get shared state
    let guild_state_arc = state::get_or_load_guild_state(ctx, guild_id).await?;
    let http_client = state::get_http_client(ctx).await?;
    let redis_pool = state::get_redis_pool(ctx).await;

    let mut gs = guild_state_arc.lock().await;
    gs.text_channel_id = Some(command.channel_id);

    if gs.now_playing.is_none() {
        info!("Nothing playing — starting playback of: {}", track.title);
        // Nothing playing — start playing now
        let play_ctx = PlayContext {
            manager: manager.clone(),
            guild_id,
            guild_state: guild_state_arc.clone(),
            http_client: http_client.clone(),
            discord_http: ctx.http.clone(),
            redis_pool: redis_pool.clone(),
        };
        match play_track(&play_ctx, &track, &mut gs).await
        {
            Ok(()) => info!("Playback started successfully"),
            Err(e) => {
                error!("Failed to start playback: {e}");
                return Err(BotError::PlayFailed(e));
            }
        }

        // Persist
        if let Some(ref pool) = redis_pool {
            let _ = crate::state::redis::save_now_playing(pool, guild_id.get(), gs.now_playing.as_ref()).await;
        }

        let embed = embeds::now_playing_embed(&track);
        let response = EditInteractionResponse::new().embed(embed);
        command.edit_response(&ctx.http, response).await?;
    } else {
        // Already playing — add to queue
        let position = gs.queue.enqueue(track.clone());
        info!("Added to queue at position {position}: {}", track.title);

        // Persist queue
        if let Some(ref pool) = redis_pool {
            let tracks: Vec<_> = gs.queue.tracks.iter().cloned().collect();
            let _ = crate::state::redis::save_queue(pool, guild_id.get(), &tracks).await;
        }

        let embed = embeds::added_to_queue_embed(&track, position);
        let response = EditInteractionResponse::new().embed(embed);
        command.edit_response(&ctx.http, response).await?;
    }

    Ok(())
}
