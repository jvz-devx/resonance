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

    let count = gs.queue.len();
    gs.queue.clear();

    // Persist
    if let Some(ref pool) = redis_pool {
        let _ = crate::state::redis::save_queue(pool, guild_id.get(), &[]).await;
    }

    let embed = if count > 0 {
        embeds::success_embed(
            "Cleared",
            &format!("Removed {count} tracks from the queue."),
        )
    } else {
        embeds::success_embed("Cleared", "The queue was already empty.")
    };

    let response =
        CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed));
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
