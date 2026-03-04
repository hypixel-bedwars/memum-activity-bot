/// Stat sweeper background task.
///
/// - `stat_sweeper` — the sweep logic itself.
/// - `start_sweeper` — spawns the recurring tokio task.
pub mod stat_sweeper;

use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tracing::{error, info};

use crate::hypixel::client::HypixelClient;

/// Spawn the stat sweeper as a background tokio task.
///
/// The task runs forever, executing a sweep every `interval_seconds` seconds.
/// Errors during individual sweeps are logged but do not crash the task.
pub fn start_sweeper(pool: SqlitePool, hypixel: Arc<HypixelClient>, interval_seconds: u64) {
    let interval = Duration::from_secs(interval_seconds);

    tokio::spawn(async move {
        info!(interval_secs = interval_seconds, "Stat sweeper started.");

        loop {
            tokio::time::sleep(interval).await;

            info!("Stat sweeper: starting sweep iteration...");

            if let Err(e) = stat_sweeper::run_sweep(&pool, &hypixel).await {
                error!(error = %e, "Stat sweeper: sweep iteration failed.");
            }
        }
    });
}
