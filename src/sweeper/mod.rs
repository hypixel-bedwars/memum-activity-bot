/// Stat sweeper background task.
///
/// - `stat_sweeper` - source-specific sweep logic.
/// - `start_hypixel_sweeper` - Hypixel loop (slow interval).
/// - `start_discord_sweeper` - Discord loop (fast interval).
pub mod stat_sweeper;

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tracing::{debug, error, info};

use crate::config::AppConfig;
use crate::hypixel::client::HypixelClient;

/// Spawn the Hypixel sweeper as a background tokio task.
pub fn start_hypixel_sweeper(
    pool: PgPool,
    hypixel: Arc<HypixelClient>,
    interval_seconds: u64,
    config: AppConfig,
) {
    let interval = Duration::from_secs(interval_seconds);

    tokio::spawn(async move {
        info!(interval_secs = interval_seconds, "Hypixel sweeper started.");

        loop {
            tokio::time::sleep(interval).await;

            info!("Hypixel sweeper: starting sweep iteration...");

            if let Err(e) = stat_sweeper::run_hypixel_sweep(&pool, &hypixel, &config).await {
                error!(error = %e, "Hypixel sweeper: sweep iteration failed.");
            }
        }
    });
}

/// Spawn the Discord sweeper as a background tokio task.
pub fn start_discord_sweeper(pool: PgPool, interval_seconds: u64, config: AppConfig) {
    let interval = Duration::from_secs(interval_seconds);

    tokio::spawn(async move {
        info!(interval_secs = interval_seconds, "Discord sweeper started.");

        loop {
            tokio::time::sleep(interval).await;

            debug!("Discord sweeper: starting sweep iteration...");

            if let Err(e) = stat_sweeper::run_discord_sweep(&pool, &config).await {
                error!(error = %e, "Discord sweeper: sweep iteration failed.");
            }
        }
    });
}
