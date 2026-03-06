/// Background stat sweepers.
///
/// This module contains source-specific sweeps:
/// - `run_hypixel_sweep` for Hypixel Bedwars polling
/// - `run_discord_sweep` for Discord activity XP processing
///
/// Both sweepers emit `StatDelta` values and pass them into the same XP update
/// pipeline so XP math and level progression stay centralized.
use std::sync::Arc;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::{debug, error, info, warn};

use crate::config::{AppConfig, GuildConfig};
use crate::database::models::{DbUser, DbXP};
use crate::database::queries;
use crate::hypixel::client::HypixelClient;
use crate::milestones;
use crate::shared::types::StatDelta;
use crate::stats_definitions::is_discord_stat;
use crate::xp::XPConfig;
use crate::xp::calculator;

const DISCORD_SOURCE: &str = "discord";

#[derive(Debug, Clone)]
struct CursorUpdate {
    source: &'static str,
    stat_name: String,
    stat_value: f64,
    last_snapshot_ts: DateTime<Utc>,
}

/// Run a single Hypixel sweep iteration for all registered users.
pub async fn run_hypixel_sweep(
    pool: &PgPool,
    hypixel: &Arc<HypixelClient>,
    config: &AppConfig,
) -> Result<()> {
    let users = queries::get_all_registered_users(pool).await?;

    if users.is_empty() {
        debug!("Hypixel sweep: no registered users, skipping.");
        return Ok(());
    }

    debug!(
        "Hypixel sweep: processing {} registered user(s)...",
        users.len()
    );

    for user in &users {
        if let Err(e) = sweep_hypixel_user(pool, hypixel, user, config).await {
            warn!(
                user_id = user.id,
                discord_user_id = user.discord_user_id,
                error = %e,
                "Hypixel sweep: failed to process user, skipping."
            );
        }
    }

    info!("Hypixel sweep: iteration complete.");
    Ok(())
}

/// Run a single Discord sweep iteration for all registered users.
pub async fn run_discord_sweep(pool: &PgPool, config: &AppConfig) -> Result<()> {
    let users = queries::get_all_registered_users(pool).await?;

    if users.is_empty() {
        debug!("Discord sweep: no registered users, skipping.");
        return Ok(());
    }

    debug!(
        "Discord sweep: processing {} registered user(s)...",
        users.len()
    );

    for user in &users {
        if let Err(e) = sweep_discord_user(pool, user, config).await {
            warn!(
                user_id = user.id,
                discord_user_id = user.discord_user_id,
                error = %e,
                "Discord sweep: failed to process user, skipping."
            );
        }
    }

    info!("Discord sweep: iteration complete.");
    Ok(())
}

/// Sweep one user's Hypixel stats.
async fn sweep_hypixel_user(
    pool: &PgPool,
    hypixel: &Arc<HypixelClient>,
    user: &DbUser,
    config: &AppConfig,
) -> Result<()> {
    // Fetch player data exactly once per user per Hypixel sweep.
    let player_data = hypixel.fetch_player(&user.minecraft_uuid).await?;
    let bw = &player_data.bedwars;

    let now = chrono::Utc::now();
    let guild_config = load_guild_config(pool, user.guild_id).await;

    let mut deltas: Vec<StatDelta> = Vec::new();

    for stat_name in guild_config.xp_config.keys() {
        if is_discord_stat(stat_name) {
            continue;
        }

        let new_value = match bw.stats.get(stat_name) {
            Some(&v) => v,
            None => continue,
        };

        let previous = queries::get_latest_hypixel_snapshot(pool, user.id, stat_name).await?;

        let time_now = chrono::Utc::now();
        // If this stat has no snapshots yet, seed a baseline and skip XP for now.
        if previous.is_none() {
            queries::insert_hypixel_snapshot(pool, user.id, stat_name, new_value, time_now).await?;
            continue;
        }

        let old_value = previous.as_ref().map(|s| s.stat_value).unwrap_or(0.0);

        queries::insert_hypixel_snapshot(pool, user.id, stat_name, new_value, time_now).await?;

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

    apply_stat_deltas(
        pool,
        user,
        &guild_config,
        config,
        &deltas,
        &[],
        &now,
        "hypixel",
        "Hypixel sweep",
    )
    .await
}

/// Sweep one user's Discord stats using cursor checkpoints.
async fn sweep_discord_user(pool: &PgPool, user: &DbUser, config: &AppConfig) -> Result<()> {
    let now = chrono::Utc::now();
    let guild_config = load_guild_config(pool, user.guild_id).await;
    let xp_row = queries::get_xp(pool, user.id).await?;

    let discord_stat_keys: Vec<String> = guild_config
        .xp_config
        .keys()
        .filter(|k| is_discord_stat(k))
        .cloned()
        .collect();

    if discord_stat_keys.is_empty() {
        return Ok(());
    }

    let mut deltas: Vec<StatDelta> = Vec::new();
    let mut cursor_updates: Vec<CursorUpdate> = Vec::new();

    for stat_name in discord_stat_keys {
        let latest = match queries::get_latest_discord_snapshot(pool, user.id, &stat_name).await? {
            Some(snap) => snap,
            None => continue,
        };

        let old_value = match queries::get_sweep_cursor(pool, user.id, DISCORD_SOURCE, &stat_name)
            .await?
        {
            Some(cursor) => cursor.stat_value,
            None => bootstrap_discord_old_value(pool, user.id, &stat_name, xp_row.as_ref()).await,
        };

        let diff = latest.stat_value - old_value;
        if diff > f64::EPSILON {
            deltas.push(StatDelta::new(
                user.id,
                stat_name.clone(),
                old_value,
                latest.stat_value,
            ));
        }

        cursor_updates.push(CursorUpdate {
            source: DISCORD_SOURCE,
            stat_name,
            stat_value: latest.stat_value,
            last_snapshot_ts: latest.timestamp,
        });
    }

    apply_stat_deltas(
        pool,
        user,
        &guild_config,
        config,
        &deltas,
        &cursor_updates,
        &now,
        "discord",
        "Discord sweep",
    )
    .await
}

/// Shared XP pipeline used by both source-specific sweepers.
///
/// For every positive-delta stat:
///   1. Inserts a `stat_deltas` row (permanent audit record).
///   2. If the stat has a configured multiplier, inserts an `xp_events` row
///      containing the multiplier value *at this point in time*. This means
///      historical XP is never affected by later admin edits to guild config.
///   3. Accumulates the total XP earned and atomically increments
///      `xp.total_xp` once at the end of the transaction.
///
/// Negative or zero deltas produce no rows in either audit table.
///
/// `source` is a short machine-readable tag stored in `stat_deltas.source`
/// (e.g. `"hypixel"`, `"discord"`).
/// `source_label` is a human-readable string used only in log messages.
async fn apply_stat_deltas(
    pool: &PgPool,
    user: &DbUser,
    guild_config: &GuildConfig,
    config: &AppConfig,
    deltas: &[StatDelta],
    cursor_updates: &[CursorUpdate],
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
    let positive_deltas: Vec<&StatDelta> = deltas
        .iter()
        .filter(|d| d.difference > 0.0)
        .collect();

    if positive_deltas.is_empty() && cursor_updates.is_empty() {
        return Ok(());
    }

    // Track whether a level-up occurred so we can fire the milestone hook
    // after the transaction commits (avoiding any DB access inside the tx).
    let mut level_up: Option<(i32, i32)> = None; // (old_level, new_level)

    let mut tx = pool.begin().await?;

    // ------------------------------------------------------------------
    // 1. Write stat_deltas + xp_events for every positive delta.
    // ------------------------------------------------------------------
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

    // ------------------------------------------------------------------
    // 2. Atomically increment xp.total_xp and recalculate level.
    // ------------------------------------------------------------------
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

    // ------------------------------------------------------------------
    // 3. Flush cursor updates (Discord sweeper only).
    // ------------------------------------------------------------------
    for cursor in cursor_updates {
        queries::upsert_sweep_cursor_in_tx(
            &mut tx,
            user.id,
            cursor.source,
            &cursor.stat_name,
            cursor.stat_value,
            &cursor.last_snapshot_ts,
            now,
        )
        .await?;
    }

    tx.commit().await?;

    // === Milestone hook =====================================================
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

/// Bootstrap policy for Discord cursor initialization.
///
/// If the cursor is missing, use the snapshot value at or before the user's
/// current XP `last_updated` timestamp. This preserves existing rollout
/// semantics and avoids retroactive XP spikes.
async fn bootstrap_discord_old_value(
    pool: &PgPool,
    user_id: i64,
    stat_name: &str,
    xp_row: Option<&DbXP>,
) -> f64 {
    let Some(xp) = xp_row else {
        return 0.0;
    };

    get_discord_value_at_time(pool, user_id, stat_name, &xp.last_updated)
        .await
        .unwrap_or(0.0)
}

/// Get the Discord stat value that was current at or before a given timestamp.
async fn get_discord_value_at_time(
    pool: &PgPool,
    user_id: i64,
    stat_name: &str,
    timestamp: &DateTime<Utc>,
) -> Option<f64> {
    sqlx::query_scalar::<_, f64>(
        "SELECT stat_value FROM discord_stats_snapshot
         WHERE user_id = $1 AND stat_name = $2 AND timestamp <= $3
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
async fn load_guild_config(pool: &PgPool, guild_id: i64) -> GuildConfig {
    match queries::get_guild(pool, guild_id).await {
        Ok(Some(guild)) => serde_json::from_value(guild.config_json.clone()).unwrap_or_default(),
        Ok(None) => GuildConfig::default(),
        Err(e) => {
            error!(guild_id, error = %e, "Failed to load guild config, using defaults.");
            GuildConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::*;
    use crate::database;

    fn test_app_config() -> AppConfig {
        AppConfig {
            discord_token: "test".to_string(),
            hypixel_api_key: "test".to_string(),
            database_url: "sqlite:test.db".to_string(),
            hypixel_sweep_interval_seconds: 60,
            discord_sweep_interval_seconds: 15,
            base_level_xp: 100.0,
            level_exponent: 1.5,
            admin_user_ids: Vec::new(),
            leaderboard_cache_seconds: 60,
            persistent_leaderboard_players: 10,
            min_message_length: 5,
            message_cooldown_seconds: 30,
        }
    }

    async fn test_pool() -> PgPool {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let db_path = format!("target/test-sweeper-{}.db", nanos);
        let _ = std::fs::remove_file(&db_path);
        database::init_db(&format!("sqlite:{}", db_path))
            .await
            .expect("test db should initialize")
    }

    async fn setup_user_with_discord_xp_stat(pool: &PgPool) -> DbUser {
        let guild_id = 42_i64;
        queries::upsert_guild(pool, guild_id)
            .await
            .expect("guild should be upserted");

        let mut guild_cfg = GuildConfig::default();
        guild_cfg.xp_config = HashMap::new();
        guild_cfg.xp_config.insert("messages_sent".to_string(), 1.0);
        queries::update_guild_config(
            pool,
            guild_id,
            serde_json::to_value(guild_cfg.clone()).expect("guild config should serialize"),
        )
        .await
        .expect("guild config should update");

        let fake_time = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

        queries::register_user(pool, 1001, Uuid::new_v4(), "player", guild_id, fake_time)
            .await
            .expect("user should register")
    }

    #[tokio::test]
    async fn discord_sweep_bootstraps_from_last_xp_timestamp() {
        let pool = test_pool().await;
        let config = test_app_config();
        let user = setup_user_with_discord_xp_stat(&pool).await;

        let fake_time = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

        queries::insert_discord_snapshot(&pool, user.id, "messages_sent", 0.0, fake_time)
            .await
            .expect("baseline discord snapshot should insert");
        queries::insert_discord_snapshot(
            &pool,
            user.id,
            "messages_sent",
            5.0,
            fake_time + chrono::Duration::minutes(5),
        )
        .await
        .expect("intermediate discord snapshot should insert");
        queries::insert_discord_snapshot(
            &pool,
            user.id,
            "messages_sent",
            9.0,
            fake_time + chrono::Duration::minutes(10),
        )
        .await
        .expect("latest discord snapshot should insert");

        // Cursor is intentionally absent. First run should bootstrap from this
        // timestamp and award only the 5 -> 9 delta.
        queries::set_xp_and_level(&pool, user.id, 10.0, 1, &(fake_time + chrono::Duration::minutes(7)))
            .await
            .expect("xp row should seed");

        run_discord_sweep(&pool, &config)
            .await
            .expect("discord sweep should run");

        let xp = queries::get_xp(&pool, user.id)
            .await
            .expect("xp query should succeed")
            .expect("xp row should exist");
        assert_eq!(xp.total_xp, 14.0);

        let cursor = queries::get_sweep_cursor(&pool, user.id, DISCORD_SOURCE, "messages_sent")
            .await
            .expect("cursor query should succeed")
            .expect("cursor should be created");
        assert_eq!(cursor.stat_value, 9.0);
    }

    #[tokio::test]
    async fn apply_stat_deltas_is_atomic_under_concurrent_writes() {
        let pool = test_pool().await;
        let config = test_app_config();
        let user = setup_user_with_discord_xp_stat(&pool).await;

        let mut guild_cfg = GuildConfig::default();
        guild_cfg.xp_config = HashMap::new();
        guild_cfg.xp_config.insert("messages_sent".to_string(), 1.0);

        let deltas_a = vec![StatDelta::new(
            user.id,
            "messages_sent".to_string(),
            0.0,
            3.0,
        )];
        let deltas_b = vec![StatDelta::new(
            user.id,
            "messages_sent".to_string(),
            0.0,
            7.0,
        )];

        let binding = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 10, 0).unwrap();
        let fut_a = apply_stat_deltas(
            &pool,
            &user,
            &guild_cfg,
            &config,
            &deltas_a,
            &[],
            &binding,
            "test",
            "Test A",
        );
        let binding = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 10, 0).unwrap();
        let fut_b = apply_stat_deltas(
            &pool,
            &user,
            &guild_cfg,
            &config,
            &deltas_b,
            &[],
            &binding,
            "test",
            "Test B",
        );

        let (res_a, res_b) = tokio::join!(fut_a, fut_b);
        res_a.expect("first concurrent update should succeed");
        res_b.expect("second concurrent update should succeed");

        let xp = queries::get_xp(&pool, user.id)
            .await
            .expect("xp query should succeed")
            .expect("xp row should exist");
        assert_eq!(xp.total_xp, 10.0);
    }

    /// Verify that `apply_stat_deltas` writes one `stat_deltas` row per
    /// positive delta and that all fields are stored correctly.
    #[tokio::test]
    async fn apply_stat_deltas_writes_stat_delta_rows() {
        let pool = test_pool().await;
        let config = test_app_config();
        let user = setup_user_with_discord_xp_stat(&pool).await;

        let mut guild_cfg = GuildConfig::default();
        guild_cfg.xp_config = HashMap::new();
        guild_cfg.xp_config.insert("messages_sent".to_string(), 2.0);

        let now = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let deltas = vec![StatDelta::new(
            user.id,
            "messages_sent".to_string(),
            10.0,
            15.0,
        )];

        apply_stat_deltas(
            &pool,
            &user,
            &guild_cfg,
            &config,
            &deltas,
            &[],
            &now,
            "test",
            "Test",
        )
        .await
        .expect("apply_stat_deltas should succeed");

        // Verify stat_deltas row
        let rows = sqlx::query_as::<_, crate::database::models::DbStatDelta>(
            "SELECT * FROM stat_deltas WHERE user_id = $1",
        )
        .bind(user.id)
        .fetch_all(&pool)
        .await
        .expect("stat_deltas query should succeed");

        assert_eq!(rows.len(), 1, "expected one stat_deltas row");
        let row = &rows[0];
        assert_eq!(row.stat_name, "messages_sent");
        assert_eq!(row.old_value, 10.0);
        assert_eq!(row.new_value, 15.0);
        assert_eq!(row.delta, 5.0);
        assert_eq!(row.source, "test");
    }

    /// Verify that `apply_stat_deltas` writes one `xp_events` row per
    /// positive delta that has a configured multiplier, and that the
    /// multiplier value captured in the row is the one active at sweep time.
    #[tokio::test]
    async fn apply_stat_deltas_writes_xp_event_with_correct_multiplier() {
        let pool = test_pool().await;
        let config = test_app_config();
        let user = setup_user_with_discord_xp_stat(&pool).await;

        let multiplier = 3.0_f64;
        let mut guild_cfg = GuildConfig::default();
        guild_cfg.xp_config = HashMap::new();
        guild_cfg.xp_config.insert("messages_sent".to_string(), multiplier);

        let now = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        // delta = 4 units → 4 * 3.0 = 12 XP
        let deltas = vec![StatDelta::new(
            user.id,
            "messages_sent".to_string(),
            0.0,
            4.0,
        )];

        apply_stat_deltas(
            &pool,
            &user,
            &guild_cfg,
            &config,
            &deltas,
            &[],
            &now,
            "test",
            "Test",
        )
        .await
        .expect("apply_stat_deltas should succeed");

        // Verify xp_events row
        let events = sqlx::query_as::<_, crate::database::models::DbXPEvent>(
            "SELECT * FROM xp_events WHERE user_id = $1",
        )
        .bind(user.id)
        .fetch_all(&pool)
        .await
        .expect("xp_events query should succeed");

        assert_eq!(events.len(), 1, "expected one xp_events row");
        let ev = &events[0];
        assert_eq!(ev.stat_name, "messages_sent");
        assert_eq!(ev.units, 4);
        assert_eq!(ev.xp_per_unit, multiplier);
        assert_eq!(ev.xp_earned, 12.0);

        // Verify delta_id FK links back to the stat_deltas row
        let delta_rows = sqlx::query_as::<_, crate::database::models::DbStatDelta>(
            "SELECT * FROM stat_deltas WHERE id = $1",
        )
        .bind(ev.delta_id)
        .fetch_all(&pool)
        .await
        .expect("stat_deltas FK query should succeed");
        assert_eq!(delta_rows.len(), 1, "xp_events.delta_id should reference a stat_deltas row");

        // Verify xp.total_xp was also updated
        let xp = queries::get_xp(&pool, user.id)
            .await
            .expect("xp query should succeed")
            .expect("xp row should exist");
        assert_eq!(xp.total_xp, 12.0);
    }

    /// Verify that `apply_stat_deltas` recalculates and stores the correct
    /// level in `xp.level` when earned XP crosses a level threshold.
    ///
    /// Config: base_level_xp = 100, exponent = 1.5  →  level 2 requires 100 XP.
    /// Awarding 100 units of `messages_sent` at 1 XP/unit = 100 XP total
    /// should advance the user from level 1 to level 2.
    #[tokio::test]
    async fn apply_stat_deltas_level_recalculates_after_xp() {
        let pool = test_pool().await;
        let config = test_app_config(); // base_level_xp=100, level_exponent=1.5
        let user = setup_user_with_discord_xp_stat(&pool).await;

        let mut guild_cfg = GuildConfig::default();
        guild_cfg.xp_config = HashMap::new();
        guild_cfg.xp_config.insert("messages_sent".to_string(), 1.0);

        let now = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        // 100 units * 1 XP/unit = 100 XP, which is exactly the threshold for level 2.
        let deltas = vec![StatDelta::new(
            user.id,
            "messages_sent".to_string(),
            0.0,
            100.0,
        )];

        apply_stat_deltas(
            &pool,
            &user,
            &guild_cfg,
            &config,
            &deltas,
            &[],
            &now,
            "test",
            "Test",
        )
        .await
        .expect("apply_stat_deltas should succeed");

        let xp = queries::get_xp(&pool, user.id)
            .await
            .expect("xp query should succeed")
            .expect("xp row should exist");

        assert_eq!(xp.total_xp, 100.0, "total_xp should be 100");
        assert_eq!(xp.level, 2, "level should advance to 2 after earning 100 XP");
    }
}
