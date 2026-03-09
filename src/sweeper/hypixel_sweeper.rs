use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::{debug, error, info, warn};

use std::sync::Arc;
use std::time::Duration;

use crate::config::{AppConfig, GuildConfig};
use crate::database::models::DbUser;
use crate::database::queries;
use crate::hypixel::client::HypixelClient;
use crate::milestones;
use crate::shared::types::StatDelta;
use crate::xp::{XPConfig, calculator};

// =============================================================
// On-demand refresh (used by commands)
// =============================================================

pub async fn refresh_hypixel_user(
    pool: &PgPool,
    hypixel: &Arc<HypixelClient>,
    user: &DbUser,
    config: &AppConfig,
) -> bool {
    let now = Utc::now();

    // Record activity
    if let Err(e) = queries::update_last_command_activity(pool, user.id, &now).await {
        warn!(
            user_id = user.id,
            error = %e,
            "refresh_hypixel_user_if_stale: failed to update last_command_activity."
        );
    }

    let cooldown = Duration::from_secs(config.hypixel_refresh_cooldown_seconds);

    let needs_refresh = match user.last_hypixel_refresh {
        None => true,
        Some(last) => {
            let elapsed = now.signed_duration_since(last);
            elapsed > chrono::Duration::from_std(cooldown).unwrap_or(chrono::Duration::seconds(60))
        }
    };

    if !needs_refresh {
        debug!(
            user_id = user.id,
            "Hypixel refresh skipped — cached data still fresh."
        );
        return false;
    }

    if let Err(e) = refresh_user(pool, hypixel, user, config).await {
        warn!(
            user_id = user.id,
            error = %e,
            "Hypixel refresh failed, using cached data."
        );
        return false;
    }

    info!(user_id = user.id, "Hypixel refresh completed.");

    true
}

// =============================================================
// Background sweeper
// =============================================================

pub async fn run_hypixel_stale_sweep(
    pool: &PgPool,
    hypixel: &Arc<HypixelClient>,
    config: &AppConfig,
) -> Result<()> {
    let cutoff = Utc::now() - chrono::Duration::hours(2);

    let users = queries::get_users_with_expired_hypixel_stats(pool, cutoff, 10).await?;

    if users.is_empty() {
        debug!("Hypixel sweep: no expired users.");
        return Ok(());
    }

    debug!("Hypixel sweep refreshing {} expired users.", users.len());

    for user in users {
        if let Err(e) = refresh_user(pool, hypixel, &user, config).await {
            warn!(
                user_id = user.id,
                error = %e,
                "Hypixel sweep: failed refreshing user."
            );
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}

// =============================================================
// Complete sweeper
// =============================================================

pub async fn run_full_hypixel_sweep(
    pool: &PgPool,
    hypixel: &Arc<HypixelClient>,
    config: &AppConfig,
) -> bool {
    let users = queries::get_all_registered_users(pool)
        .await
        .unwrap_or_default();

    for user in users {
        if let Err(e) = refresh_user(pool, hypixel, &user, config).await {
            warn!(
                user_id = user.id,
                error = %e,
                "Hypixel complete sweep: failed refreshing user."
            );
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    info!("All player sweep completed.");

    true
}

// =============================================================
// Core refresh logic
// =============================================================

async fn refresh_user(
    pool: &PgPool,
    hypixel: &Arc<HypixelClient>,
    user: &DbUser,
    config: &AppConfig,
) -> Result<()> {
    // Preemptively update the refresh timestamp to reduce the chance of
    // multiple concurrent sweepers refreshing the same user simultaneously.
    queries::update_last_hypixel_refresh(pool, user.id, &Utc::now()).await?;

    let player_data = hypixel.fetch_player(&user.minecraft_uuid).await?;

    let bw = &player_data.bedwars;

    // ---------------------------------------------------------
    // Rank persistence
    // ---------------------------------------------------------

    let rank_db_str = player_data.rank.as_db_str();
    let rank_plus = player_data.rank_plus_color.as_deref();

    let rank_changed = user.hypixel_rank.as_deref() != rank_db_str
        || user.hypixel_rank_plus_color.as_deref() != rank_plus;

    if rank_changed {
        if let Err(e) =
            queries::update_user_hypixel_rank(pool, user.id, rank_db_str, rank_plus).await
        {
            warn!(
                user_id = user.id,
                error = %e,
                "Failed updating Hypixel rank."
            );
        } else {
            debug!(
                user_id = user.id,
                rank = ?rank_db_str,
                "Updated Hypixel rank."
            );
        }
    }

    // ---------------------------------------------------------
    // Stat delta computation
    // ---------------------------------------------------------

    let now = Utc::now();

    let guild_config = load_guild_config(pool, user.guild_id).await;

    let mut deltas: Vec<StatDelta> = Vec::new();

    for stat_name in guild_config.xp_config.keys() {
        let new_value = match bw.stats.get(stat_name) {
            Some(&v) => v,
            None => continue,
        };

        let previous = queries::get_latest_hypixel_snapshot(pool, user.id, stat_name).await?;

        if previous.is_none() {
            queries::insert_hypixel_snapshot(pool, user.id, stat_name, new_value, now).await?;

            continue;
        }

        let old_value = previous.as_ref().map(|s| s.stat_value).unwrap_or(0.0);

        if new_value != old_value {
            queries::insert_hypixel_snapshot(pool, user.id, stat_name, new_value, now).await?;
        }

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

    // ---------------------------------------------------------
    // Apply XP pipeline
    // ---------------------------------------------------------

    apply_stat_deltas(
        pool,
        user,
        &guild_config,
        config,
        &deltas,
        &now,
        "hypixel",
        "Hypixel sweep",
    )
    .await?;

    // ---------------------------------------------------------
    // Update refresh timestamp
    // ---------------------------------------------------------

    if let Err(e) = queries::update_last_hypixel_refresh(pool, user.id, &now).await {
        warn!(
            user_id = user.id,
            error = %e,
            "Failed updating last_hypixel_refresh."
        );
    }

    Ok(())
}

// =============================================================
// Config loader
// =============================================================

async fn load_guild_config(pool: &PgPool, guild_id: i64) -> GuildConfig {
    match queries::get_guild(pool, guild_id).await {
        Ok(Some(guild)) => serde_json::from_value(guild.config_json.clone()).unwrap_or_default(),

        Ok(None) => GuildConfig::default(),

        Err(e) => {
            error!(
                guild_id,
                error = %e,
                "Failed loading guild config."
            );

            GuildConfig::default()
        }
    }
}

async fn apply_stat_deltas(
    pool: &PgPool,
    user: &DbUser,
    guild_config: &GuildConfig,
    config: &AppConfig,
    deltas: &[StatDelta],
    now: &DateTime<Utc>,
    source: &str,
    source_label: &str,
) -> Result<()> {
    let xp_cfg = XPConfig::new(guild_config.xp_config.clone());

    // Compute per-delta XP rewards up-front so we can do an early return
    // when there is truly nothing to do (no positive deltas with a
    // configured multiplier AND no cursor updates to flush).
    let rewards = calculator::calculate_xp_rewards(deltas, &xp_cfg);
    let total_earned: f64 = rewards.iter().map(|r| r.xp_earned).sum();

    // Filter to deltas that are positive — these get a stat_deltas row
    // regardless of whether they have an XP multiplier configured.
    let positive_deltas: Vec<&StatDelta> = deltas.iter().filter(|d| d.difference > 0.0).collect();

    if positive_deltas.is_empty() {
        return Ok(());
    }

    // Track whether a level-up occurred so we can fire the milestone hook
    // after the transaction commits (avoiding any DB access inside the tx).
    let mut level_up: Option<(i32, i32)> = None; // (old_level, new_level)

    let mut tx = pool.begin().await?;

    // Build a lookup of XPReward by stat_name so we can pair rewards with
    // their corresponding delta row in a single pass.
    let reward_by_stat: std::collections::HashMap<&str, &crate::xp::calculator::XPReward> =
        rewards.iter().map(|r| (r.stat_name.as_str(), r)).collect();

    for delta in &positive_deltas {
        let delta_id = queries::insert_stat_delta_in_tx(
            &mut tx,
            user.id,
            &delta.stat_name,
            delta.old_value,
            delta.new_value,
            delta.difference,
            source,
            now,
        )
        .await?;

        // Only write an xp_events row when a multiplier is configured for
        // this stat. Unknown stats are still recorded in stat_deltas for
        // auditability but award no XP.
        if let Some(reward) = reward_by_stat.get(delta.stat_name.as_str()) {
            queries::insert_xp_event_in_tx(
                &mut tx,
                user.id,
                &delta.stat_name,
                delta_id,
                reward.units as i32,
                reward.xp_per_unit,
                reward.xp_earned,
                now,
            )
            .await?;
        }
    }

    if total_earned > 0.0 {
        // Atomic XP increment protects against lost updates when multiple
        // sweeper loops process the same user concurrently.
        sqlx::query(
            "INSERT INTO xp (user_id, total_xp, last_updated)
         VALUES ($1, $2, $3)
         ON CONFLICT(user_id) DO UPDATE SET
             total_xp = xp.total_xp + excluded.total_xp,
             last_updated = excluded.last_updated",
        )
        .bind(user.id)
        .bind(total_earned)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let xp_row = sqlx::query_as::<_, crate::database::models::DbXP>(
            "SELECT * FROM xp WHERE user_id = $1",
        )
        .bind(user.id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| anyhow!("XP row missing after upsert for user {}", user.id))?;

        let old_level = xp_row.level;
        let new_level = calculator::calculate_level(
            xp_row.total_xp,
            config.base_level_xp,
            config.level_exponent,
        ) as i32;

        if new_level != old_level {
            sqlx::query("UPDATE xp SET level = $1, last_updated = $2 WHERE user_id = $3")
                .bind(new_level)
                .bind(now)
                .bind(user.id)
                .execute(&mut *tx)
                .await?;
        }

        debug!(
            user_id = user.id,
            earned = total_earned,
            total_xp = xp_row.total_xp,
            level = new_level,
            source = source_label,
            "{}: XP updated for user.",
            source_label
        );

        if new_level > old_level {
            debug!(
                user_id = user.id,
                discord_user_id = user.discord_user_id,
                old_level,
                new_level,
                total_xp = xp_row.total_xp,
                source = source_label,
                "{}: level up detected.",
                source_label
            );
            level_up = Some((old_level, new_level));
        }
    }

    tx.commit().await?;

    // Milestone hook
    // Runs outside the transaction so a hook failure never rolls back XP.
    // The hook itself is currently a no-op but exists as an extension point.
    if let Some((old_level, new_level)) = level_up {
        let milestones = queries::get_milestones(pool, user.guild_id)
            .await
            .unwrap_or_default();

        for m in &milestones {
            // Fire for every milestone threshold crossed in this level-up.
            if m.level > old_level && m.level <= new_level {
                debug!(
                    user_id = user.id,
                    discord_user_id = user.discord_user_id,
                    milestone_level = m.level,
                    "Milestone reached — calling handle_milestone_reached."
                );
                milestones::handle_milestone_reached(user.discord_user_id as u64, m.level).await;
            }
        }
    }

    Ok(())
}
