/// Persistent event leaderboard and status message background updater.
///
/// Runs on a timer (matching the leaderboard cache interval) and edits all
/// persistent event leaderboard messages with fresh images, and updates event
/// status messages with current information.
///
/// The updater stops refreshing an event's leaderboard once the event has
/// ended and the final frozen standings have been captured — it does one last
/// update after the event ends and then skips all subsequent ticks.
use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude::{self as serenity, CreateAttachment, CreateEmbed, EditMessage};
use sqlx::PgPool;
use tracing::{error, info, warn};

use crate::commands::leaderboard::helpers;
use crate::database::queries;
use crate::utils::stats_definitions::display_name_for_key;

/// Spawn the persistent event leaderboard and status updater as a background tokio task.
///
/// `http` is the Serenity HTTP client for editing Discord messages.
/// `interval_secs` is how often (in seconds) to refresh all persistent event
/// leaderboards and status messages — should match `leaderboard_cache_seconds`.
pub fn start_event_leaderboard_updater(
    pool: PgPool,
    http: Arc<serenity::Http>,
    interval_secs: u64,
) {
    let interval = Duration::from_secs(interval_secs);

    tokio::spawn(async move {
        info!(
            interval_secs,
            "Persistent event leaderboard and status updater started."
        );

        loop {
            tokio::time::sleep(interval).await;

            if let Err(e) = update_all_event_leaderboards(&pool, &http).await {
                error!(error = %e, "Event leaderboard updater: iteration failed.");
            }

            if let Err(e) = update_all_event_status_messages(&pool, &http).await {
                error!(error = %e, "Event status updater: iteration failed.");
            }
        }
    });
}

/// Update all persistent event leaderboards.
async fn update_all_event_leaderboards(
    pool: &PgPool,
    http: &Arc<serenity::Http>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let leaderboards = queries::get_all_persistent_event_leaderboards(pool).await?;

    if leaderboards.is_empty() {
        return Ok(());
    }

    info!(
        "Event leaderboard updater: updating {} persistent event leaderboard(s)...",
        leaderboards.len()
    );

    for lb in &leaderboards {
        if let Err(e) = update_single_event_leaderboard(pool, http, lb).await {
            warn!(
                event_id = lb.event_id,
                error = %e,
                "Event leaderboard updater: failed to update event leaderboard, skipping."
            );
        }
    }

    Ok(())
}

/// Update all persistent event status messages.
async fn update_all_event_status_messages(
    pool: &PgPool,
    http: &Arc<serenity::Http>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let status_messages = queries::get_all_event_status_messages(pool).await?;

    if status_messages.is_empty() {
        return Ok(());
    }

    info!(
        "Event status updater: updating {} persistent event status message(s)...",
        status_messages.len()
    );

    for sm in &status_messages {
        if let Err(e) = update_single_event_status_message(pool, http, sm).await {
            warn!(
                event_id = sm.event_id,
                error = %e,
                "Event status updater: failed to update event status message, skipping."
            );
        }
    }

    Ok(())
}

/// Update a single event's persistent leaderboard.
async fn update_single_event_leaderboard(
    pool: &PgPool,
    http: &Arc<serenity::Http>,
    record: &crate::database::models::DbPersistentEventLeaderboard,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Load the event to know its status, name, and dates.
    let event = match queries::get_event_by_id(pool, record.event_id).await? {
        Some(e) => e,
        None => {
            warn!(
                event_id = record.event_id,
                "Event leaderboard updater: event not found, skipping."
            );
            return Ok(());
        }
    };

    // If the event has ended AND we already updated after the end date, this
    // leaderboard is frozen — skip all future updates.
    if event.status == "ended" && record.last_updated > event.end_date {
        return Ok(());
    }

    let is_final_update = event.status == "ended";

    let channel = serenity::ChannelId::new(record.channel_id as u64);
    let msg_ids: Vec<u64> = serde_json::from_value(record.message_ids.clone()).unwrap_or_default();

    // Determine how many pages to render.
    let total_participants = queries::count_event_participants(pool, record.event_id).await?;
    let total_pages = ((total_participants as f64) / helpers::PAGE_SIZE as f64)
        .ceil()
        .max(1.0) as u32;

    // Update (or re-render) each page message.
    for (i, msg_id) in msg_ids.iter().enumerate() {
        let page = (i as u32) + 1;
        if page > total_pages {
            break;
        }

        let result = helpers::generate_event_leaderboard_page(
            pool,
            record.event_id,
            &event.name,
            &event.status,
            event.start_date.timestamp(),
            page,
        )
        .await;

        let (png_bytes, _) = match result {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    event_id = record.event_id,
                    page,
                    error = %e,
                    "Event leaderboard updater: failed to generate page."
                );
                continue;
            }
        };

        let attachment =
            CreateAttachment::bytes(png_bytes, format!("event_leaderboard_page_{page}.png"));
        let edit = EditMessage::new().new_attachment(attachment);

        if let Err(e) = channel
            .edit_message(http, serenity::MessageId::new(*msg_id), edit)
            .await
        {
            warn!(
                event_id = record.event_id,
                page,
                message_id = msg_id,
                error = %e,
                "Event leaderboard updater: failed to edit message."
            );
        }
    }

    // Update status message.
    let unix_time = time::OffsetDateTime::now_utc().unix_timestamp();
    let status_content = if is_final_update {
        format!(
            "**Event has ended.** These are the final standings.\n\
             -# Last updated: <t:{unix_time}>"
        )
    } else {
        format!(
            "Last Updated: <t:{unix_time}>\n\
             -# Live standings — updates every few minutes."
        )
    };

    if record.status_message_id != 0 {
        let status_edit = EditMessage::new().content(status_content);
        if let Err(e) = channel
            .edit_message(
                http,
                serenity::MessageId::new(record.status_message_id as u64),
                status_edit,
            )
            .await
        {
            warn!(
                event_id = record.event_id,
                status_message_id = record.status_message_id,
                error = %e,
                "Event leaderboard updater: failed to update status message."
            );
        }
    }

    // Persist the updated timestamp so we know this tick was processed.
    let now = chrono::Utc::now();
    let message_ids_json = record.message_ids.clone();
    queries::update_persistent_event_leaderboard_messages(
        pool,
        record.event_id,
        &message_ids_json,
        &now,
    )
    .await?;

    if is_final_update {
        info!(
            event_id = record.event_id,
            "Event leaderboard updater: event ended — final standings captured, freezing."
        );
    } else {
        info!(
            event_id = record.event_id,
            "Event leaderboard updater: updated event leaderboard."
        );
    }

    Ok(())
}

/// Update a single event's persistent status message.
async fn update_single_event_status_message(
    pool: &PgPool,
    http: &Arc<serenity::Http>,
    record: &crate::database::models::DbEventStatusMessage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Load the event to know its status, name, and dates.
    let event = match queries::get_event_by_id(pool, record.event_id).await? {
        Some(e) => e,
        None => {
            warn!(
                event_id = record.event_id,
                "Event status updater: event not found, skipping."
            );
            return Ok(());
        }
    };

    let channel = serenity::ChannelId::new(record.channel_id as u64);

    // Generate the status embed
    let embed = generate_event_status_embed(pool, &event).await?;

    let edit = EditMessage::new().embed(embed);

    if let Err(e) = channel
        .edit_message(
            http,
            serenity::MessageId::new(record.message_id as u64),
            edit,
        )
        .await
    {
        warn!(
            event_id = record.event_id,
            message_id = record.message_id,
            error = %e,
            "Event status updater: failed to update status message."
        );
    } else {
        // Update the updated_at timestamp
        let now = chrono::Utc::now();
        queries::upsert_event_status_message(
            pool,
            record.event_id,
            record.channel_id,
            record.message_id,
            &record.created_at,
            &now,
        )
        .await?;
    }

    info!(
        event_id = record.event_id,
        "Event status updater: updated event status message."
    );

    Ok(())
}

/// Generate an embed showing the current status of an event.
async fn generate_event_status_embed(
    pool: &PgPool,
    event: &crate::database::models::DbEvent,
) -> Result<CreateEmbed, Box<dyn std::error::Error + Send + Sync>> {
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
            embed = embed.footer(serenity::CreateEmbedFooter::new(
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
