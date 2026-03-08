/// Stat sweeper background task.
///
/// - `stat_sweeper` - source-specific sweep logic.
/// - `start_hypixel_sweeper` - Hypixel loop (per-user 1s delay + configurable
///   rest period between full cycles).
/// - `start_discord_sweeper` - Discord loop (fast interval).
/// - `refresh_hypixel_user_if_stale` - on-demand refresh gate used by
///   commands: records command activity, checks the per-user cooldown, and
///   calls `sweep_hypixel_user` when the cooldown has elapsed.
pub mod stat_sweeper;

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;
use tracing::{debug, error, info, warn};

use crate::config::AppConfig;
use crate::database::models::DbUser;
use crate::database::queries;
use crate::hypixel::client::HypixelClient;

/// Spawn the Hypixel sweeper as a background tokio task.
///
/// Loop structure:
///   1. Run a full sweep cycle (all users, 1-second delay between each).
///   2. Sleep for `interval_seconds` (rest period) before starting the next
///      cycle.
///
/// The 1-second per-user delay is the primary rate control mechanism and
/// keeps Hypixel API usage well under the platform limit.  The rest period
/// prevents the sweeper from restarting immediately after a large user list
/// has been processed.
pub fn start_hypixel_sweeper(
    pool: PgPool,
    hypixel: Arc<HypixelClient>,
    interval_seconds: u64,
    config: AppConfig,
) {
    if !config.enable_hypixel_sweeper {
        info!("Hypixel sweeper disabled.");
        return;
    }

    let rest = Duration::from_secs(interval_seconds);

    tokio::spawn(async move {
        info!(
            rest_secs = interval_seconds,
            "Hypixel sweeper started (1s per-user delay + {}s rest between cycles).",
            interval_seconds
        );

        loop {
            info!("Hypixel sweeper: starting sweep cycle...");

            if let Err(e) = stat_sweeper::run_hypixel_sweep(&pool, &hypixel, &config).await {
                error!(error = %e, "Hypixel sweeper: sweep cycle failed.");
            }

            debug!(
                rest_secs = interval_seconds,
                "Hypixel sweeper: cycle complete, resting."
            );
            tokio::time::sleep(rest).await;
        }
    });
}

/// On-demand Hypixel refresh gate, called by stat commands (`/level`, `/stats`).
///
/// This function:
///   1. Always updates `last_command_activity` for the user so the background
///      sweeper knows the user is active.
///   2. Checks whether the per-user refresh cooldown has elapsed since
///      `last_hypixel_refresh`.
///   3. If the cooldown has elapsed (or the user has never been refreshed),
///      calls `sweep_hypixel_user` and awaits the result.  Errors are logged
///      non-fatally so that commands always return a response.
///
/// Returns `true` if a live Hypixel refresh was performed, `false` if the
/// cached data was fresh enough.  Callers can use this to add a "just
/// refreshed" indicator to their response if desired.
pub(crate) async fn refresh_hypixel_user_if_stale(
    pool: &PgPool,
    hypixel: &Arc<HypixelClient>,
    user: &DbUser,
    config: &AppConfig,
) -> bool {
    let now = Utc::now();

    // Always stamp activity so the sweeper prioritises this user.
    if let Err(e) = queries::update_last_command_activity(pool, user.id, &now).await {
        warn!(
            user_id = user.id,
            error = %e,
            "refresh_hypixel_user_if_stale: failed to update last_command_activity."
        );
    }

    // Check whether the cooldown has elapsed.
    let cooldown = Duration::from_secs(config.hypixel_refresh_cooldown_seconds);
    let needs_refresh = match user.last_hypixel_refresh {
        None => true, // never refreshed — always fetch
        Some(last) => {
            let elapsed = now.signed_duration_since(last);
            elapsed > chrono::Duration::from_std(cooldown).unwrap_or(chrono::Duration::seconds(60))
        }
    };

    if !needs_refresh {
        debug!(
            user_id = user.id,
            "refresh_hypixel_user_if_stale: data is fresh, skipping API call."
        );
        return false;
    }

    debug!(
        user_id = user.id,
        "refresh_hypixel_user_if_stale: cooldown elapsed, fetching fresh Hypixel data."
    );

    if let Err(e) = stat_sweeper::sweep_hypixel_user(pool, hypixel, user, config).await {
        warn!(
            user_id = user.id,
            error = %e,
            "refresh_hypixel_user_if_stale: on-demand refresh failed, using cached data."
        );
        return false;
    }

    info!(
        user_id = user.id,
        "refresh_hypixel_user_if_stale: on-demand refresh complete."
    );
    true
}
