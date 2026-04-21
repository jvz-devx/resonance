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

    if gs.queue.len() <= 1 {
        let embed = embeds::error_embed("Not enough tracks to shuffle.");
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    gs.queue.shuffle();

    // Persist
    if let Some(ref pool) = redis_pool {
        let tracks: Vec<_> = gs.queue.tracks.iter().cloned().collect();
        let _ = crate::state::redis::save_queue(pool, guild_id.get(), &tracks).await;
    }

    let embed = embeds::success_embed(
        "Shuffled",
        &format!("Shuffled {} tracks in the queue.", gs.queue.len()),
    );
    let response =
        CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed));
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
