use std::time::{Duration, Instant};

use serenity::all::{Context, EventHandler, Interaction, Reaction, Ready};
use serenity::async_trait;
use serenity::builder::{
    CreateInteractionResponse, CreateInteractionResponseMessage, EditInteractionResponse,
};
use serenity::model::application::Command;
use serenity::model::channel::ReactionType;
use tracing::{debug, error, info, warn};

use crate::commands;
use crate::commands::search::emoji_to_index;
use crate::player::events::{PlayContext, play_track};
use crate::queue::track::TrackMetadata;
use crate::state;
use crate::utils::embeds;

pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);
        info!(
            "Invite link: https://discord.com/oauth2/authorize?client_id={}&permissions=3165184&scope=bot%20applications.commands",
            ready.user.id
        );

        // Set online presence
        ctx.set_presence(
            Some(serenity::gateway::ActivityData::listening("/play")),
            serenity::model::user::OnlineStatus::Online,
        );

        // Log which guilds the bot is in
        info!("Bot is in {} guilds:", ready.guilds.len());
        for guild in &ready.guilds {
            info!("  Guild ID: {}", guild.id);
        }

        // Register slash commands
        let commands_to_register = commands::all_commands();

        // Register as guild-specific commands (instant) for each guild
        for guild in &ready.guilds {
            match guild
                .id
                .set_commands(&ctx.http, commands_to_register.clone())
                .await
            {
                Ok(cmds) => {
                    info!(
                        "Registered {} guild commands for guild {}",
                        cmds.len(),
                        guild.id
                    );
                }
                Err(e) => error!("Failed to register guild commands for {}: {e}", guild.id),
            }
        }

        // Clear any stale global commands (guild commands are sufficient)
        if let Err(e) = Command::set_global_commands(&ctx.http, Vec::new()).await {
            warn!("Failed to clear global slash commands: {e}");
        }

        // Spawn background tasks
        spawn_pending_search_cleanup(ctx.clone());
        spawn_auto_disconnect(ctx.clone());
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            let command_name = command.data.name.clone();
            info!(
                "Slash command: /{} by {} in {:?}",
                command_name, command.user.name, command.guild_id
            );

            let result = match command_name.as_str() {
                "play" => commands::play::run(&ctx, &command).await,
                "search" => commands::search::run(&ctx, &command).await,
                "queue" => commands::queue::run(&ctx, &command).await,
                "skip" => commands::skip::run(&ctx, &command).await,
                "pause" => commands::pause::run(&ctx, &command).await,
                "resume" => commands::resume::run(&ctx, &command).await,
                "stop" => commands::stop::run(&ctx, &command).await,
                "nowplaying" => commands::nowplaying::run(&ctx, &command).await,
                "shuffle" => commands::shuffle::run(&ctx, &command).await,
                "loop" => commands::loop_cmd::run(&ctx, &command).await,
                "normalize" => commands::normalize::run(&ctx, &command).await,
                "remove" => commands::remove::run(&ctx, &command).await,
                "clear" => commands::clear::run(&ctx, &command).await,
                "join" => commands::join::run(&ctx, &command).await,
                "leave" => commands::leave::run(&ctx, &command).await,
                _ => {
                    warn!("Unknown command: {command_name}");
                    Ok(())
                }
            };

            if let Err(e) = result {
                error!("Command /{command_name} error: {e}");

                // Try to send error as ephemeral response
                let embed = embeds::error_embed(&e.user_message());

                // Try editing deferred response first, fall back to new response
                let edit_result = command
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().embed(embed.clone()),
                    )
                    .await;

                if edit_result.is_err() {
                    let response = CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .embed(embed)
                            .ephemeral(true),
                    );
                    let _ = command.create_response(&ctx.http, response).await;
                }
            }
        }
    }

    async fn reaction_add(&self, ctx: Context, reaction: Reaction) {
        if let Err(e) = handle_reaction(&ctx, &reaction).await {
            error!("Reaction handler error: {e}");
        }
    }
}

/// Handle a reaction add event — check if it's a search selection
async fn handle_reaction(
    ctx: &Context,
    reaction: &Reaction,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Ignore bot's own reactions
    let reactor_id = match reaction.user_id {
        Some(id) => id,
        None => return Ok(()),
    };

    if reactor_id == ctx.cache.current_user().id {
        return Ok(());
    }

    // Check if this is a numbered emoji reaction
    let emoji_str = match &reaction.emoji {
        ReactionType::Unicode(s) => s.clone(),
        _ => return Ok(()),
    };

    let index = match emoji_to_index(&emoji_str) {
        Some(i) => i,
        None => return Ok(()),
    };

    // Look up the pending search
    let pending_searches = match state::get_pending_searches(ctx).await {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };

    let pending_data = {
        if let Some((_, search)) = pending_searches.remove(&reaction.message_id) {
            // Verify it's the right user and not expired
            if search.user_id != reactor_id {
                // Not our user — put it back
                pending_searches.insert(reaction.message_id, search);
                return Ok(());
            }
            if search.expires_at < Instant::now() {
                return Ok(());
            }
            if index >= search.results.len() {
                // Out of bounds — put it back
                pending_searches.insert(reaction.message_id, search);
                return Ok(());
            }

            Some((
                search.results[index].clone(),
                search.guild_id,
                search.channel_id,
            ))
        } else {
            None
        }
    };

    let ((title, url, duration), guild_id, channel_id) = match pending_data {
        Some(data) => data,
        None => return Ok(()),
    };

    info!(
        "Search selection: {} chose '{}' in guild {}",
        reactor_id, title, guild_id
    );

    // Get the user's name
    let requester_name = reaction
        .user(&ctx.http)
        .await
        .map(|u| u.name.clone())
        .unwrap_or_else(|_| "Unknown".into());

    let track = TrackMetadata::new(
        title.clone(),
        url,
        duration,
        None,
        reactor_id,
        requester_name,
    );

    // Auto-join voice if needed
    let guild = match guild_id.to_guild_cached(&ctx.cache) {
        Some(g) => g.clone(),
        None => return Ok(()),
    };

    let user_channel = match guild
        .voice_states
        .get(&reactor_id)
        .and_then(|vs| vs.channel_id)
    {
        Some(ch) => ch,
        None => {
            let _ = channel_id
                .say(
                    &ctx.http,
                    "You need to be in a voice channel to select a track.",
                )
                .await;
            return Ok(());
        }
    };

    let manager = state::get_songbird(ctx).await?;

    // Remove stale connection before joining
    if let Some(call) = manager.get(guild_id) {
        let current = call.lock().await.current_channel();
        debug!("Existing voice connection for guild {guild_id}: channel={current:?}");
        if current.is_none() {
            debug!("Removing stale voice connection for guild {guild_id}");
            let _ = manager.remove(guild_id).await;
        }
    }

    if manager.get(guild_id).is_none() {
        debug!("Joining voice channel {user_channel} in guild {guild_id} (search selection)");
        match manager.join(guild_id, user_channel).await {
            Ok(call) => {
                let ch = call.lock().await.current_channel();
                debug!("Voice join succeeded for guild {guild_id}: channel={ch:?}");
            }
            Err(e) => {
                error!("Failed to join voice channel {user_channel} in guild {guild_id}: {e:?}");
                let _ = manager.remove(guild_id).await;
                return Ok(());
            }
        }
    } else {
        debug!("Already in voice channel for guild {guild_id}, skipping join");
    }

    // Get shared state
    let guild_state_arc = state::get_or_load_guild_state(ctx, guild_id).await?;
    let http_client = state::get_http_client(ctx).await?;
    let redis_pool = state::get_redis_pool(ctx).await;

    let mut gs = guild_state_arc.lock().await;
    gs.text_channel_id = Some(channel_id);

    if gs.now_playing.is_none() {
        debug!(
            "Nothing playing in guild {guild_id} — starting playback of: {}",
            track.title
        );
        let play_ctx = PlayContext {
            manager: manager.clone(),
            guild_id,
            guild_state: guild_state_arc.clone(),
            http_client: http_client.clone(),
            discord_http: ctx.http.clone(),
            redis_pool: redis_pool.clone(),
        };
        match play_track(&play_ctx, &track, &mut gs).await {
            Ok(()) => {
                debug!("Playback started successfully in guild {guild_id}");
                if let Some(ref pool) = redis_pool {
                    if let Err(e) = crate::state::redis::save_now_playing(
                        pool,
                        guild_id.get(),
                        gs.now_playing.as_ref(),
                    )
                    .await
                    {
                        warn!("Failed to persist now_playing to Redis: {e}");
                    }
                }
                let embed = embeds::now_playing_embed(&track);
                let builder = serenity::builder::CreateMessage::new().embed(embed);
                let _ = channel_id.send_message(&ctx.http, builder).await;
            }
            Err(e) => {
                error!("Failed to play selected track: {e}");
                let _ = channel_id
                    .say(&ctx.http, format!("Failed to play: {}", e.user_message()))
                    .await;
            }
        }
    } else {
        let position = gs.queue.enqueue(track.clone());
        if let Some(ref pool) = redis_pool {
            let tracks: Vec<_> = gs.queue.tracks.iter().cloned().collect();
            if let Err(e) = crate::state::redis::save_queue(pool, guild_id.get(), &tracks).await {
                warn!("Failed to persist queue to Redis: {e}");
            }
        }
        let embed = embeds::added_to_queue_embed(&track, position);
        let builder = serenity::builder::CreateMessage::new().embed(embed);
        let _ = channel_id.send_message(&ctx.http, builder).await;
    }

    // Edit original search message to show selection
    if let Ok(mut msg) = ctx.http.get_message(channel_id, reaction.message_id).await {
        let edit = serenity::builder::EditMessage::new().embed(embeds::success_embed(
            "Selected",
            &format!("Playing **{title}**"),
        ));
        let _ = msg.edit(&ctx.http, edit).await;
    }

    Ok(())
}

/// Background task: clean up expired pending searches every 10 seconds
fn spawn_pending_search_cleanup(ctx: Context) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;

            match state::get_pending_searches(&ctx).await {
                Ok(pending) => {
                    let now = Instant::now();
                    pending.retain(|_msg_id, search| search.expires_at > now);
                }
                Err(e) => {
                    warn!("Could not access pending searches for cleanup: {e}");
                }
            }
        }
    });
}

/// Background task: auto-disconnect from voice after idle timeout
fn spawn_auto_disconnect(ctx: Context) {
    tokio::spawn(async move {
        let idle_secs = state::get_idle_timeout(&ctx).await;
        let idle_timeout = Duration::from_secs(idle_secs);

        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;

            let manager = match state::get_songbird(&ctx).await {
                Ok(m) => m,
                Err(e) => {
                    warn!("Could not access songbird for auto-disconnect: {e}");
                    continue;
                }
            };

            let guild_states = match state::get_guild_states(&ctx).await {
                Ok(gs) => gs,
                Err(e) => {
                    warn!("Could not access guild states for auto-disconnect: {e}");
                    continue;
                }
            };

            let mut guilds_to_leave = Vec::new();

            for entry in guild_states.iter() {
                let guild_id = *entry.key();
                let state_lock = entry.value().clone();
                let gs = state_lock.lock().await;

                if gs.is_idle_for(idle_timeout) && manager.get(guild_id).is_some() {
                    info!(
                        "Auto-disconnecting from guild {} (idle for 5+ minutes)",
                        guild_id
                    );
                    guilds_to_leave.push(guild_id);
                }
            }

            let redis_pool = state::get_redis_pool(&ctx).await;

            for guild_id in guilds_to_leave {
                let _ = manager.remove(guild_id).await;

                // Clean up state
                if let Some(entry) = guild_states.get(&guild_id) {
                    let mut gs = entry.value().lock().await;
                    gs.queue.clear();
                    gs.now_playing = None;
                    gs.current_track_handle = None;
                }

                // Remove from DashMap to prevent memory leak
                guild_states.remove(&guild_id);

                // Clear Redis data
                if let Some(ref pool) = redis_pool {
                    let _ = crate::state::redis::save_queue(pool, guild_id.get(), &[]).await;
                    let _ = crate::state::redis::save_now_playing(pool, guild_id.get(), None).await;
                }
            }
        }
    });
}
