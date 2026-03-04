/// Background stat sweeper.
///
/// Periodically fetches Hypixel stats for all registered users, computes
/// deltas against the most recent snapshot, stores a new snapshot, and feeds
/// the deltas into the points calculator.
///
/// The sweeper is designed to be extensible: adding a new stat source is a
/// matter of fetching additional data, producing `StatDelta` values, and
/// inserting new snapshot rows.
use std::sync::Arc;

use anyhow::Result;
use sqlx::SqlitePool;
use time::OffsetDateTime;
use tracing::{error, info, warn};

use crate::config::GuildConfig;
use crate::database::queries;
use crate::hypixel::client::HypixelClient;
use crate::points::calculator;
use crate::shared::types::StatDelta;

/// Run a single sweep iteration for all registered users.
///
/// This is called by the background loop in `start_sweeper` but is also
/// usable standalone (e.g. for testing or manual triggers).
pub async fn run_sweep(pool: &SqlitePool, hypixel: &Arc<HypixelClient>) -> Result<()> {
    let users = queries::get_all_registered_users(pool).await?;

    if users.is_empty() {
        info!("Sweep: no registered users, skipping.");
        return Ok(());
    }

    info!("Sweep: processing {} registered user(s)...", users.len());

    for user in &users {
        if let Err(e) = sweep_user(pool, hypixel, user).await {
            warn!(
                user_id = user.id,
                discord_user_id = user.discord_user_id,
                error = %e,
                "Sweep: failed to process user, skipping."
            );
        }
    }

    info!("Sweep: iteration complete.");
    Ok(())
}

/// Sweep a single user: fetch stats, diff, snapshot, award points.
async fn sweep_user(
    pool: &SqlitePool,
    hypixel: &Arc<HypixelClient>,
    user: &crate::database::models::DbUser,
) -> Result<()> {
    // 1. Fetch current stats from Hypixel.
    let stats = hypixel.fetch_bedwars_stats(&user.minecraft_uuid).await?;

    let now = OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());

    // 2. For each stat, compare with the latest snapshot and build deltas.
    let mut deltas: Vec<StatDelta> = Vec::new();

    for (stat_name, &new_value) in &stats.stats {
        let previous = queries::get_latest_hypixel_snapshot(pool, user.id, stat_name).await?;

        let old_value = previous.as_ref().map(|s| s.stat_value).unwrap_or(0.0);

        // Always store a new snapshot (even if the value hasn't changed, so we
        // have a continuous timeline).
        queries::insert_hypixel_snapshot(pool, user.id, stat_name, new_value, &now).await?;

        // Only produce a delta if the value actually changed.
        let diff = new_value - old_value;
        if diff.abs() > f64::EPSILON {
            deltas.push(StatDelta::new(
                user.id,
                stat_name.clone(),
                old_value,
                new_value,
            ));
        }
    }

    // 3. If there are meaningful deltas, calculate and award points.
    if !deltas.is_empty() {
        // Load the guild config to get point multipliers.
        let guild_config = load_guild_config(pool, user.guild_id).await;
        let earned = calculator::calculate_points(&guild_config, &deltas);

        if earned > 0.0 {
            queries::upsert_points(pool, user.id, earned, &now).await?;
            info!(user_id = user.id, earned, "Sweep: awarded points to user.");
        }
    }

    Ok(())
}

/// Load and parse the guild config, falling back to defaults on error.
async fn load_guild_config(pool: &SqlitePool, guild_id: i64) -> GuildConfig {
    match queries::get_guild(pool, guild_id).await {
        Ok(Some(guild)) => serde_json::from_str(&guild.config_json).unwrap_or_default(),
        Ok(None) => GuildConfig::default(),
        Err(e) => {
            error!(guild_id, error = %e, "Failed to load guild config, using defaults.");
            GuildConfig::default()
        }
    }
}
