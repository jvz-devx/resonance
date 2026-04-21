use serenity::all::{CommandInteraction, Context, ResolvedValue};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};

use crate::state;
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;

    let explicit = command
        .data
        .options()
        .into_iter()
        .find(|o| o.name == "enabled")
        .and_then(|o| match o.value {
            ResolvedValue::String(s) => Some(s == "on"),
            _ => None,
        });

    let state_lock = state::get_or_load_guild_state(ctx, guild_id).await?;
    let redis_pool = state::get_redis_pool(ctx).await;
    let mut gs = state_lock.lock().await;

    gs.normalize = explicit.unwrap_or(!gs.normalize);

    // Persist
    if let Some(ref pool) = redis_pool {
        let _ = crate::state::redis::save_normalize(pool, guild_id.get(), gs.normalize).await;
    }

    let description = if gs.normalize {
        "Sound normalisation is now **on**."
    } else {
        "Sound normalisation is now **off**."
    };

    let embed = embeds::success_embed("Normalisation", description);
    let response =
        CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed));
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
