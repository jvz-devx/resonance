use serenity::all::{CommandInteraction, Context, ResolvedValue};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};

use crate::state::{self, LoopMode};
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;

    let mode_str = command
        .data
        .options()
        .into_iter()
        .find(|o| o.name == "mode")
        .and_then(|o| match o.value {
            ResolvedValue::String(s) => Some(s.to_string()),
            _ => None,
        })
        .ok_or(BotError::Other("Missing mode argument".into()))?;

    let loop_mode = LoopMode::from_str(&mode_str).ok_or(BotError::Other(format!(
        "Invalid loop mode: {mode_str}. Use off, track, or queue."
    )))?;

    let state_lock = state::get_or_load_guild_state(ctx, guild_id).await?;
    let redis_pool = state::get_redis_pool(ctx).await;
    let mut gs = state_lock.lock().await;

    gs.loop_mode = loop_mode.clone();

    // Persist
    if let Some(ref pool) = redis_pool {
        let _ = crate::state::redis::save_loop_mode(pool, guild_id.get(), &loop_mode).await;
    }

    let description = match loop_mode {
        LoopMode::Off => "Loop mode disabled.",
        LoopMode::Track => "Now looping the current track.",
        LoopMode::Queue => "Now looping the entire queue.",
    };

    let embed = embeds::success_embed("Loop Mode", description);
    let response =
        CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed));
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
