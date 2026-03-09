use chrono::{Duration as ChronoDuration, Utc};
use sqlx::PgPool;
use tracing::{error, info};

use crate::database::queries;

pub async fn start_daily_snapshot_loop(pool: PgPool) {
    loop {
        let now = Utc::now();

        // compute next midnight
        let tomorrow = (now.date_naive() + ChronoDuration::days(1))
            .and_hms_opt(0, 0, 0)
            .unwrap();

        let next_midnight = chrono::DateTime::<Utc>::from_naive_utc_and_offset(tomorrow, Utc);

        let sleep_duration = next_midnight - now;

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

        if let Err(e) = queries::insert_daily_snapshot(&pool).await {
            error!(error = %e, "Daily snapshot job failed.");
        } else {
            info!("Daily snapshot completed successfully.");
        }
    }
}