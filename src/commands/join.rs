use serenity::all::{CommandInteraction, Context};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};
use tracing::{error, info};

use crate::state;
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;
    info!("Join command in guild {guild_id} by {}", command.user.name);

    // Find the user's voice channel
    let guild = guild_id
        .to_guild_cached(&ctx.cache)
        .ok_or(BotError::NotInGuild)?
        .clone();

    let channel_id = guild
        .voice_states
        .get(&command.user.id)
        .and_then(|vs| vs.channel_id)
        .ok_or(BotError::NotInVoice)?;

    info!("User is in voice channel {channel_id}, attempting to join...");

    let manager = state::get_songbird(ctx).await?;

    match manager.join(guild_id, channel_id).await {
        Ok(_) => {
            info!("Successfully joined voice channel {channel_id} in guild {guild_id}");
        }
        Err(e) => {
            error!("Failed to join voice channel {channel_id}: {e:?}");
            return Err(BotError::JoinError(format!("{e:?}")));
        }
    }

    let embed = embeds::success_embed(
        "Joined Voice Channel",
        &format!("Connected to <#{}>", channel_id),
    );

    let response =
        CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().embed(embed));
    command.create_response(&ctx.http, response).await?;

    Ok(())
}
