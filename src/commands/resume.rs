use serenity::all::{CommandInteraction, Context};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};

use crate::state::{self, get_or_create_guild_state};
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;

    let guild_states = state::get_guild_states(ctx).await?;
    let state_lock = get_or_create_guild_state(&guild_states, guild_id);
    let gs = state_lock.lock().await;

    if let Some(ref handle) = gs.current_track_handle {
        let _ = handle.play();
        let embed = embeds::success_embed("Resumed", "Playback resumed.");
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new().embed(embed),
        );
        command.create_response(&ctx.http, response).await?;
    } else {
        let embed = embeds::error_embed("Nothing is currently playing.");
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
    }

    Ok(())
}
