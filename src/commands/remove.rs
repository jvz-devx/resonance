use serenity::all::{CommandInteraction, Context, ResolvedValue};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};

use crate::state::{self, get_or_create_guild_state};
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;

    let position = command
        .data
        .options()
        .into_iter()
        .find(|o| o.name == "position")
        .and_then(|o| match o.value {
            ResolvedValue::Integer(i) => Some(i as usize),
            _ => None,
        })
        .ok_or(BotError::Other("Missing position argument".into()))?;

    let guild_states = state::get_guild_states(ctx).await?;
    let state_lock = get_or_create_guild_state(&guild_states, guild_id);
    let redis_pool = state::get_redis_pool(ctx).await;
    let mut gs = state_lock.lock().await;

    if let Some(removed) = gs.queue.remove(position) {
        // Persist
        if let Some(ref pool) = redis_pool {
            let tracks: Vec<_> = gs.queue.tracks.iter().cloned().collect();
            let _ = crate::state::redis::save_queue(pool, guild_id.get(), &tracks).await;
        }

        let embed = embeds::success_embed(
            "Removed",
            &format!("Removed **{}** from position #{position}.", removed.title),
        );
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().embed(embed),
        );
        command.create_response(&ctx.http, response).await?;
    } else {
        let embed = embeds::error_embed(&format!(
            "Invalid position {position}. Queue has {} tracks.",
            gs.queue.len()
        ));
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
    }

    Ok(())
}
