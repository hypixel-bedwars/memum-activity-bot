/// `/edit-event` command group — admin only.
///
/// Provides subcommands for managing guild events:
/// - `new`                 — create a new event
/// - `edit`                — edit event details
/// - `delete`              — delete an event
/// - `start`               — force start an event
/// - `end`                 — force end an active event
/// - `participants`        — list event participants with pagination
/// - `leaderboard persist` — send a persistent leaderboard message
/// - `status create`       — create a persistent status message
/// - `status remove`       — remove the persistent status message
/// - `stats-add`           — add a stat to an event
/// - `stats-remove`        — remove a stat from an event
/// - `stats-edit`          — edit XP per unit for a stat
/// - `list`                     — list all events
/// - `backfill`                 — backfill XP for an event
/// - `milestones-add`           — add XP-threshold milestones to an event
/// - `milestones-remove`        — remove milestones from an event
/// - `milestones-completers`    — participant-centric view of milestone completers
///
/// All subcommands are ephemeral and require the admin check.
use poise::serenity_prelude::{
    self as serenity, CreateAttachment, CreateEmbed, CreateEmbedFooter, CreateMessage,
};
use sqlx::PgPool;
use tracing::{error, info};

use crate::commands::leaderboard::helpers as lb_helpers;
use crate::commands::logger::logger::{LogType, logger, logger_system};
use crate::config::GuildConfig;
use crate::database::models::DbEvent;
use crate::database::queries;
use crate::shared::types::{Context, Error};
use crate::utils::stats_definitions::display_name_for_key;

async fn autocomplete_event_name<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> Vec<serenity::AutocompleteChoice> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id.get() as i64,
        None => return Vec::new(),
    };

    let events = queries::list_events(&ctx.data().db, guild_id)
        .await
        .unwrap_or_default();

    let partial_lower = partial.to_lowercase();

    events
        .iter()
        .filter(|e| e.name.to_lowercase().contains(&partial_lower))
        .take(25)
        .map(|e| {
            let label = format!("{} [{}]", e.name, e.status);
            serenity::AutocompleteChoice::new(label, e.name.clone())
        })
        .collect()
}

/// Autocomplete for event names — returns all events including ended.
async fn autocomplete_any_event_name<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> Vec<serenity::AutocompleteChoice> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id.get() as i64,
        None => return Vec::new(),
    };

    let events = queries::list_events(&ctx.data().db, guild_id)
        .await
        .unwrap_or_default();

    let partial_lower = partial.to_lowercase();

    events
        .iter()
        .filter(|e| e.name.to_lowercase().contains(&partial_lower))
        .take(25)
        .map(|e| {
            let label = format!("{} [{}]", e.name, e.status);
            serenity::AutocompleteChoice::new(label, e.name.clone())
        })
        .collect()
}

/// Autocomplete for stat names within an event.
async fn autocomplete_event_stat<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> Vec<serenity::AutocompleteChoice> {
    // We need the event_id, but autocomplete doesn't easily give us
    // the other parameter's value. Fall back to listing all known stat
    // definitions from the guild xp_config + discord + bedwars.
    let guild_id = match ctx.guild_id() {
        Some(id) => id.get() as i64,
        None => return Vec::new(),
    };

    let config: GuildConfig = match queries::get_guild(&ctx.data().db, guild_id).await {
        Ok(Some(row)) => serde_json::from_value(row.config_json).unwrap_or_default(),
        _ => GuildConfig::default(),
    };

    let partial_lower = partial.to_lowercase();

    let mut results: Vec<(String, String)> = config
        .xp_config
        .keys()
        .filter_map(|k| {
            let display = display_name_for_key(k);
            let matches = display.to_lowercase().contains(&partial_lower)
                || k.to_lowercase().contains(&partial_lower);
            if matches {
                Some((display, k.clone()))
            } else {
                None
            }
        })
        .collect();

    results.sort_by(|a, b| a.0.cmp(&b.0));
    results.truncate(25);

    results
        .into_iter()
        .map(|(display, raw_key)| serenity::AutocompleteChoice::new(display, raw_key))
        .collect()
}

/// Manage guild events. Admin only.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "edit-event",
    subcommands(
        "new",
        "edit",
        "delete",
        "start",
        "end",
        "participants",
        "edit_event_leaderboard",
        "stats_add",
        "stats_remove",
        "stats_edit",
        "list",
        "backfill",
        "status",
        "milestones_add",
        "milestones_remove",
        "milestones_completers",
        "leaderboard_remove"
    )
)]
pub async fn edit_event(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Create a new event. Stats are seeded from the guild's current XP config.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn new(
    ctx: Context<'_>,
    #[description = "Unique event name"] name: String,
    #[description = "Optional description"] description: Option<String>,
    #[description = "Start date/time (ISO 8601, e.g. 2026-04-01T00:00:00Z)"] start: String,
    #[description = "End date/time (ISO 8601, e.g. 2026-04-08T00:00:00Z)"] end: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let start_date: chrono::DateTime<chrono::Utc> = start
        .parse()
        .map_err(|_| "Invalid start date. Use ISO 8601 format, e.g. `2026-04-01T00:00:00Z`.")?;
    let end_date: chrono::DateTime<chrono::Utc> = end
        .parse()
        .map_err(|_| "Invalid end date. Use ISO 8601 format, e.g. `2026-04-08T00:00:00Z`.")?;

    if end_date <= start_date {
        ctx.say("End date must be after start date.").await?;
        return Ok(());
    }

    if queries::get_event_by_name(&data.db, guild_id, &name)
        .await?
        .is_some()
    {
        ctx.say(format!(
            "An event named **{name}** already exists. Choose a different name."
        ))
        .await?;
        return Ok(());
    }

    let event = queries::create_event(
        &data.db,
        guild_id,
        &name,
        description.as_deref(),
        &start_date,
        &end_date,
    )
    .await?;

    let guild_config: GuildConfig = match queries::get_guild(&data.db, guild_id).await? {
        Some(g) => serde_json::from_value(g.config_json).unwrap_or_default(),
        None => GuildConfig::default(),
    };

    queries::seed_event_stats_from_xp_config(&data.db, event.id, &guild_config.xp_config).await?;

    // Immediately run the global updater so events with a start_date <= NOW()
    // become active right away (instead of waiting until the next daily snapshot).
    if let Err(e) = queries::update_event_statuses(&data.db).await {
        error!(error = %e, "Failed to update event statuses after creating event.");
    }

    let stats_count = guild_config.xp_config.len();

    let needs_backfill = start_date < chrono::Utc::now();

    let backfill_notice = if needs_backfill {
        "\n\nBackfilling historical XP in the background..."
    } else {
        ""
    };

    ctx.say(format!(
        "Created event **{name}** (ID: {}).\n\
         Start: <t:{}:F>\n\
         End: <t:{}:F>\n\
         Seeded **{stats_count}** stats from guild XP config.{backfill_notice}",
        event.id,
        start_date.timestamp(),
        end_date.timestamp(),
    ))
    .await?;

    info!(
        "Event '{}' created by {} (guild {})",
        name,
        ctx.author().name,
        guild_id
    );

    logger(
        ctx.serenity_context(),
        data,
        ctx.guild_id().unwrap(),
        LogType::Info,
        format!(
            "{} created event **{}** ({} → {})",
            ctx.author().name,
            name,
            start_date.format("%Y-%m-%d"),
            end_date.format("%Y-%m-%d"),
        ),
    )
    .await?;

    if needs_backfill {
        let pool = data.db.clone();
        let http = data.http.clone();
        let base_level_xp = data.config.base_level_xp;
        let level_exponent = data.config.level_exponent;
        let event_id = event.id;
        let event_name = name.clone();

        tokio::spawn(async move {
            match queries::backfill_event_xp(&pool, event_id, base_level_xp, level_exponent).await {
                Ok(summary) => {
                    info!(
                        event_id,
                        deltas_processed = summary.deltas_processed,
                        total_xp = summary.total_xp_awarded,
                        users_affected = summary.users_affected,
                        "Background backfill completed for event '{}'.",
                        event_name
                    );
                    logger_system(
                        &http,
                        &pool,
                        guild_id,
                        LogType::Info,
                        format!(
                            "Backfill for **{}** complete — {} deltas, {:.0} XP, {} users affected.",
                            event_name,
                            summary.deltas_processed,
                            summary.total_xp_awarded,
                            summary.users_affected,
                        ),
                    )
                    .await;
                }
                Err(e) => {
                    error!(event_id, error = %e, "Background backfill failed for event '{}'.", event_name);
                    logger_system(
                        &http,
                        &pool,
                        guild_id,
                        LogType::Error,
                        format!("Backfill for **{}** failed: {}", event_name, e),
                    )
                    .await;
                }
            }
        });
    }

    Ok(())
}

/// Edit an existing event's name, description, or dates.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn edit(
    ctx: Context<'_>,
    #[description = "Event to edit"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
    #[description = "New name (leave blank to keep)"] new_name: Option<String>,
    #[description = "New description (leave blank to keep)"] new_description: Option<String>,
    #[description = "New start date (ISO 8601)"] new_start: Option<String>,
    #[description = "New end date (ISO 8601)"] new_end: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{event_name}** not found."))
                .await?;
            return Ok(());
        }
    };

    if event.status == "ended" {
        ctx.say("Cannot edit an event that has already ended.")
            .await?;
        return Ok(());
    }

    let start_dt: Option<chrono::DateTime<chrono::Utc>> = match new_start {
        Some(ref s) => Some(
            s.parse()
                .map_err(|_| "Invalid start date. Use ISO 8601 format.")?,
        ),
        None => None,
    };

    let end_dt: Option<chrono::DateTime<chrono::Utc>> = match new_end {
        Some(ref s) => Some(
            s.parse()
                .map_err(|_| "Invalid end date. Use ISO 8601 format.")?,
        ),
        None => None,
    };

    let updated = queries::update_event(
        &data.db,
        guild_id,
        event.id,
        new_name.as_deref(),
        new_description.as_deref(),
        start_dt.as_ref(),
        end_dt.as_ref(),
    )
    .await?;

    if updated {
        ctx.say(format!("Event **{event_name}** updated successfully."))
            .await?;

        logger(
            ctx.serenity_context(),
            data,
            ctx.guild_id().unwrap(),
            crate::commands::logger::logger::LogType::Warn,
            format!("{} edited event **{}**", ctx.author().name, event_name),
        )
        .await?;
    } else {
        ctx.say("No changes were applied (event may have ended or not found).")
            .await?;
    }

    Ok(())
}

/// Delete a pending or active event.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn delete(
    ctx: Context<'_>,
    #[description = "Event to delete"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{event_name}** not found."))
                .await?;
            return Ok(());
        }
    };

    if event.status == "ended" {
        ctx.say("Cannot delete an event that has already ended.")
            .await?;
        return Ok(());
    }

    let deleted = queries::delete_event(&data.db, guild_id, event.id).await?;

    if deleted {
        ctx.say(format!("Event **{event_name}** deleted.")).await?;

        info!(
            "Event '{}' deleted by {} (guild {})",
            event_name,
            ctx.author().name,
            guild_id
        );

        logger(
            ctx.serenity_context(),
            data,
            ctx.guild_id().unwrap(),
            crate::commands::logger::logger::LogType::Warn,
            format!("{} deleted event **{}**", ctx.author().name, event_name),
        )
        .await?;
    } else {
        ctx.say("Failed to delete event — it may have ended or been removed already.")
            .await?;
    }

    Ok(())
}

/// Add a tracked stat to an event.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "stats-add",
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn stats_add(
    ctx: Context<'_>,
    #[description = "Event to modify"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
    #[description = "Stat key to add"]
    #[autocomplete = "autocomplete_event_stat"]
    stat_name: String,
    #[description = "XP awarded per unit"] xp_per_unit: f64,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{event_name}** not found."))
                .await?;
            return Ok(());
        }
    };

    if event.status == "ended" {
        ctx.say("Cannot modify stats on an ended event.").await?;
        return Ok(());
    }

    if event.status == "active" {
        ctx.say("Cannot modify stats on an active event.").await?;
        return Ok(());
    }

    if xp_per_unit < 0.0 {
        ctx.say("XP per unit cannot be negative.").await?;
        return Ok(());
    }

    let added = queries::add_event_stat(&data.db, event.id, &stat_name, xp_per_unit).await?;

    if added {
        let display = display_name_for_key(&stat_name);
        ctx.say(format!(
            "Added **{display}** (`{stat_name}`) to event **{event_name}** → **{xp_per_unit} XP/unit**."
        ))
        .await?;
    } else {
        ctx.say(format!(
            "Stat `{stat_name}` is already configured for this event. Use `stats-edit` to change its value."
        ))
        .await?;
    }

    Ok(())
}

/// Remove a tracked stat from an event.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "stats-remove",
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn stats_remove(
    ctx: Context<'_>,
    #[description = "Event to modify"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
    #[description = "Stat key to remove"]
    #[autocomplete = "autocomplete_event_stat"]
    stat_name: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{event_name}** not found."))
                .await?;
            return Ok(());
        }
    };

    if event.status == "ended" {
        ctx.say("Cannot modify stats on an ended event.").await?;
        return Ok(());
    }

    if event.status == "active" {
        ctx.say("Cannot modify stats on an active event.").await?;
        return Ok(());
    }

    let removed = queries::remove_event_stat(&data.db, event.id, &stat_name).await?;

    if removed {
        let display = display_name_for_key(&stat_name);
        ctx.say(format!(
            "Removed **{display}** (`{stat_name}`) from event **{event_name}**."
        ))
        .await?;
    } else {
        ctx.say(format!(
            "Stat `{stat_name}` is not configured for this event."
        ))
        .await?;
    }

    Ok(())
}

/// Change the XP-per-unit for a stat in an event.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "stats-edit",
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn stats_edit(
    ctx: Context<'_>,
    #[description = "Event to modify"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
    #[description = "Stat key to edit"]
    #[autocomplete = "autocomplete_event_stat"]
    stat_name: String,
    #[description = "New XP per unit"] xp_per_unit: f64,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{event_name}** not found."))
                .await?;
            return Ok(());
        }
    };

    if event.status == "ended" {
        ctx.say("Cannot modify stats on an ended event.").await?;
        return Ok(());
    }

    if event.status == "active" {
        ctx.say("Cannot modify stats on an active event.").await?;
        return Ok(());
    }

    if xp_per_unit < 0.0 {
        ctx.say("XP per unit cannot be negative.").await?;
        return Ok(());
    }

    let updated = queries::edit_event_stat(&data.db, event.id, &stat_name, xp_per_unit).await?;

    if updated {
        let display = display_name_for_key(&stat_name);
        ctx.say(format!(
            "Updated **{display}** (`{stat_name}`) → **{xp_per_unit} XP/unit** on event **{event_name}**."
        ))
        .await?;
    } else {
        ctx.say(format!(
            "Stat `{stat_name}` is not configured for this event. Use `stats-add` first."
        ))
        .await?;
    }

    Ok(())
}

/// List all events and their status.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let events = queries::list_events(&data.db, guild_id).await?;

    if events.is_empty() {
        ctx.say("No events have been created yet. Use `/edit-events new` to create one.")
            .await?;
        return Ok(());
    }

    let mut description = String::new();

    for event in &events {
        let stats = queries::get_event_stats(&data.db, event.id).await?;
        let stats_summary = if stats.is_empty() {
            "no stats".to_string()
        } else {
            format!("{} stats", stats.len())
        };

        let status_emoji = match event.status.as_str() {
            "pending" => ":clock3:",
            "active" => ":green_circle:",
            "ended" => ":red_circle:",
            _ => ":grey_question:",
        };

        description.push_str(&format!(
            "{} **{}** — {} ({}) | <t:{}:d> → <t:{}:d>\n",
            status_emoji,
            event.name,
            event.status,
            stats_summary,
            event.start_date.timestamp(),
            event.end_date.timestamp(),
        ));
    }

    let embed = CreateEmbed::default()
        .title(format!("Events — {} total", events.len()))
        .description(description)
        .color(0x00BFFF);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Manually trigger a retroactive XP backfill for an event.
///
/// Safe to re-run multiple times — already-processed deltas are skipped via
/// `ON CONFLICT DO NOTHING`. The command waits for the backfill to finish
/// before replying with a summary.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn backfill(
    ctx: Context<'_>,
    #[description = "Event to backfill"]
    #[autocomplete = "autocomplete_any_event_name"]
    event_name: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("No event named **{event_name}** was found."))
                .await?;
            return Ok(());
        }
    };

    // Quick count so the admin knows there's something to process.
    let delta_count = queries::count_deltas_for_event(&data.db, event.id).await?;
    if delta_count == 0 {
        ctx.say(format!(
            "No eligible stat deltas found for **{event_name}** in its time window. Nothing to backfill."
        ))
        .await?;
        return Ok(());
    }

    ctx.say(format!(
        "Backfilling **{event_name}** — found {delta_count} eligible deltas. This may take a moment..."
    ))
    .await?;

    let base_level_xp = data.config.base_level_xp;
    let level_exponent = data.config.level_exponent;

    match queries::backfill_event_xp(&data.db, event.id, base_level_xp, level_exponent).await {
        Ok(summary) => {
            info!(
                event_id = event.id,
                deltas_processed = summary.deltas_processed,
                total_xp = summary.total_xp_awarded,
                users_affected = summary.users_affected,
                "Manual backfill completed for event '{}'.",
                event_name
            );

            let embed = poise::serenity_prelude::CreateEmbed::default()
                .title(format!("Backfill Complete — {event_name}"))
                .description(format!(
                    "Deltas processed: **{}**\nXP awarded: **{:.0}**\nUsers affected: **{}**",
                    summary.deltas_processed, summary.total_xp_awarded, summary.users_affected,
                ))
                .color(0x2ECC71);

            ctx.send(poise::CreateReply::default().embed(embed)).await?;

            logger(
                ctx.serenity_context(),
                data,
                ctx.guild_id().unwrap(),
                LogType::Info,
                format!(
                    "{} ran backfill for **{}** — {} deltas, {:.0} XP, {} users.",
                    ctx.author().name,
                    event_name,
                    summary.deltas_processed,
                    summary.total_xp_awarded,
                    summary.users_affected,
                ),
            )
            .await?;
        }
        Err(e) => {
            error!(event_id = event.id, error = %e, "Manual backfill failed for event '{}'.", event_name);
            ctx.say(format!("Backfill failed: {e}")).await?;
        }
    }

    Ok(())
}

/// Autocomplete for event names that currently have a persistent leaderboard.
async fn autocomplete_persisted_event_name<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> Vec<serenity::AutocompleteChoice> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id.get() as i64,
        None => return Vec::new(),
    };

    // Fetch all persistent event leaderboard records for this guild.
    let records = match queries::get_all_persistent_event_leaderboards(&ctx.data().db).await {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let partial_lower = partial.to_lowercase();
    let mut choices = Vec::new();

    for record in records.iter().filter(|r| r.guild_id == guild_id) {
        let event = match queries::get_event_by_id(&ctx.data().db, record.event_id).await {
            Ok(Some(e)) => e,
            _ => continue,
        };

        if event.name.to_lowercase().contains(&partial_lower) {
            let label = format!(
                "{} [{}] — <#{}>",
                event.name, event.status, record.channel_id
            );
            choices.push(serenity::AutocompleteChoice::new(label, event.name));
        }

        if choices.len() >= 25 {
            break;
        }
    }

    choices
}

/// Remove a persistent event leaderboard and delete its Discord messages.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "leaderboard-remove",
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn leaderboard_remove(
    ctx: Context<'_>,
    #[description = "Event whose persistent leaderboard should be removed"]
    #[autocomplete = "autocomplete_persisted_event_name"]
    event_name: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{event_name}** not found."))
                .await?;
            return Ok(());
        }
    };

    let existing = match queries::get_persistent_event_leaderboard(&data.db, event.id).await? {
        Some(r) => r,
        None => {
            ctx.say(format!(
                "No persistent leaderboard exists for **{event_name}**."
            ))
            .await?;
            return Ok(());
        }
    };

    let channel = serenity::ChannelId::new(existing.channel_id as u64);

    // Delete leaderboard page messages.
    let msg_ids: Vec<u64> =
        serde_json::from_value(existing.message_ids.clone()).unwrap_or_default();
    for msg_id in msg_ids {
        let _ = channel
            .delete_message(&ctx.http(), serenity::MessageId::new(msg_id))
            .await;
    }

    // Delete status message.
    if existing.status_message_id != 0 {
        let _ = channel
            .delete_message(
                &ctx.http(),
                serenity::MessageId::new(existing.status_message_id as u64),
            )
            .await;
    }

    // Delete milestone card message.
    if existing.milestone_message_id != 0 {
        let _ = channel
            .delete_message(
                &ctx.http(),
                serenity::MessageId::new(existing.milestone_message_id as u64),
            )
            .await;
    }

    // Remove database record.
    queries::delete_persistent_event_leaderboard(&data.db, event.id).await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            CreateEmbed::new()
                .title("Persistent Event Leaderboard Removed")
                .color(0xFF4444)
                .field("Event", &event.name, true)
                .field("Channel", format!("<#{}>", existing.channel_id), true),
        ),
    )
    .await?;

    info!(
        event_id = event.id,
        guild_id,
        channel_id = existing.channel_id,
        "Removed persistent event leaderboard."
    );

    logger(
        ctx.serenity_context(),
        data,
        ctx.guild_id().unwrap(),
        LogType::Info,
        format!(
            "{} removed the persistent event leaderboard for **{}** from <#{}>",
            ctx.author().name,
            event.name,
            existing.channel_id,
        ),
    )
    .await?;

    Ok(())
}

#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn start(
    ctx: Context<'_>,
    #[description = "Force starts the selected event"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get() as i64;
    let data = ctx.data();

    let event = queries::get_event_by_name(&data.db, guild_id, &event_name).await?;

    if let Some(event) = event {
        queries::force_start_event(&data.db, event.id).await?;
        ctx.say(format!("Event **{}** has been force started.", event_name))
            .await?;
    } else {
        ctx.say("Event not found.").await?;
    }

    Ok(())
}

#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn end(
    ctx: Context<'_>,
    #[description = "Force ends the selected event"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get() as i64;
    let data = ctx.data();

    let event = queries::get_event_by_name(&data.db, guild_id, &event_name).await?;

    if let Some(event) = event {
        queries::force_end_event(&data.db, event.id).await?;
        ctx.say(format!("Event **{}** has been force ended.", event_name))
            .await?;
    } else {
        ctx.say("Event not found.").await?;
    }

    Ok(())
}

#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn participants(
    ctx: Context<'_>,
    #[description = "Lists all the participants of the selected event."]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get() as i64;
    let data = ctx.data();

    let event = queries::get_event_by_name(&data.db, guild_id, &event_name).await?;

    let Some(event) = event else {
        ctx.say("Event not found.").await?;
        return Ok(());
    };

    let participants = queries::get_event_participants(&data.db, event.id).await?;

    if participants.is_empty() {
        ctx.say("No participants yet.").await?;
        return Ok(());
    }

    let list = participants
        .iter()
        .take(10)
        .map(|p| format!("<@{}>", p.user_id))
        .collect::<Vec<_>>()
        .join("\n");

    let embed = CreateEmbed::default()
        .title(format!("Participants — {}", event_name))
        .description(list)
        .color(0x00BFFF);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

#[poise::command(slash_command, subcommands("persist"), rename = "leaderboard")]
pub async fn edit_event_leaderboard(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn persist(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_any_event_name"] event_name: Option<String>,
    #[description = "Channel to post the persistent leaderboard in"] channel: serenity::ChannelId,
    #[description = "Number of players to display (1-50, default 20)"]
    #[min = 1]
    #[max = 50]
    display_count: Option<i32>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server.")?;
    let guild_id_i64 = guild_id.get() as i64;

    // Validate and set display limit (default 20, max 50)
    let display_limit = display_count.unwrap_or(20).clamp(1, 50) as i64;

    // Resolve event — use provided name or fall back to the latest event.
    let resolved_name = match event_name {
        Some(n) => n,
        None => queries::get_latest_event_name(&ctx.data().db, guild_id_i64)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No events found in this server."))?,
    };

    let event =
        match queries::get_event_by_name(&ctx.data().db, guild_id_i64, &resolved_name).await? {
            Some(e) => e,
            None => {
                ctx.send(
                    poise::CreateReply::default()
                        .ephemeral(true)
                        .content(format!("Event **{resolved_name}** not found.")),
                )
                .await?;
                return Ok(());
            }
        };

    // If there's already a persistent leaderboard for this event, delete the
    // old messages before posting fresh ones.
    if let Some(existing) =
        queries::get_persistent_event_leaderboard(&ctx.data().db, event.id).await?
    {
        let old_msg_ids: Vec<u64> =
            serde_json::from_value(existing.message_ids.clone()).unwrap_or_default();
        let old_channel = serenity::ChannelId::new(existing.channel_id as u64);

        for msg_id in old_msg_ids {
            let _ = old_channel
                .delete_message(&ctx.http(), serenity::MessageId::new(msg_id))
                .await;
        }
        if existing.status_message_id != 0 {
            let _ = old_channel
                .delete_message(
                    &ctx.http(),
                    serenity::MessageId::new(existing.status_message_id as u64),
                )
                .await;
        }
        if existing.milestone_message_id != 0 {
            let _ = old_channel
                .delete_message(
                    &ctx.http(),
                    serenity::MessageId::new(existing.milestone_message_id as u64),
                )
                .await;
        }

        queries::delete_persistent_event_leaderboard(&ctx.data().db, event.id).await?;
    }

    // Determine how many pages to post.
    let total_participants = queries::count_event_participants(&ctx.data().db, event.id).await?;
    // Limit displayed participants to display_limit
    let displayed_count = total_participants.min(display_limit);
    // Always show at least one page (pending / empty state).
    let total_pages = ((displayed_count as f64) / lb_helpers::PAGE_SIZE as f64)
        .ceil()
        .max(1.0) as u32;

    let mut message_ids: Vec<u64> = Vec::new();

    for page in 1..=total_pages {
        let result = lb_helpers::generate_event_leaderboard_page(
            &ctx.data().db,
            event.id,
            &event.name,
            &event.status,
            event.start_date.timestamp(),
            page,
            Some(display_limit),
        )
        .await;

        let (png_bytes, _) = match result {
            Ok(v) => v,
            Err(e) => {
                ctx.send(
                    poise::CreateReply::default()
                        .ephemeral(true)
                        .content(format!("Failed to generate leaderboard page {page}: {e}")),
                )
                .await?;
                return Ok(());
            }
        };

        let attachment =
            CreateAttachment::bytes(png_bytes, format!("event_leaderboard_page_{page}.png"));
        let msg = channel
            .send_message(&ctx.http(), CreateMessage::new().add_file(attachment))
            .await?;

        message_ids.push(msg.id.get());
    }

    // Post milestone card if milestones exist for this event (before status message).
    let milestone_message_id: i64 = match lb_helpers::generate_event_milestone_card(
        &ctx.data().db,
        event.id,
        &event.name,
    )
    .await
    {
        Ok(Some(bytes)) => {
            let attachment = CreateAttachment::bytes(bytes, "event_milestones.png");
            match channel
                .send_message(&ctx.http(), CreateMessage::new().add_file(attachment))
                .await
            {
                Ok(msg) => msg.id.get() as i64,
                Err(e) => {
                    error!(event_id = event.id, error = %e, "Failed to post event milestone card.");
                    0
                }
            }
        }
        Ok(None) => 0,
        Err(e) => {
            error!(event_id = event.id, error = %e, "Failed to generate event milestone card.");
            0
        }
    };

    // Post status / last-updated message (always last, so it sits at the bottom).
    let unix_time = time::OffsetDateTime::now_utc().unix_timestamp();
    let status_content = if event.status == "ended" {
        format!(
            "**Event has ended.** These are the final standings.\n\
             -# Last updated: <t:{unix_time}>"
        )
    } else {
        format!(
            "Last Updated: <t:{unix_time}>\n\
             -# Live standings — updates every {} seconds.",
            ctx.data().config.leaderboard_cache_seconds
        )
    };

    let status_msg = channel
        .send_message(&ctx.http(), CreateMessage::new().content(status_content))
        .await?;

    let status_message_id = status_msg.id.get() as i64;

    // Store in database.
    let now = chrono::Utc::now();
    let message_ids_json = serde_json::json!(message_ids);

    queries::upsert_persistent_event_leaderboard(
        &ctx.data().db,
        event.id,
        guild_id_i64,
        channel.get() as i64,
        &message_ids_json,
        status_message_id,
        milestone_message_id,
        display_limit as i32,
        &now,
        &now,
    )
    .await?;

    // Confirmation embed.
    let update_note = if event.status == "ended" {
        "Event has ended — standings are frozen.".to_string()
    } else {
        format!(
            "Auto-updates every {} seconds.",
            ctx.data().config.leaderboard_cache_seconds
        )
    };

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            CreateEmbed::new()
                .title("Persistent Event Leaderboard Created")
                .color(0x00BFFF)
                .field("Event", &event.name, true)
                .field("Channel", format!("<#{}>", channel.get()), true)
                .field("Pages", total_pages.to_string(), true)
                .field("Status", update_note, false),
        ),
    )
    .await?;

    info!(
        event_id = event.id,
        guild_id = guild_id_i64,
        channel_id = channel.get(),
        pages = total_pages,
        "Created persistent event leaderboard."
    );

    logger(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        LogType::Info,
        format!(
            "{} created a persistent event leaderboard for **{}** in <#{}> ({} page(s))",
            ctx.author().name,
            event.name,
            channel.get(),
            total_pages
        ),
    )
    .await?;

    Ok(())
}

/// Generate an embed showing the current status of an event.
async fn generate_event_status_embed(pool: &PgPool, event: &DbEvent) -> Result<CreateEmbed, Error> {
    let now = chrono::Utc::now();
    let mut embed = CreateEmbed::new().title(&event.name).color(0x00BFFF);

    match event.status.as_str() {
        "pending" => {
            embed = embed.field("Status", "Pending", true);
            if event.start_date > now {
                let countdown = event.start_date.signed_duration_since(now);
                let days = countdown.num_days();
                let hours = countdown.num_hours() % 24;
                let mins = countdown.num_minutes() % 60;
                embed = embed.field("Starts In", format!("{}d {}h {}m", days, hours, mins), true);
            }
            embed = embed.field(
                "Starts",
                format!("<t:{}:F>", event.start_date.timestamp()),
                false,
            );
        }
        "active" => {
            embed = embed.field("Status", "Active", true);
            let participants = queries::count_event_participants(pool, event.id).await?;
            embed = embed.field("Participants", participants.to_string(), true);
            if event.end_date > now {
                let countdown = event.end_date.signed_duration_since(now);
                let days = countdown.num_days();
                let hours = countdown.num_hours() % 24;
                let mins = countdown.num_minutes() % 60;
                embed = embed.field("Ends In", format!("{}d {}h {}m", days, hours, mins), true);
            }
            embed = embed.field(
                "Ends",
                format!("<t:{}:F>", event.end_date.timestamp()),
                false,
            );
        }
        "ended" => {
            embed = embed.field("Status", "Ended", true);
            let participants = queries::count_event_participants(pool, event.id).await?;
            embed = embed.field("Participants", participants.to_string(), true);
            let duration = event.end_date.signed_duration_since(event.start_date);
            let days = duration.num_days();
            embed = embed.field("Duration", format!("{} days", days), true);
            embed = embed.footer(CreateEmbedFooter::new(
                "View results with /event leaderboard",
            ));
        }
        _ => {}
    }

    // Show stats for pending and active events
    if event.status == "pending" || event.status == "active" {
        let stats = queries::get_event_stats(pool, event.id).await?;
        if !stats.is_empty() {
            let mut stats_desc = String::new();
            for stat in stats {
                let display_name = display_name_for_key(&stat.stat_name);
                stats_desc.push_str(&format!(
                    "• {} — +{} XP\n",
                    display_name, stat.xp_per_unit as i32
                ));
            }
            embed = embed.field("Stats Enabled", stats_desc, false);
        }
    }

    Ok(embed)
}

#[poise::command(slash_command, subcommands("create", "remove"))]
pub async fn status(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn create(
    ctx: Context<'_>,
    #[description = "Select the event to send the status for (defaults to latest event)"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: Option<String>,
    #[description = "Channel to post the status message in"] channel: serenity::ChannelId,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server.")?;
    let guild_id_i64 = guild_id.get() as i64;

    // Resolve event — use provided name or fall back to the latest event.
    let resolved_name = match event_name {
        Some(n) => n,
        None => queries::get_latest_event_name(&ctx.data().db, guild_id_i64)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No event currently scheduled."))?,
    };

    let event = queries::get_event_by_name(&ctx.data().db, guild_id_i64, &resolved_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Event **{}** not found.", resolved_name))?;

    // If there's already a status message for this event, delete the old one.
    if let Some(existing) = queries::get_event_status_message(&ctx.data().db, event.id).await? {
        let old_channel = serenity::ChannelId::new(existing.channel_id as u64);
        let _ = old_channel
            .delete_message(
                &ctx.http(),
                serenity::MessageId::new(existing.message_id as u64),
            )
            .await;
        queries::delete_event_status_message(&ctx.data().db, event.id).await?;
    }

    // Generate the status embed
    let embed = generate_event_status_embed(&ctx.data().db, &event).await?;

    // Send the message
    let msg = channel
        .send_message(&ctx.http(), CreateMessage::new().embed(embed))
        .await?;

    let message_id = msg.id.get() as i64;

    // Store in database
    let now = chrono::Utc::now();
    queries::upsert_event_status_message(
        &ctx.data().db,
        event.id,
        channel.get() as i64,
        message_id,
        &now,
        &now,
    )
    .await?;

    // Confirmation
    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            CreateEmbed::new()
                .title("Event Status Message Created")
                .color(0x00BFFF)
                .field("Event", &event.name, true)
                .field("Channel", format!("<#{}>", channel.get()), true)
                .field("Status", "Auto-updates periodically", false),
        ),
    )
    .await?;

    info!(
        event_id = event.id,
        guild_id = guild_id_i64,
        channel_id = channel.get(),
        "Created persistent event status message."
    );

    logger(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        LogType::Info,
        format!(
            "{} created a persistent event status message for **{}** in <#{}>",
            ctx.author().name,
            event.name,
            channel.get()
        ),
    )
    .await?;

    Ok(())
}

#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Select the event whose status message should be removed (defaults to latest event)"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server.")?;
    let guild_id_i64 = guild_id.get() as i64;

    // Resolve event — use provided name or fall back to the latest event.
    let resolved_name = match event_name {
        Some(n) => n,
        None => queries::get_latest_event_name(&ctx.data().db, guild_id_i64)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No event currently scheduled."))?,
    };

    let event = queries::get_event_by_name(&ctx.data().db, guild_id_i64, &resolved_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Event **{}** not found.", resolved_name))?;

    if let Some(existing) = queries::get_event_status_message(&ctx.data().db, event.id).await? {
        // Attempt to delete the message from Discord
        let old_channel = serenity::ChannelId::new(existing.channel_id as u64);
        let _ = old_channel
            .delete_message(
                &ctx.http(),
                serenity::MessageId::new(existing.message_id as u64),
            )
            .await;

        queries::delete_event_status_message(&ctx.data().db, event.id).await?;
    }

    // Confirmation
    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            CreateEmbed::new()
                .title("Event Status Message Removed")
                .color(0x00BFFF)
                .field("Event", &event.name, true),
        ),
    )
    .await?;

    info!(
        event_id = event.id,
        guild_id = guild_id_i64,
        "Removed persistent event status message."
    );

    logger(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        LogType::Info,
        format!(
            "{} removed the persistent event status message for **{}**",
            ctx.author().name,
            event.name
        ),
    )
    .await?;

    Ok(())
}

/// Add XP-threshold milestones to an event (pending or active only).
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "milestones-add",
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn milestones_add(
    ctx: Context<'_>,
    #[description = "Event name"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
    #[description = "Comma-separated XP thresholds, e.g. 500, 1000, 5000"] milestones: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{event_name}** not found."))
                .await?;
            return Ok(());
        }
    };

    if event.status == "ended" {
        ctx.say("Cannot add milestones to an event that has already ended.")
            .await?;
        return Ok(());
    }

    // Parse the comma-separated thresholds.
    let thresholds: Vec<f64> = milestones
        .split(',')
        .filter_map(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return None;
            }
            trimmed.parse::<f64>().ok()
        })
        .filter(|&v| v > 0.0)
        .collect();

    if thresholds.is_empty() {
        ctx.say("No valid positive XP thresholds found. Use a format like `500, 1000, 5000`.")
            .await?;
        return Ok(());
    }

    let inserted = queries::add_event_milestones(&data.db, event.id, &thresholds).await?;

    let threshold_list = thresholds
        .iter()
        .map(|v| format!("{v}"))
        .collect::<Vec<_>>()
        .join(", ");

    ctx.say(format!(
        "Added **{inserted}** new milestone(s) to **{event_name}** (thresholds: {threshold_list}).\n\
         Use `/edit-event milestones-remove` to remove any."
    ))
    .await?;

    logger(
        ctx.serenity_context(),
        data,
        ctx.guild_id().unwrap(),
        LogType::Info,
        format!(
            "{} added milestones to **{}**: {}",
            ctx.author().name,
            event_name,
            threshold_list
        ),
    )
    .await?;

    Ok(())
}

/// Remove XP-threshold milestones from an event (pending or active only).
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "milestones-remove",
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn milestones_remove(
    ctx: Context<'_>,
    #[description = "Event name"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
    #[description = "Comma-separated XP thresholds to remove, e.g. 500, 1000"] milestones: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{event_name}** not found."))
                .await?;
            return Ok(());
        }
    };

    if event.status == "ended" {
        ctx.say("Cannot remove milestones from an event that has already ended.")
            .await?;
        return Ok(());
    }

    // Parse the comma-separated thresholds.
    let thresholds: Vec<f64> = milestones
        .split(',')
        .filter_map(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return None;
            }
            trimmed.parse::<f64>().ok()
        })
        .filter(|&v| v > 0.0)
        .collect();

    if thresholds.is_empty() {
        ctx.say("No valid XP thresholds found. Use a format like `500, 1000`.")
            .await?;
        return Ok(());
    }

    let removed = queries::remove_event_milestones(&data.db, event.id, &thresholds).await?;

    let threshold_list = thresholds
        .iter()
        .map(|v| format!("{v}"))
        .collect::<Vec<_>>()
        .join(", ");

    ctx.say(format!(
        "Removed **{removed}** milestone(s) from **{event_name}** (thresholds: {threshold_list})."
    ))
    .await?;

    logger(
        ctx.serenity_context(),
        data,
        ctx.guild_id().unwrap(),
        LogType::Info,
        format!(
            "{} removed milestones from **{}**: {}",
            ctx.author().name,
            event_name,
            threshold_list
        ),
    )
    .await?;

    Ok(())
}

/// Show a participant-centric view of milestone completers for an event.
///
/// For each participant who completed at least one milestone, lists the
/// milestones they reached. Admin-only, ephemeral.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "milestones-completers",
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn milestones_completers(
    ctx: Context<'_>,
    #[description = "Event name (defaults to most recent)"]
    #[autocomplete = "autocomplete_any_event_name"]
    event_name: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let event_name = match event_name {
        Some(n) => n,
        None => queries::get_latest_event_name(&data.db, guild_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No active or ended events found"))?,
    };

    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{event_name}** not found."))
                .await?;
            return Ok(());
        }
    };

    let milestones = queries::get_event_milestones(&data.db, event.id).await?;

    if milestones.is_empty() {
        ctx.say(format!(
            "No milestones have been configured for **{}** yet. Use `/edit-event milestones-add` to add some.",
            event.name
        ))
        .await?;
        return Ok(());
    }

    // Build a map: participant discord_user_id -> Vec of thresholds they completed.
    use std::collections::HashMap;
    let mut participant_milestones: HashMap<i64, Vec<f64>> = HashMap::new();

    for milestone in &milestones {
        let completers =
            queries::get_event_milestone_completers(&data.db, event.id, milestone.xp_threshold)
                .await
                .unwrap_or_default();

        for user_id in completers {
            participant_milestones
                .entry(user_id)
                .or_default()
                .push(milestone.xp_threshold);
        }
    }

    if participant_milestones.is_empty() {
        ctx.say(format!(
            "No participants have completed any milestones for **{}** yet.",
            event.name
        ))
        .await?;
        return Ok(());
    }

    // Sort participants by number of milestones completed (desc), then by user_id for stability.
    let mut sorted: Vec<(i64, Vec<f64>)> = participant_milestones.into_iter().collect();
    sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));

    let mut embed = poise::serenity_prelude::CreateEmbed::new()
        .title(format!("Milestone Completers — {}", event.name))
        .color(0x00BFFF)
        .description(format!(
            "{} participant(s) have completed at least one milestone.",
            sorted.len()
        ));

    // One field per participant (cap at Discord's 25-field limit).
    for (user_id, mut thresholds) in sorted.into_iter().take(25) {
        // Show thresholds in ascending order.
        thresholds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let field_name = format!("<@{user_id}>");
        let threshold_strs: Vec<String> = thresholds
            .iter()
            .map(|&t| format!("{} XP", t as i64))
            .collect();
        let value = threshold_strs.join(", ");

        embed = embed.field(field_name, value, false);
    }

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
