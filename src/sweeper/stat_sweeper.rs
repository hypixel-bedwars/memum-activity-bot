/// Background stat sweeper.
///
/// Periodically fetches Hypixel stats for all registered users, computes
/// deltas against the most recent snapshot, stores a new snapshot, and feeds
/// the deltas into the XP calculator. Also collects Discord activity deltas
/// when enabled for the guild.
///
/// The sweeper is designed to be extensible: adding a new stat source is a
/// matter of fetching additional data, producing `StatDelta` values, and
/// inserting new snapshot rows.
use std::sync::Arc;

use anyhow::Result;
use sqlx::SqlitePool;
use time::OffsetDateTime;
use tracing::{error, info, warn};

use crate::config::{AppConfig, GuildConfig};
use crate::database::queries;
use crate::hypixel::client::HypixelClient;
use crate::xp::calculator;
use crate::shared::types::StatDelta;

/// Discord stat names that are tracked in the `discord_stats_snapshot` table.
const DISCORD_STAT_NAMES: &[&str] = &["messages_sent", "reactions_added", "commands_used"];

/// Run a single sweep iteration for all registered users.
///
/// This is called by the background loop in `start_sweeper` but is also
/// usable standalone (e.g. for testing or manual triggers).
pub async fn run_sweep(
    pool: &SqlitePool,
    hypixel: &Arc<HypixelClient>,
    config: &AppConfig,
) -> Result<()> {
    let users = queries::get_all_registered_users(pool).await?;

    if users.is_empty() {
        info!("Sweep: no registered users, skipping.");
        return Ok(());
    }

    info!("Sweep: processing {} registered user(s)...", users.len());

    for user in &users {
        if let Err(e) = sweep_user(pool, hypixel, user, config).await {
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

/// Sweep a single user: fetch stats, diff, snapshot, award XP, update level.
async fn sweep_user(
    pool: &SqlitePool,
    hypixel: &Arc<HypixelClient>,
    user: &crate::database::models::DbUser,
    config: &AppConfig,
) -> Result<()> {
    // 1. Fetch current stats from Hypixel.
    let player_data = hypixel.fetch_player(&user.minecraft_uuid).await?;
    let stats = &player_data.bedwars;

    let now = OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());

    // 2. For each Hypixel stat, compare with the latest snapshot and build deltas.
    let mut deltas: Vec<StatDelta> = Vec::new();

    for (stat_name, &new_value) in &stats.stats {
        let previous = queries::get_latest_hypixel_snapshot(pool, user.id, stat_name).await?;

        // If no previous snapshot exists, create a baseline snapshot and skip XP
        if previous.is_none() {
            queries::insert_hypixel_snapshot(pool, user.id, stat_name, new_value, &now).await?;
            continue;
        }

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

    // 3. Collect Discord activity deltas (if enabled for this guild).
    let guild_config = load_guild_config(pool, user.guild_id).await;

    if guild_config.discord_stats_enabled {
        for &stat_name in DISCORD_STAT_NAMES {
            let latest = queries::get_latest_discord_snapshot(pool, user.id, stat_name).await?;
            if let Some(snap) = latest {
                let current_value = snap.stat_value;

                // Get the user's XP row to find last_updated (last sweep time).
                // Compare the current cumulative discord stat value against the
                // value that was current at the last sweep time.
                let xp_row = queries::get_xp(pool, user.id).await?;
                let old_value = match &xp_row {
                    Some(xp) => {
                        get_discord_value_at_time(pool, user.id, stat_name, &xp.last_updated)
                            .await
                            .unwrap_or(0.0)
                    }
                    None => 0.0,
                };

                let diff = current_value - old_value;
                if diff > f64::EPSILON {
                    deltas.push(StatDelta::new(
                        user.id,
                        stat_name.to_string(),
                        old_value,
                        current_value,
                    ));
                }
            }
        }
    }

    // 4. If there are meaningful deltas, calculate XP and update total/level.
    if !deltas.is_empty() {
        // Build XP config from guild config
        let xp_cfg = crate::xp::XPConfig::new(guild_config.xp_config.clone());

        let earned = calculator::calculate_xp(&deltas, &xp_cfg);
        if earned > 0.0 {
            // Fetch current XP total and level
            let xp_row = queries::get_xp(pool, user.id).await?;
            let current_xp = xp_row.as_ref().map(|x| x.total_xp).unwrap_or(0.0);
            let old_level = xp_row.as_ref().map(|x| x.level).unwrap_or(1);

            let new_total = current_xp + earned;
            let new_level = calculator::calculate_level(
                new_total,
                config.base_level_xp,
                config.level_exponent,
            ) as i64;

            // Persist new total XP and level
            queries::set_xp_and_level(pool, user.id, new_total, new_level, &now).await?;

            info!(
                user_id = user.id,
                earned = earned,
                total_xp = new_total,
                level = new_level,
                "Sweep: XP updated for user."
            );

            // 5. Level-up detection
            if new_level > old_level {
                info!(
                    user_id = user.id,
                    discord_user_id = user.discord_user_id,
                    old_level = old_level,
                    new_level = new_level,
                    total_xp = new_total,
                    "Level up! User advanced from level {} to level {}.",
                    old_level,
                    new_level
                );
            }
        }
    }

    Ok(())
}

/// Get the discord stat value that was current at or before a given timestamp.
async fn get_discord_value_at_time(
    pool: &SqlitePool,
    user_id: i64,
    stat_name: &str,
    timestamp: &str,
) -> Option<f64> {
    sqlx::query_scalar::<_, f64>(
        "SELECT stat_value FROM discord_stats_snapshot
         WHERE user_id = ? AND stat_name = ? AND timestamp <= ?
         ORDER BY timestamp DESC
         LIMIT 1",
    )
    .bind(user_id)
    .bind(stat_name)
    .bind(timestamp)
    .fetch_optional(pool)
    .await
    .ok()?
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
