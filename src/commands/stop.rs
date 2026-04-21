use serenity::all::{CommandInteraction, Context};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};

use crate::state;
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;

    let state_lock = state::get_or_load_guild_state(ctx, guild_id).await?;
    let redis_pool = state::get_redis_pool(ctx).await;
    let mut gs = state_lock.lock().await;

    // Stop current track
    if let Some(ref handle) = gs.current_track_handle {
        let _ = handle.stop();
    }

    // Clear everything
    gs.queue.clear();
    gs.now_playing = None;
    gs.current_track_handle = None;
    gs.touch();

    // Persist
    if let Some(ref pool) = redis_pool {
        let _ = crate::state::redis::save_queue(pool, guild_id.get(), &[]).await;
        let _ = crate::state::redis::save_now_playing(pool, guild_id.get(), None).await;
    }

    let embed = embeds::success_embed("Stopped", "Playback stopped and queue cleared.");
    let response =
        CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed));
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
