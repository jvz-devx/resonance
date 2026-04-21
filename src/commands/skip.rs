use serenity::all::{CommandInteraction, Context};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};

use crate::state;
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;

    let state_lock = state::get_or_load_guild_state(ctx, guild_id).await?;
    let gs = state_lock.lock().await;

    if gs.now_playing.is_none() {
        let embed = embeds::error_embed("Nothing is currently playing.");
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .ephemeral(true),
        );
        command.create_response(&ctx.http, response).await?;
        return Ok(());
    }

    let skipped_title = gs
        .now_playing
        .as_ref()
        .map(|t| t.title.clone())
        .unwrap_or_else(|| "Unknown".into());

    // Stop the current track — the TrackEndHandler will auto-play the next one
    if let Some(ref handle) = gs.current_track_handle {
        let _ = handle.stop();
    }

    let embed = embeds::success_embed("Skipped", &format!("Skipped **{skipped_title}**"));
    let response =
        CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed));
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
