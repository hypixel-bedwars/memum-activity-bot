use chrono::{Duration, Utc};
use poise::serenity_prelude::{self as serenity, CreateEmbed};
use tracing::info;

use crate::commands::logger::logger::{LogType, logger};
use crate::database::queries;
use crate::shared::types::{Context, Error};

async fn autocomplete_event_name<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> Vec<serenity::AutocompleteChoice> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id.get() as i64,
        None => return Vec::new(),
    };

    let events = queries::list_events_by_status(&ctx.data().db, guild_id, "active")
        .await
        .unwrap_or_default();

    let partial_lower = partial.to_lowercase();

    events
        .iter()
        .filter(|e| e.name.to_lowercase().contains(&partial_lower))
        .take(25)
        .map(|e| serenity::AutocompleteChoice::new(e.name.clone(), e.id.to_string()))
        .collect()
}

/// Disqualify a user globally (temporary event ban) or for a specific event.
///
/// Usage:
/// - Global ban:  /admin disqualify user:<user> duration:<days> reason:<optional>
/// - Event DQ:    /admin disqualify user:<user> event:<event_id> reason:<optional>
///
/// Exactly one of `duration` or `event` must be provided.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn disqualify(
    ctx: Context<'_>,
    #[description = "User to disqualify"] user: serenity::User,
    #[description = "Global ban duration in days (mutually exclusive with event)"] duration: Option<
        i64,
    >,
    #[autocomplete = "autocomplete_event_name"]
    #[description = "Event ID to disqualify the user from (mutually exclusive with duration)"]
    event: Option<i64>,
    #[description = "Optional reason"] reason: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command must be run in a server")?;
    let guild_i64 = guild_id.get() as i64;
    let pool = &ctx.data().db;

    // Validate mutually exclusive inputs
    match (duration, event) {
        (Some(_), Some(_)) => {
            ctx.say("Provide either `duration` (global ban) or `event`, not both.")
                .await?;
            return Ok(());
        }
        (None, None) => {
            ctx.say(
                "You must provide either a `duration` (global ban) or an `event` to disqualify.",
            )
            .await?;
            return Ok(());
        }
        _ => {}
    }

    // Load user (even if inactive) to allow banning/disqualifying
    let db_user = queries::get_user_by_discord_id_any(pool, user.id.get() as i64, guild_i64)
        .await?
        .ok_or("User is not registered in this guild")?;

    if let Some(days) = duration {
        if days <= 0 {
            ctx.say("Duration must be a positive number of days.")
                .await?;
            return Ok(());
        }

        let ban_until = Utc::now() + Duration::days(days);
        queries::ban_user_from_events(pool, db_user.id, days, reason.as_deref()).await?;

        let embed = CreateEmbed::new()
            .title("Global Event Ban Applied")
            .description({
                let duration_suffix = if days == 1 {
                    " (1 day)".to_string()
                } else {
                    format!(" ({} days)", days)
                };
                format!(
                    "User <@{}> is banned from **all events** until <t:{}:F>{}.\nReason: {}",
                    user.id,
                    ban_until.timestamp(),
                    duration_suffix,
                    reason.as_deref().unwrap_or("No reason provided.")
                )
            })
            .color(0xFF0000);

        ctx.send(poise::CreateReply::default().embed(embed)).await?;

        info!(
            admin = %ctx.author().name,
            target = %user.id,
            duration_days = days,
            reason = reason.as_deref().unwrap_or(""),
            "Applied global event ban"
        );

        logger(
            ctx.serenity_context(),
            ctx.data(),
            guild_id,
            LogType::Warn,
            format!(
                "{} applied global event ban to <@{}> for {} day(s). Reason: {}",
                ctx.author().name,
                user.id,
                days,
                reason.as_deref().unwrap_or("No reason provided.")
            ),
        )
        .await?;
    } else if let Some(event_id) = event {
        // Ensure event exists and belongs to this guild
        let event_row = queries::get_event_by_id(pool, event_id)
            .await?
            .ok_or("Event not found")?;
        if event_row.guild_id != guild_i64 {
            ctx.say("That event does not belong to this guild.").await?;
            return Ok(());
        }

        // Upsert participant row and mark disqualified
        queries::disqualify_user_from_event(pool, event_id, db_user.id).await?;

        let embed = CreateEmbed::new()
            .title("Event Disqualification Applied")
            .description(format!(
                "User <@{}> has been disqualified from event **{}** (ID: {}).\nReason: {}",
                user.id,
                event_row.name,
                event_row.id,
                reason.as_deref().unwrap_or("No reason provided.")
            ))
            .color(0xFFA500);

        ctx.send(poise::CreateReply::default().embed(embed)).await?;

        info!(
            admin = %ctx.author().name,
            target = %user.id,
            event_id = event_row.id,
            reason = reason.as_deref().unwrap_or(""),
            "Applied event-specific disqualification"
        );

        logger(
            ctx.serenity_context(),
            ctx.data(),
            guild_id,
            LogType::Warn,
            format!(
                "{} disqualified <@{}> from event '{}' (ID: {}). Reason: {}",
                ctx.author().name,
                user.id,
                event_row.name,
                event_row.id,
                reason.as_deref().unwrap_or("No reason provided.")
            ),
        )
        .await?;
    }

    Ok(())
}
