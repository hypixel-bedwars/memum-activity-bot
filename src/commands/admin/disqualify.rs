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

#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "BAN_MEMBERS",
    default_member_permissions = "BAN_MEMBERS"
)]
pub async fn disqualify(
    ctx: Context<'_>,
    user: serenity::User,
    duration: Option<i64>,
    event: Option<i64>,
    reason: Option<String>,
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
            ctx.say("You must provide either a `duration` or an `event`.")
                .await?;
            return Ok(());
        }
        _ => {}
    }

    // Load user
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
            .description(format!(
                "User <@{}> is banned from **all events** until <t:{}:F>.\nReason: {}",
                user.id,
                ban_until.timestamp(),
                reason.as_deref().unwrap_or("No reason provided.")
            ))
            .color(0xFF0000);

        ctx.send(poise::CreateReply::default().embed(embed)).await?;

        info!(
            admin = %ctx.author().name,
            target = %user.id,
            duration_days = days,
            "Applied global event ban"
        );

        logger(
            ctx.serenity_context(),
            ctx.data(),
            guild_id,
            LogType::Warn,
            format!(
                "{} globally banned <@{}> for {} day(s)",
                ctx.author().name,
                user.id,
                days
            ),
        )
        .await?;
    } else if let Some(event_id) = event {
        let event_row = queries::get_event_by_id(pool, event_id)
            .await?
            .ok_or("Event not found")?;

        if event_row.guild_id != guild_i64 {
            ctx.say("That event does not belong to this guild.").await?;
            return Ok(());
        }

        let already = queries::is_user_disqualified_from_event(pool, event_id, db_user.id).await?;

        if already {
            ctx.say("User is already disqualified from this event.")
                .await?;
            return Ok(());
        }

        queries::disqualify_user_from_event(pool, event_id, db_user.id).await?;

        let embed = CreateEmbed::new()
            .title("Event Disqualification Applied")
            .description(format!(
                "User <@{}> has been disqualified from **{}**.\nReason: {}",
                user.id,
                event_row.name,
                reason.as_deref().unwrap_or("No reason provided.")
            ))
            .color(0xFFA500);

        ctx.send(poise::CreateReply::default().embed(embed)).await?;

        info!(
            admin = %ctx.author().name,
            target = %user.id,
            event_id = event_row.id,
            "Applied event DQ"
        );

        logger(
            ctx.serenity_context(),
            ctx.data(),
            guild_id,
            LogType::Warn,
            format!(
                "{} disqualified <@{}> from event {}",
                ctx.author().name,
                user.id,
                event_row.name
            ),
        )
        .await?;
    }

    Ok(())
}

#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "BAN_MEMBERS",
    default_member_permissions = "BAN_MEMBERS"
)]
pub async fn undisqualify(
    ctx: Context<'_>,
    #[description = "User to re-qualify"] user: serenity::User,
    #[autocomplete = "autocomplete_event_name"]
    #[description = "Event ID (leave empty to remove global ban)"]
    event: Option<i64>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("Must be in a server")?;
    let guild_i64 = guild_id.get() as i64;
    let pool = &ctx.data().db;

    let db_user = queries::get_user_by_discord_id_any(pool, user.id.get() as i64, guild_i64)
        .await?
        .ok_or("User not found")?;

    if let Some(event_id) = event {
        queries::requalify_user_for_event(pool, event_id, db_user.id).await?;

        let embed = CreateEmbed::new()
            .title("Event Requalification")
            .description(format!(
                "<@{}> is no longer disqualified from event {}.",
                user.id, event_id
            ))
            .color(0x00FF00);

        ctx.send(poise::CreateReply::default().embed(embed)).await?;
    } else {
        queries::remove_global_event_ban(pool, db_user.id).await?;

        let embed = CreateEmbed::new()
            .title("Global Ban Removed")
            .description(format!(
                "<@{}> can now participate in events again.",
                user.id
            ))
            .color(0x00FF00);

        ctx.send(poise::CreateReply::default().embed(embed)).await?;
    }

    logger(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        LogType::Info,
        format!("{} undisqualified <@{}>", ctx.author().name, user.id),
    )
    .await?;

    Ok(())
}
