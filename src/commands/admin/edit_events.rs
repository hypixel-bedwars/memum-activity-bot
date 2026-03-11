/// `/edit-events` command group — admin only.
///
/// Provides subcommands for managing guild events that track stats and award XP:
/// - `create`       — create a new event (seeds event_stats from guild xp_config)
/// - `edit`         — change event name, description, or dates
/// - `delete`       — delete a pending/active event
/// - `stats-add`    — add a stat to an event
/// - `stats-remove` — remove a stat from an event
/// - `stats-edit`   — change the XP-per-unit for an event stat
/// - `list`         — list all events with their status
///
/// All subcommands are ephemeral and require the admin check.
use poise::serenity_prelude::{self as serenity, CreateEmbed};
use tracing::{error, info};

use crate::commands::logger::logger::{logger, logger_system, LogType};
use crate::config::GuildConfig;
use crate::database::queries;
use crate::shared::types::{Context, Error};
use crate::utils::stats_definitions::display_name_for_key;

/// Autocomplete for event names — returns non-ended events in this guild.
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
        .filter(|e| e.status != "ended")
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
    rename = "edit-events",
    subcommands(
        "create",
        "edit",
        "delete",
        "stats_add",
        "stats_remove",
        "stats_edit",
        "list",
        "backfill"
    )
)]
pub async fn edit_events(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Create a new event. Stats are seeded from the guild's current XP config.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    check = "crate::utils::permissions::admin_check"
)]
pub async fn create(
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

    // Check for duplicate name.
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

    // Create the event.
    let event = queries::create_event(
        &data.db,
        guild_id,
        &name,
        description.as_deref(),
        &start_date,
        &end_date,
    )
    .await?;

    // Seed event_stats from guild xp_config.
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
    check = "crate::utils::permissions::admin_check"
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
    check = "crate::utils::permissions::admin_check"
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
    check = "crate::utils::permissions::admin_check"
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
    check = "crate::utils::permissions::admin_check"
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
    check = "crate::utils::permissions::admin_check"
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
    check = "crate::utils::permissions::admin_check"
)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let events = queries::list_events(&data.db, guild_id).await?;

    if events.is_empty() {
        ctx.say("No events have been created yet. Use `/edit-events create` to create one.")
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
    check = "crate::utils::permissions::admin_check"
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
                    summary.deltas_processed,
                    summary.total_xp_awarded,
                    summary.users_affected,
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
