use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use poise::serenity_prelude as serenity;
use tracing::{error, info};

use crate::commands::logger::logger::LogType;
use crate::database::queries;
use crate::shared::types::Data;

pub async fn start_daily_snapshot_loop(data: Arc<Data>) {
    loop {
        let now = Utc::now();

        // Compute next UTC midnight so snapshot_date always matches the
        // scheduler's notion of "today", regardless of DB server timezone.
        let tomorrow = (now.date_naive() + ChronoDuration::days(1))
            .and_hms_opt(0, 0, 0)
            .expect("midnight is always a valid time");

        let next_midnight = chrono::DateTime::<Utc>::from_naive_utc_and_offset(tomorrow, Utc);

        let sleep_duration = next_midnight - now;

        // Guard against edge cases (e.g. clock skew) — retry in 60 s.
        let sleep_std = match sleep_duration.to_std() {
            Ok(d) => d,
            Err(_) => std::time::Duration::from_secs(60),
        };

        info!(
            seconds = sleep_std.as_secs(),
            "Daily snapshot scheduler sleeping until midnight."
        );

        tokio::time::sleep(sleep_std).await;

        info!("Running daily snapshot job.");

        // Snapshot date is computed in UTC so it is always consistent with
        // the scheduler's UTC midnight boundary, not the DB server timezone.
        let snapshot_date = Utc::now().date_naive();

        match queries::insert_daily_snapshot_for_date(&data.db, snapshot_date).await {
            Err(e) => {
                error!(error = %e, %snapshot_date, "Daily snapshot job failed.");
                broadcast_log(
                    &data,
                    LogType::Error,
                    format!("Daily snapshot for `{}` failed: {}", snapshot_date, e),
                )
                .await;
            }

            Ok(()) => {
                info!(%snapshot_date, "Daily snapshot completed successfully.");
                broadcast_log(
                    &data,
                    LogType::Info,
                    format!(
                        "Daily stats snapshot for **{}** completed successfully.",
                        snapshot_date
                    ),
                )
                .await;
            }
        }
    }
}

/// Send a log message to every guild that has a logging channel configured.
///
/// Uses a single query to fetch all `(guild_id, channel_id)` pairs, then posts
/// directly — no N+1 query per guild. Any failure is only traced locally; it
/// never aborts the snapshot loop.
async fn broadcast_log(data: &Data, log_type: LogType, msg: String) {
    let guild_channels = match queries::get_all_guild_log_channels(&data.db).await {
        Ok(pairs) => pairs,
        Err(e) => {
            error!(error = %e, "Failed to fetch guild log channels for daily snapshot broadcast.");
            return;
        }
    };

    for (_guild_id, channel_id) in guild_channels {
        // Build embed inside the loop — CreateEmbed is cheap to construct.
        let embed = match &log_type {
            LogType::Info => serenity::CreateEmbed::new()
                .title("ℹ️ Info")
                .description(&msg)
                .color(0x3498db_u32),
            LogType::Warn => serenity::CreateEmbed::new()
                .title("⚠️ Warning")
                .description(&msg)
                .color(0xf1c40f_u32),
            LogType::Error => serenity::CreateEmbed::new()
                .title("❌ Error")
                .description(&msg)
                .color(0xe74c3c_u32),
            LogType::Debug => serenity::CreateEmbed::new()
                .title("🐛 Debug")
                .description(&msg)
                .color(0x95a5a6_u32),
        };

        let channel = serenity::ChannelId::new(channel_id as u64);
        let _ = channel
            .send_message(&data.http, serenity::CreateMessage::new().embed(embed))
            .await;
    }
}
