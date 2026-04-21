use serenity::all::{CommandInteraction, Context};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};

use crate::state;
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;

    let state_lock = state::get_or_load_guild_state(ctx, guild_id).await?;
    let gs = state_lock.lock().await;

    let tracks: Vec<_> = gs.queue.tracks.iter().cloned().collect();
    let embed = embeds::queue_embed(gs.now_playing.as_ref(), &tracks, &gs.loop_mode);

    let response =
        CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed));
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
