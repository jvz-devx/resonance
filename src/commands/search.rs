use std::time::{Duration, Instant};

use serenity::all::{CommandInteraction, Context, ResolvedValue};
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};
use serenity::model::channel::ReactionType;

use crate::state::{self, PendingSearch};
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};
use crate::youtube::search::search_youtube;

pub const SEARCH_TIMEOUT_SECS: u64 = 30;
pub const REACTION_1: &str = "\u{31}\u{fe0f}\u{20e3}"; // 1️⃣
pub const REACTION_2: &str = "\u{32}\u{fe0f}\u{20e3}"; // 2️⃣
pub const REACTION_3: &str = "\u{33}\u{fe0f}\u{20e3}"; // 3️⃣

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;

    let query = command
        .data
        .options()
        .into_iter()
        .find(|o| o.name == "query")
        .and_then(|o| match o.value {
            ResolvedValue::String(s) => Some(s.to_string()),
            _ => None,
        })
        .ok_or(BotError::Other("Missing query argument".into()))?;

    // Defer the response
    let defer = CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new());
    command.create_response(&ctx.http, defer).await?;

    // Search YouTube
    let results = search_youtube(&query, 3).await?;

    // Build and send embed
    let embed = embeds::search_results_embed(&results);
    let response = serenity::builder::EditInteractionResponse::new().embed(embed);
    let message = command.edit_response(&ctx.http, response).await?;

    // Add reaction emojis
    let reactions = [REACTION_1, REACTION_2, REACTION_3];
    for (i, emoji) in reactions.iter().enumerate() {
        if i < results.len() {
            let reaction = ReactionType::Unicode(emoji.to_string());
            let _ = message.react(&ctx.http, reaction).await;
        }
    }

    // Store pending search for reaction handler
    let pending = state::get_pending_searches(ctx).await?;
    pending.insert(
        message.id,
        PendingSearch {
            user_id: command.user.id,
            guild_id,
            channel_id: command.channel_id,
            results,
            expires_at: Instant::now() + Duration::from_secs(SEARCH_TIMEOUT_SECS),
        },
    );

    Ok(())
}

/// Map a reaction emoji to a result index (0-based)
pub fn emoji_to_index(emoji: &str) -> Option<usize> {
    match emoji {
        s if s == REACTION_1 => Some(0),
        s if s == REACTION_2 => Some(1),
        s if s == REACTION_3 => Some(2),
        _ => None,
    }
}
