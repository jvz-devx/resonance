use serenity::all::{CommandInteraction, Context};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};

use crate::state::{self, get_or_create_guild_state};
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;

    let manager = state::get_songbird(ctx).await?;

    if manager.get(guild_id).is_none() {
        let embed = embeds::error_embed("I'm not in a voice channel.");
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    manager
        .remove(guild_id)
        .await
        .map_err(|e| BotError::JoinError(e.to_string()))?;

    // Clear guild state
    {
        let guild_states = state::get_guild_states(ctx).await?;
        let state_lock = get_or_create_guild_state(&guild_states, guild_id);
        let mut gs = state_lock.lock().await;
        gs.queue.clear();
        gs.now_playing = None;
        gs.current_track_handle = None;
    }

    let embed = embeds::success_embed("Disconnected", "Left the voice channel.");
    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new().embed(embed),
    );
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
