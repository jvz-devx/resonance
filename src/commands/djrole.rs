use std::time::Duration;

use serenity::all::{CommandInteraction, Context, ResolvedValue};
use serenity::builder::{CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage};
use serenity::model::id::{GuildId, RoleId};
use tracing::warn;

use crate::state;
use crate::utils::embeds;
use crate::utils::error::{BotError, BotResult};

/// Commands that change playback for everyone in the voice channel
const DJ_COMMANDS: &[&str] = &[
    "play", "skip", "pause", "resume", "stop", "shuffle", "loop", "remove", "clear", "leave",
];

const TEMPORARY_NOTE: &str =
    "\n\nRedis is disabled, so this setting resets when the bot goes idle or restarts.";
const REDIS_TIMEOUT: Duration = Duration::from_millis(750);

#[derive(Debug, PartialEq, Eq)]
enum Access {
    Allowed,
    Denied(RoleId),
}

fn check_access(dj_role: Option<RoleId>, member_roles: &[RoleId]) -> Access {
    match dj_role {
        None => Access::Allowed,
        Some(role) if member_roles.contains(&role) => Access::Allowed,
        Some(role) => Access::Denied(role),
    }
}

/// Server managers bypass the DJ check for controls other than /play.
/// Uses the permissions Discord sends with the interaction, so no privileged intent is needed.
fn can_bypass_dj_role(command: &CommandInteraction) -> bool {
    command.data.name != "play"
        && command
            .member
            .as_ref()
            .and_then(|member| member.permissions)
            .is_some_and(|permissions| permissions.administrator() || permissions.manage_guild())
}

pub async fn authorize(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    if !DJ_COMMANDS.contains(&command.data.name.as_str()) {
        return Ok(());
    }

    // Outside a guild the command itself reports the error
    let Some(guild_id) = command.guild_id else {
        return Ok(());
    };

    if can_bypass_dj_role(command) {
        return Ok(());
    }

    let member_roles = command
        .member
        .as_ref()
        .map(|member| member.roles.as_slice())
        .unwrap_or_default();

    match check_access(load(ctx, guild_id).await?, member_roles) {
        Access::Allowed => Ok(()),
        Access::Denied(role) => Err(BotError::DjOnly(role)),
    }
}

pub async fn run(ctx: &Context, command: &CommandInteraction) -> BotResult<()> {
    let guild_id = command.guild_id.ok_or(BotError::NotInGuild)?;

    let options = command.data.options();
    let subcommand = options
        .first()
        .ok_or(BotError::Other("Missing subcommand".into()))?;

    if matches!(subcommand.name, "set" | "clear") {
        let data = ctx.data.read().await;
        if data.get::<state::DjRoleManager>() != Some(&command.user.id) {
            return Err(BotError::DjRoleManagerOnly);
        }
    }

    match (subcommand.name, &subcommand.value) {
        ("set", ResolvedValue::SubCommand(args)) => {
            let role = args
                .iter()
                .find_map(|option| match option.value {
                    ResolvedValue::Role(role) if option.name == "role" => Some(role),
                    _ => None,
                })
                .ok_or(BotError::Other("Missing role argument".into()))?;

            // The @everyone role shares the guild's ID and is never listed on a member
            if role.id.get() == guild_id.get() {
                let embed = embeds::error_embed(
                    "@everyone can't be the DJ role. Use `/djrole clear` to let everyone control playback.",
                );
                return respond(ctx, command, embed).await;
            }

            let note = store(ctx, guild_id, Some(role.id)).await?;
            let embed = embeds::success_embed(
                "DJ Role",
                &format!(
                    "Only members with <@&{0}> can use `/play`. Members with <@&{0}> and server managers can use the other playback controls.{note}",
                    role.id
                ),
            );
            respond(ctx, command, embed).await
        }
        ("clear", _) => {
            let note = store(ctx, guild_id, None).await?;
            let embed = embeds::success_embed(
                "DJ Role",
                &format!("Everyone can use the playback controls again.{note}"),
            );
            respond(ctx, command, embed).await
        }
        ("show", _) => {
            let embed = match load(ctx, guild_id).await? {
                Some(role) => embeds::success_embed(
                    "DJ Role",
                    &format!("The playback controls are limited to <@&{role}>."),
                ),
                None => embeds::success_embed(
                    "DJ Role",
                    "No DJ role is set. Everyone can use the playback controls.",
                ),
            };
            respond(ctx, command, embed).await
        }
        _ => Err(BotError::Other("Unknown subcommand".into())),
    }
}

async fn load(ctx: &Context, guild_id: GuildId) -> BotResult<Option<RoleId>> {
    if let Some(pool) = state::get_redis_pool(ctx).await {
        match tokio::time::timeout(
            REDIS_TIMEOUT,
            state::redis::load_dj_role(&pool, guild_id.get()),
        )
        .await
        {
            Ok(Ok(role)) => return Ok(role),
            failure => warn!(?failure, "Failed to read the DJ role for guild {guild_id}"),
        }
        return Err(BotError::DjRoleUnavailable);
    }

    let guild_state = state::get_or_load_guild_state(ctx, guild_id).await?;
    Ok(guild_state.lock().await.dj_role)
}

async fn store(ctx: &Context, guild_id: GuildId, role: Option<RoleId>) -> BotResult<&'static str> {
    if let Some(pool) = state::get_redis_pool(ctx).await {
        match tokio::time::timeout(
            REDIS_TIMEOUT,
            state::redis::save_dj_role(&pool, guild_id.get(), role),
        )
        .await
        {
            Ok(Ok(())) => return Ok(""),
            failure => warn!(
                ?failure,
                "Could not confirm the DJ role update for guild {guild_id}"
            ),
        }
        return Err(BotError::DjRoleUpdateUnconfirmed);
    }

    let guild_state = state::get_or_load_guild_state(ctx, guild_id).await?;
    guild_state.lock().await.dj_role = role;
    Ok(TEMPORARY_NOTE)
}

async fn respond(ctx: &Context, command: &CommandInteraction, embed: CreateEmbed) -> BotResult<()> {
    let response = CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .embed(embed)
            .ephemeral(true),
    );
    command.create_response(&ctx.http, response).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DJ: RoleId = RoleId::new(100);
    const OTHER: RoleId = RoleId::new(200);

    #[test]
    fn everyone_is_allowed_without_a_dj_role() {
        assert_eq!(check_access(None, &[]), Access::Allowed);
        assert_eq!(check_access(None, &[OTHER]), Access::Allowed);
    }

    #[test]
    fn members_with_the_dj_role_are_allowed() {
        assert_eq!(check_access(Some(DJ), &[OTHER, DJ]), Access::Allowed);
    }

    #[test]
    fn members_without_the_dj_role_are_denied() {
        assert_eq!(check_access(Some(DJ), &[]), Access::Denied(DJ));
        assert_eq!(check_access(Some(DJ), &[OTHER]), Access::Denied(DJ));
    }

    #[test]
    fn only_control_commands_are_restricted() {
        for name in [
            "search",
            "queue",
            "nowplaying",
            "join",
            "normalize",
            "djrole",
        ] {
            assert!(!DJ_COMMANDS.contains(&name), "{name} should stay open");
        }
        for name in [
            "play", "skip", "stop", "clear", "remove", "shuffle", "loop", "pause", "resume",
            "leave",
        ] {
            assert!(
                DJ_COMMANDS.contains(&name),
                "{name} should need the DJ role"
            );
        }
    }
}
