/// `/statistics` command.
///
/// Displays server-wide aggregated statistics as a card image, with optional
/// time-range filtering via a dropdown selector or manual date parameters.
///
/// # Priority order for date range
/// 1. `start_date` + `end_date` provided → exact range, no dropdown
/// 2. Only `start_date`                  → end = now
/// 3. Only `end_date`                    → start = end - 14 days
/// 4. Neither                            → last 14 days (default), dropdown shown
use chrono::{Duration, NaiveDate, Utc};
use poise::serenity_prelude::{
    CreateActionRow, CreateAttachment, CreateSelectMenu, CreateSelectMenuKind,
    CreateSelectMenuOption,
};
use tracing::info;

use crate::cards::statistics::{self, StatisticsCardParams};
use crate::database::queries;
use crate::shared::types::{Context, Error};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Custom ID prefix for the statistics range dropdown.
/// Full ID format: `stats_range_{guild_id}`.
pub const STATS_RANGE_PREFIX: &str = "stats_range_";

/// Available preset ranges: (days, label) pairs.
pub const PRESET_RANGES: &[(i64, &str)] = &[
    (7, "Last 7 Days"),
    (14, "Last 14 Days"),
    (28, "Last 28 Days"),
    (60, "Last 60 Days"),
    (120, "Last 120 Days"),
];

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

/// Show server-wide aggregated statistics.
#[poise::command(slash_command, guild_only)]
pub async fn statistics(
    ctx: Context<'_>,
    #[description = "Start date (YYYY-MM-DD). Defaults to 14 days ago."] start_date: Option<
        String,
    >,
    #[description = "End date (YYYY-MM-DD). Defaults to today."] end_date: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;

    let now = Utc::now();

    // Determine date range, subtitle, and whether to show the dropdown.
    // Returns (start_dt, end_dt, subtitle, show_dropdown, selected_days).
    let (start_dt, end_dt, subtitle, show_dropdown, selected_days) =
        match (start_date.as_deref(), end_date.as_deref()) {
            (Some(s), Some(e)) => {
                // Both provided: exact range, no dropdown.
                let start = parse_date_start(s)?;
                let end = parse_date_end(e)?;
                if start > end {
                    ctx.say("**Error:** `start_date` must be before `end_date`.")
                        .await?;
                    return Ok(());
                }
                let sub = format!("{} \u{2192} {}", s, e);
                (start, end, sub, false, 0i64)
            }
            (Some(s), None) => {
                // Only start_date: end = now.
                let start = parse_date_start(s)?;
                let end_label = now.format("%Y-%m-%d").to_string();
                let sub = format!("{} \u{2192} {}", s, end_label);
                (start, now, sub, false, 0i64)
            }
            (None, Some(e)) => {
                // Only end_date: start = end - 14 days.
                let end = parse_date_end(e)?;
                let start = end - Duration::days(14);
                let start_label = start.format("%Y-%m-%d").to_string();
                let sub = format!("{} \u{2192} {}", start_label, e);
                (start, end, sub, false, 0i64)
            }
            (None, None) => {
                // Default: last 14 days + dropdown.
                let start = now - Duration::days(14);
                (start, now, "Last 14 Days".to_string(), true, 14i64)
            }
        };

    let stats =
        queries::get_guild_statistics_ranged(&ctx.data().db, guild_id, start_dt, end_dt).await?;

    let params = StatisticsCardParams {
        title: "Server Statistics".to_string(),
        subtitle: Some(subtitle),
        stats,
    };

    let png_bytes = statistics::render(&params);
    let attachment = CreateAttachment::bytes(png_bytes, "statistics.png");

    let mut reply = poise::CreateReply::default().attachment(attachment);

    if show_dropdown {
        let components = build_range_components(guild_id, selected_days);
        reply = reply.components(components);
    }

    ctx.send(reply).await?;

    info!(
        "Sent server statistics card for guild {} (range: {} days from {})",
        guild_id,
        (end_dt - start_dt).num_days(),
        start_dt.format("%Y-%m-%d")
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Component builder (pub so event handler can use it for re-renders)
// ---------------------------------------------------------------------------

/// Build the dropdown `ActionRow` components for range selection.
///
/// `selected_days` marks which option is pre-selected (use `14` for default).
pub fn build_range_components(guild_id: i64, selected_days: i64) -> Vec<CreateActionRow> {
    let options: Vec<CreateSelectMenuOption> = PRESET_RANGES
        .iter()
        .map(|(days, label)| {
            let opt = CreateSelectMenuOption::new(*label, days.to_string());
            if *days == selected_days {
                opt.default_selection(true)
            } else {
                opt
            }
        })
        .collect();

    let menu = CreateSelectMenu::new(
        format!("{}{}", STATS_RANGE_PREFIX, guild_id),
        CreateSelectMenuKind::String { options },
    )
    .placeholder("Select time range");

    vec![CreateActionRow::SelectMenu(menu)]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a `YYYY-MM-DD` string as the **start** of that day (00:00:00 UTC).
fn parse_date_start(s: &str) -> Result<chrono::DateTime<Utc>, Error> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| format!("Invalid date format: `{}`. Expected `YYYY-MM-DD`.", s))?;
    Ok(d.and_hms_opt(0, 0, 0)
        .ok_or("Invalid time")?
        .and_utc())
}

/// Parse a `YYYY-MM-DD` string as the **end** of that day (23:59:59 UTC).
fn parse_date_end(s: &str) -> Result<chrono::DateTime<Utc>, Error> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| format!("Invalid date format: `{}`. Expected `YYYY-MM-DD`.", s))?;
    Ok(d.and_hms_opt(23, 59, 59)
        .ok_or("Invalid time")?
        .and_utc())
}
