/// Persistent leaderboard background updater.
///
/// Runs on a timer (matching the leaderboard cache interval) and edits all
/// persistent leaderboard messages with fresh leaderboard images.
use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude::{self as serenity, CreateAttachment, EditMessage};
use sqlx::SqlitePool;
use tracing::{error, info, warn};

use crate::commands::leaderboard::helpers::{self, PAGE_SIZE};
use crate::config::AppConfig;
use crate::database::queries;

/// Spawn the persistent leaderboard updater as a background tokio task.
///
/// `http` is the Serenity HTTP client for editing Discord messages.
pub fn start_leaderboard_updater(pool: SqlitePool, http: Arc<serenity::Http>, config: AppConfig) {
    let interval = Duration::from_secs(config.leaderboard_cache_seconds);
    let persistent_players = config.persistent_leaderboard_players;

    tokio::spawn(async move {
        info!(
            interval_secs = config.leaderboard_cache_seconds,
            "Persistent leaderboard updater started."
        );

        loop {
            tokio::time::sleep(interval).await;

            if let Err(e) = update_all_leaderboards(&pool, &http, persistent_players).await {
                error!(error = %e, "Leaderboard updater: iteration failed.");
            }
        }
    });
}

/// Update all persistent leaderboards across all guilds.
async fn update_all_leaderboards(
    pool: &SqlitePool,
    http: &Arc<serenity::Http>,
    persistent_players: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let leaderboards = queries::get_all_persistent_leaderboards(pool).await?;

    if leaderboards.is_empty() {
        return Ok(());
    }

    info!(
        "Leaderboard updater: updating {} persistent leaderboard(s)...",
        leaderboards.len()
    );

    for lb in &leaderboards {
        info!(
            guild_id = lb.guild_id,
            status_message_id = lb.status_message_id,
            "Leaderboard updater: loaded leaderboard config."
        );
        if let Err(e) = update_single_leaderboard(
            pool,
            http,
            lb.guild_id,
            lb.channel_id,
            &lb.message_ids,
            lb.status_message_id,
            persistent_players,
        )
        .await
        {
            warn!(
                guild_id = lb.guild_id,
                error = %e,
                "Leaderboard updater: failed to update guild, skipping."
            );
        }
    }

    Ok(())
}

/// Update a single guild's persistent leaderboard.
async fn update_single_leaderboard(
    pool: &SqlitePool,
    http: &Arc<serenity::Http>,
    guild_id: i64,
    channel_id: i64,
    message_ids_json: &str,
    status_message_id: i64,
    persistent_players: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let msg_ids: Vec<u64> = serde_json::from_str(message_ids_json).unwrap_or_default();
    let channel = serenity::ChannelId::new(channel_id as u64);

    let total_pages = ((persistent_players as f64) / PAGE_SIZE as f64)
        .ceil()
        .max(1.0) as u32;

    for (i, msg_id) in msg_ids.iter().enumerate() {
        let page = (i as u32) + 1;
        if page > total_pages {
            break;
        }

        let result = helpers::generate_leaderboard_page(pool, guild_id, page).await;

        let (png_bytes, _) = match result {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    guild_id,
                    page,
                    error = %e,
                    "Leaderboard updater: failed to generate page."
                );
                continue;
            }
        };

        let attachment = CreateAttachment::bytes(png_bytes, format!("leaderboard_page_{page}.png"));

        let edit = EditMessage::new().new_attachment(attachment);

        if let Err(e) = channel
            .edit_message(http, serenity::MessageId::new(*msg_id), edit)
            .await
        {
            warn!(
                guild_id,
                page,
                message_id = msg_id,
                error = %e,
                "Leaderboard updater: failed to edit message."
            );
        }
    }

    // Update last_updated timestamp
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());

    queries::update_persistent_leaderboard_messages(pool, guild_id, message_ids_json, &now).await?;

    let unix_time = time::OffsetDateTime::now_utc().unix_timestamp();

    let status_edit = EditMessage::new().content(format!(
        "Last Fully Updated: <t:{unix_time}>\n\
         -# This is the last updated date of the most outdated player data."
    ));

    info!(
        guild_id,
        status_message_id, "Leaderboard updater: attempting to update status message."
    );

    if status_message_id != 0 {
        info!(
            guild_id,
            status_message_id, "Leaderboard updater: editing status message."
        );

        if let Err(e) = channel
            .edit_message(
                http,
                serenity::MessageId::new(status_message_id as u64),
                status_edit,
            )
            .await
        {
            warn!(
                guild_id,
                status_message_id,
                error = %e,
                "Leaderboard updater: failed to update status message."
            );
        }
    } else {
        warn!(
            guild_id,
            "Leaderboard updater: status_message_id is 0, skipping update."
        );
    }

    Ok(())
}
