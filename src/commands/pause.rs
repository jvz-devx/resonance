use serenity::all::{CommandInteraction, Context};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};

use crate::state;
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;

    let state_lock = state::get_or_load_guild_state(ctx, guild_id).await?;
    let gs = state_lock.lock().await;

    if let Some(ref handle) = gs.current_track_handle {
        let is_paused = handle
            .get_info()
            .await
            .map(|info| info.playing == songbird::tracks::PlayMode::Pause)
            .unwrap_or(false);

        if is_paused {
            let embed = embeds::error_embed("Already paused.");
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .embed(embed)
                    .ephemeral(true),
            );
            command.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        let _ = handle.pause();
        let embed = embeds::success_embed("Paused", "Playback paused.");
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
