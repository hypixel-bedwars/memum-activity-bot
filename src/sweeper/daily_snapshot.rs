use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use tracing::{error, info};

use crate::commands::logger::logger::{LogType, logger_system};
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

                // Transition events between pending/active/ended now that a new
                // snapshot is available for accurate start_snapshot_date resolution.
                if let Err(e) = queries::update_event_statuses(&data.db).await {
                    error!(error = %e, "Failed to update event statuses after daily snapshot.");
                    broadcast_log(
                        &data,
                        LogType::Warn,
                        format!(
                            "Daily snapshot succeeded but failed to update event statuses: {}",
                            e
                        ),
                    )
                    .await;
                }
            }
        }
    }
}

/// Send a log message to every guild that has a logging channel configured.
/// Any failure is only traced — it never aborts the snapshot loop.
async fn broadcast_log(data: &Data, log_type: LogType, msg: String) {
    let guild_ids = match queries::get_guilds_with_log_channel(&data.db).await {
        Ok(ids) => ids,
        Err(e) => {
            error!(error = %e, "Failed to fetch guilds for daily snapshot log.");
            return;
        }
    };

    for guild_id in guild_ids {
        // Re-create the variant each iteration since LogType is not Copy.
        let lt = match log_type {
            LogType::Info => LogType::Info,
            LogType::Warn => LogType::Warn,
            LogType::Error => LogType::Error,
            LogType::Debug => LogType::Debug,
        };

        logger_system(&data.http, &data.db, guild_id, lt, msg.clone()).await;
    }
}
