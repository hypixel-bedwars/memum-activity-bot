use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
/// Database query functions.
///
/// All functions accept a `&PgPool` so they can be called from any context
/// that has access to the shared `Data` struct. Queries are organized by table.
///
/// Some functions are not yet called but exist as part of the public query API
/// for extensions and future commands.
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::collections::HashMap;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::database::models::{EventMessageRequirementDetail, RequirementStatus};

use super::models::{
    BackfillSummary, DbDailySnapshot, DbEvent, DbEventMilestone, DbEventStat, DbEventStatusMessage,
    DbGuild, DbMilestone, DbPersistentEventLeaderboard, DbPersistentLeaderboard, DbStatDelta,
    DbStatsSnapshot, DbSweepCursor, DbUser, DbVcSession, DbXP, EventLeaderboardEntry,
    EventMilestoneWithCount, EventParticipant, LeaderboardEntry, MilestoneWithCount,
};

// =========================================================================
// guilds
// =========================================================================

/// Insert a guild row if it does not already exist. If the guild already exists,
/// this is a no-op (the existing row is preserved).
pub async fn upsert_guild(pool: &PgPool, guild_id: i64) -> Result<(), sqlx::Error> {
    debug!("queries::upsert_guild: guild_id={}", guild_id);
    sqlx::query("INSERT INTO guilds (guild_id) VALUES ($1) ON CONFLICT(guild_id) DO NOTHING")
        .bind(guild_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set or clear the logging channel configured for this guild.
///
/// Pass `Some(channel_id)` to set the channel, or `None` to clear it.
/// This updates the `guilds.log_channel_id` column.
pub async fn set_guild_log_channel(
    pool: &PgPool,
    guild_id: i64,
    channel_id: Option<i64>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::set_guild_log_channel: guild_id={}, channel_id={:?}",
        guild_id, channel_id
    );
    sqlx::query("UPDATE guilds SET log_channel_id = $1 WHERE guild_id = $2")
        .bind(channel_id)
        .bind(guild_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Retrieve all guild IDs that have a logging channel configured.
///
/// Used by background sweepers to broadcast log messages to every guild that
/// has opted in to logging. Prefer [`get_all_guild_log_channels`] when the
/// channel ID is also needed (avoids a follow-up per-guild query).
pub async fn get_guilds_with_log_channel(pool: &PgPool) -> Result<Vec<i64>, sqlx::Error> {
    debug!("queries::get_guilds_with_log_channel");

    let rows: Vec<(i64,)> =
        sqlx::query_as("SELECT guild_id FROM guilds WHERE log_channel_id IS NOT NULL")
            .fetch_all(pool)
            .await?;

    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Retrieve all `(guild_id, channel_id)` pairs for guilds that have a logging
/// channel configured.
///
/// Preferred over [`get_guilds_with_log_channel`] when the channel ID is
/// needed immediately, because it fetches both in a single query (no N+1).
/// Used by the Discord log worker in `logging.rs` and by `broadcast_log` in
/// `daily_snapshot.rs`.
pub async fn get_all_guild_log_channels(pool: &PgPool) -> Result<Vec<(i64, i64)>, sqlx::Error> {
    debug!("queries::get_all_guild_log_channels");

    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT guild_id, log_channel_id FROM guilds WHERE log_channel_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn get_guild_log_channel(
    pool: &PgPool,
    guild_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    debug!("queries::get_guild_log_channel: guild_id={}", guild_id);

    // Reuse the existing DbGuild mapping so we don't need an additional SQL mapping.
    let guild_row: Option<super::models::DbGuild> =
        sqlx::query_as("SELECT * FROM guilds WHERE guild_id = $1")
            .bind(guild_id)
            .fetch_optional(pool)
            .await?;

    Ok(guild_row.and_then(|g| g.log_channel_id))
}

/// Retrieve a guild row by its Discord snowflake.
pub async fn get_guild(pool: &PgPool, guild_id: i64) -> Result<Option<DbGuild>, sqlx::Error> {
    debug!("queries::get_guild: guild_id={}", guild_id);
    sqlx::query_as::<_, DbGuild>("SELECT * FROM guilds WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_optional(pool)
        .await
}

/// Update the `config_json` column for a guild.
pub async fn update_guild_config(
    pool: &PgPool,
    guild_id: i64,
    config_json: Value,
) -> Result<(), sqlx::Error> {
    debug!("queries::update_guild_config: guild_id={}", guild_id);

    sqlx::query("UPDATE guilds SET config_json = $1 WHERE guild_id = $2")
        .bind(config_json)
        .bind(guild_id)
        .execute(pool)
        .await?;

    Ok(())
}

// =========================================================================
// users
// =========================================================================

/// Register a new user. Uses `ON CONFLICT` to update the Minecraft UUID and
/// username if the user re-registers in the same guild.
pub async fn register_user(
    pool: &PgPool,
    discord_user_id: i64,
    minecraft_uuid: Uuid,
    minecraft_username: &str,
    guild_id: i64,
    registered_at: DateTime<Utc>,
) -> Result<DbUser, sqlx::Error> {
    debug!(
        "queries::register_user: discord_user_id={}, minecraft_uuid={}, minecraft_username={}, guild_id={}, registered_at={}",
        discord_user_id, minecraft_uuid, minecraft_username, guild_id, registered_at
    );
    sqlx::query(
        "INSERT INTO users (discord_user_id, minecraft_uuid, minecraft_username, guild_id, registered_at)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT(discord_user_id, guild_id) DO UPDATE SET
             minecraft_uuid     = excluded.minecraft_uuid,
             minecraft_username = excluded.minecraft_username,
             active             = TRUE,
             left_at            = NULL",
    )
    .bind(discord_user_id)
    .bind(minecraft_uuid)
    .bind(minecraft_username)
    .bind(guild_id)
    .bind(registered_at)
    .execute(pool)
    .await?;

    // Return the (possibly updated) row.
    sqlx::query_as::<_, DbUser>("SELECT * FROM users WHERE discord_user_id = $1 AND guild_id = $2")
        .bind(discord_user_id)
        .bind(guild_id)
        .fetch_one(pool)
        .await
}

/// Get the most recent Hypixel stat snapshot for a user with a timestamp
/// strictly before `before_ts`. Used to compute "since last sweep" deltas.
pub async fn get_hypixel_snapshot_before(
    pool: &PgPool,
    user_id: i64,
    stat_name: &str,
    before_ts: &DateTime<Utc>,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    debug!(
        "queries::get_hypixel_snapshot_before: user_id={}, stat_name={}, before_ts={}",
        user_id, stat_name, before_ts
    );
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM hypixel_stats_snapshot
         WHERE user_id = $1 AND stat_name = $2 AND timestamp < $3
         ORDER BY timestamp DESC
         LIMIT 1",
    )
    .bind(user_id)
    .bind(stat_name)
    .bind(before_ts)
    .fetch_optional(pool)
    .await
}

/// Get the most recent Discord stat snapshot for a user with a timestamp
/// strictly before `before_ts`. Used to compute "since last sweep" deltas.
pub async fn get_discord_snapshot_before(
    pool: &PgPool,
    user_id: i64,
    stat_name: &str,
    before_ts: &DateTime<Utc>,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    debug!(
        "queries::get_discord_snapshot_before: user_id={}, stat_name={}, before_ts={}",
        user_id, stat_name, before_ts
    );
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM discord_stats_snapshot
         WHERE user_id = $1 AND stat_name = $2 AND timestamp < $3
         ORDER BY timestamp DESC
         LIMIT 1",
    )
    .bind(user_id)
    .bind(stat_name)
    .bind(before_ts)
    .fetch_optional(pool)
    .await
}

/// Get the earliest (registration-time) Hypixel stat snapshot for a user.
pub async fn get_first_hypixel_snapshot(
    pool: &PgPool,
    user_id: i64,
    stat_name: &str,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    debug!(
        "queries::get_first_hypixel_snapshot: user_id={}, stat_name={}",
        user_id, stat_name
    );
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM hypixel_stats_snapshot
         WHERE user_id = $1 AND stat_name = $2
         ORDER BY timestamp ASC
         LIMIT 1",
    )
    .bind(user_id)
    .bind(stat_name)
    .fetch_optional(pool)
    .await
}

/// Get the earliest (registration-time) Discord stat snapshot for a user.
pub async fn get_first_discord_snapshot(
    pool: &PgPool,
    user_id: i64,
    stat_name: &str,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    debug!(
        "queries::get_first_discord_snapshot: user_id={}, stat_name={}",
        user_id, stat_name
    );
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM discord_stats_snapshot
         WHERE user_id = $1 AND stat_name = $2
         ORDER BY timestamp ASC
         LIMIT 1",
    )
    .bind(user_id)
    .bind(stat_name)
    .fetch_optional(pool)
    .await
}

pub async fn wipe_user_stats(pool: &PgPool, user_id: i64) -> Result<(), sqlx::Error> {
    debug!(user_id, "Wiping all user stats");
    let mut tx = pool.begin().await?;

    // Delete in order (children → parents to avoid FK issues)
    sqlx::query(
        r#"
        DELETE FROM stat_deltas              WHERE user_id = $1;
        DELETE FROM xp_events                WHERE user_id = $1;
        DELETE FROM event_xp                 WHERE user_id = $1;
        DELETE FROM daily_snapshots          WHERE user_id = $1;
        DELETE FROM sweep_cursor             WHERE user_id = $1;
        DELETE FROM hypixel_stats_snapshot   WHERE user_id = $1;
        DELETE FROM discord_stats_snapshot   WHERE user_id = $1;
        DELETE FROM xp                       WHERE user_id = $1;
        "#,
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(())
}

/// Look up an *active* user by Discord ID within a specific guild.
///
/// Inactive users are preserved for historical stats, but should be ignored by
/// tracking and most commands.
pub async fn get_user_by_discord_id(
    pool: &PgPool,
    discord_user_id: i64,
    guild_id: i64,
) -> Result<Option<DbUser>, sqlx::Error> {
    debug!(
        "queries::get_user_by_discord_id: discord_user_id={}, guild_id={}",
        discord_user_id, guild_id
    );
    sqlx::query_as::<_, DbUser>(
        "SELECT * FROM users
         WHERE discord_user_id = $1
           AND guild_id = $2
           AND active = TRUE",
    )
    .bind(discord_user_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await
}

/// Like `get_user_by_discord_id` but returns the row regardless of the
/// `active` flag.  Used during registration to detect inactive (previously
/// unregistered) users so they can be reactivated instead of treated as
/// brand-new registrations.
pub async fn get_user_by_discord_id_any(
    pool: &PgPool,
    discord_user_id: i64,
    guild_id: i64,
) -> Result<Option<DbUser>, sqlx::Error> {
    debug!(
        "queries::get_user_by_discord_id_any: discord_user_id={}, guild_id={}",
        discord_user_id, guild_id
    );
    sqlx::query_as::<_, DbUser>(
        "SELECT * FROM users
         WHERE discord_user_id = $1
           AND guild_id = $2",
    )
    .bind(discord_user_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await
}

/// Get all registered users across every guild. Used by the sweeper.
pub async fn get_all_registered_users(pool: &PgPool) -> Result<Vec<DbUser>, sqlx::Error> {
    debug!("queries::get_all_registered_users");
    sqlx::query_as::<_, DbUser>("SELECT * FROM users")
        .fetch_all(pool)
        .await
}

/// Get all registered users, sorted so that recently active users come first.
///
/// "Active" is defined as having a `last_command_activity` timestamp on or
/// after `activity_cutoff`.  Within each group (active / inactive) users are
/// ordered by `last_hypixel_refresh` ascending (least-recently-refreshed
/// first) so that stale data is prioritised within each tier.
///
/// Used by the Hypixel background sweeper.
pub async fn get_users_prioritized_for_hypixel_sweep(
    pool: &PgPool,
    activity_cutoff: DateTime<Utc>,
) -> Result<Vec<DbUser>, sqlx::Error> {
    debug!(
        "queries::get_users_prioritized_for_hypixel_sweep: activity_cutoff={}",
        activity_cutoff
    );
    sqlx::query_as::<_, DbUser>(
        "SELECT * FROM users
         ORDER BY
             CASE WHEN last_command_activity >= $1 THEN 0 ELSE 1 END ASC,
             COALESCE(last_hypixel_refresh, '1970-01-01 00:00:00+00') ASC",
    )
    .bind(activity_cutoff)
    .fetch_all(pool)
    .await
}

/// Record that a Hypixel API fetch completed successfully for this user.
///
/// Called by `sweep_hypixel_user` after every successful API round-trip so
/// that the cooldown check in commands has an accurate timestamp to compare
/// against.
pub async fn update_last_hypixel_refresh(
    pool: &PgPool,
    user_id: i64,
    timestamp: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::update_last_hypixel_refresh: user_id={}, timestamp={}",
        user_id, timestamp
    );
    sqlx::query("UPDATE users SET last_hypixel_refresh = $1 WHERE id = $2")
        .bind(timestamp)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Update the Hypixel rank and rank-plus-colour for a user.
///
/// Called from `sweep_hypixel_user` after every successful API fetch so that
/// rank data is always kept in sync with the live Hypixel response.
pub async fn update_user_hypixel_rank(
    pool: &PgPool,
    user_id: i64,
    rank: Option<&str>,
    rank_plus_color: Option<&str>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::update_user_hypixel_rank: user_id={}, rank={:?}, rank_plus_color={:?}",
        user_id, rank, rank_plus_color
    );
    sqlx::query(
        "UPDATE users
         SET hypixel_rank = $1, hypixel_rank_plus_color = $2
         WHERE id = $3",
    )
    .bind(rank)
    .bind(rank_plus_color)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record that the user invoked a stat-related command right now.
///
/// Called at the start of `/level` and `/stats` so that the background
/// sweeper can identify recently active users and prioritise their refresh
/// slot in the next sweep cycle.
pub async fn update_last_command_activity(
    pool: &PgPool,
    user_id: i64,
    timestamp: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::update_last_command_activity: user_id={}, timestamp={}",
        user_id, timestamp
    );
    sqlx::query("UPDATE users SET last_command_activity = $1 WHERE id = $2")
        .bind(timestamp)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Store cached head texture for a user (head_texture is a base64 PNG or data URL).
pub async fn set_user_head_texture(
    pool: &PgPool,
    user_id: i64,
    head_texture: &str,
    updated_at: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::set_user_head_texture: user_id={}, head_texture_len={}, updated_at={}",
        user_id,
        head_texture.len(),
        updated_at
    );
    sqlx::query("UPDATE users SET head_texture = $1, head_texture_updated_at = $2 WHERE id = $3")
        .bind(head_texture)
        .bind(updated_at)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Unregister a user.
///
/// This project uses **soft unregister** via `mark_user_inactive` to preserve
/// historical stats and avoid foreign-key constraint issues.
///
/// This legacy hard-delete function is kept for admin/maintenance use only.
/// Prefer `mark_user_inactive` for user-facing flows.
///
/// Deletes a user and all related data via ON DELETE CASCADE. This will remove:
/// - hypixel_stats_snapshot
/// - discord_stats_snapshot
/// - xp
/// - sweep_cursor
/// - stat_deltas
/// - xp_events
/// - daily_snapshots
/// - event_xp
pub async fn unregister_user(
    pool: &PgPool,
    discord_user_id: i64,
    guild_id: i64,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::unregister_user: discord_user_id={}, guild_id={}",
        discord_user_id, guild_id
    );

    // Find the user row to get the internal user id
    let user: Option<DbUser> =
        sqlx::query_as("SELECT * FROM users WHERE discord_user_id = $1 AND guild_id = $2")
            .bind(discord_user_id)
            .bind(guild_id)
            .fetch_optional(pool)
            .await?;

    let Some(user) = user else {
        warn!(
            discord_user_id,
            guild_id, "Attempted to unregister user but no matching user found"
        );
        return Ok(());
    };

    // Just delete the user; all dependent rows will be deleted via ON DELETE CASCADE
    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user.id)
        .execute(pool)
        .await?;

    let rows_deleted = result.rows_affected();
    info!(
        discord_user_id,
        guild_id,
        user_id = user.id,
        minecraft_uuid = %user.minecraft_uuid,
        rows_deleted,
        "User unregistered successfully - all related data cascaded"
    );

    Ok(())
}

/// Get all registered users within a specific guild.
pub async fn get_all_users_in_guild(
    pool: &PgPool,
    guild_id: i64,
) -> Result<Vec<DbUser>, sqlx::Error> {
    debug!("queries::get_all_users_in_guild: guild_id={}", guild_id);
    sqlx::query_as::<_, DbUser>("SELECT * FROM users WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_all(pool)
        .await
}

/// Get the rank of a user within their guild, based on total XP. Returns `None` if the user is not registered or has no XP record.
/// Inactive users are excluded so ranks reflect only active members.
pub async fn get_user_rank_in_guild(
    pool: &PgPool,
    user_id: i64,
    guild_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    debug!(
        "queries::get_user_rank_in_guild: user_id={}, guild_id={}",
        user_id, guild_id
    );
    sqlx::query_scalar::<_, i64>(
        "SELECT rank FROM (
             SELECT u.id AS user_id, RANK() OVER (ORDER BY COALESCE(x.total_xp, 0) DESC) AS rank
             FROM users u
             LEFT JOIN xp x ON x.user_id = u.id
             WHERE u.guild_id = $1 AND u.active = TRUE
         ) sub
         WHERE user_id = $2",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

// =========================================================================
// hypixel_stats_snapshot
// =========================================================================

/// Insert a new Hypixel stat snapshot row.
pub async fn insert_hypixel_snapshot(
    pool: &PgPool,
    user_id: i64,
    stat_name: &str,
    stat_value: i64,
    timestamp: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::insert_hypixel_snapshot: user_id={}, stat_name={}, stat_value={}, timestamp={}",
        user_id, stat_name, stat_value, timestamp
    );
    sqlx::query(
        "INSERT INTO hypixel_stats_snapshot (user_id, stat_name, stat_value, timestamp)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(stat_name)
    .bind(stat_value)
    .bind(timestamp)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get the most recent snapshot value for a given user and stat name.
pub async fn get_latest_hypixel_snapshot(
    pool: &PgPool,
    user_id: i64,
    stat_name: &str,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    debug!(
        "queries::get_latest_hypixel_snapshot: user_id={}, stat_name={}",
        user_id, stat_name
    );
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM hypixel_stats_snapshot
         WHERE user_id = $1 AND stat_name = $2
         ORDER BY timestamp DESC
         LIMIT 1",
    )
    .bind(user_id)
    .bind(stat_name)
    .fetch_optional(pool)
    .await
}

/// Get all latest Hypixel snapshots for a user (one per stat name).
pub async fn get_latest_hypixel_snapshots_for_user(
    pool: &PgPool,
    user_id: i64,
) -> Result<Vec<DbStatsSnapshot>, sqlx::Error> {
    debug!(
        "queries::get_latest_hypixel_snapshots_for_user: user_id={}",
        user_id
    );

    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT DISTINCT ON (stat_name) *
         FROM hypixel_stats_snapshot
         WHERE user_id = $1
         ORDER BY stat_name, timestamp DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

// =========================================================================
// discord_stats_snapshot
// =========================================================================

/// Insert a new Discord stat snapshot row.
pub async fn insert_discord_snapshot(
    pool: &PgPool,
    user_id: i64,
    stat_name: &str,
    stat_value: i64,
    timestamp: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::insert_discord_snapshot: user_id={}, stat_name={}, stat_value={}, timestamp={}",
        user_id, stat_name, stat_value, timestamp
    );
    sqlx::query(
        "INSERT INTO discord_stats_snapshot (user_id, stat_name, stat_value, timestamp)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(stat_name)
    .bind(stat_value)
    .bind(timestamp)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get the most recent Discord stat snapshot for a given user and stat name.
pub async fn get_latest_discord_snapshot(
    pool: &PgPool,
    user_id: i64,
    stat_name: &str,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    debug!(
        "queries::get_latest_discord_snapshot: user_id={}, stat_name={}",
        user_id, stat_name
    );
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM discord_stats_snapshot
         WHERE user_id = $1 AND stat_name = $2
         ORDER BY timestamp DESC
         LIMIT 1",
    )
    .bind(user_id)
    .bind(stat_name)
    .fetch_optional(pool)
    .await
}

pub async fn get_latest_discord_snapshots_for_user(
    pool: &PgPool,
    user_id: i64,
) -> Result<Vec<DbStatsSnapshot>, sqlx::Error> {
    debug!(
        "queries::get_latest_discord_snapshots_for_user: user_id={}",
        user_id
    );

    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT DISTINCT ON (stat_name) *
         FROM discord_stats_snapshot
         WHERE user_id = $1
         ORDER BY stat_name, timestamp DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

// =========================================================================
// xp
// =========================================================================

/// Set the XP total and level for a user.
///
/// # Test-only seeding
///
/// This is an **absolute setter** — it replaces `total_xp` and `level`
/// entirely rather than incrementing them.  It must **not** be called from
/// production code; use the `apply_stat_deltas` pipeline in
/// `src/sweeper/stat_sweeper.rs` instead.  This function exists solely to
/// seed deterministic state in integration tests.
pub async fn set_xp_and_level(
    pool: &PgPool,
    user_id: i64,
    total_xp: f64,
    level: i32,
    timestamp: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::set_xp_and_level: user_id={}, total_xp={}, level={}, timestamp={}",
        user_id, total_xp, level, timestamp
    );
    sqlx::query(
        "INSERT INTO xp (user_id, total_xp, level, last_updated)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT(user_id) DO UPDATE SET
             total_xp = excluded.total_xp,
             level = excluded.level,
             last_updated = excluded.last_updated",
    )
    .bind(user_id)
    .bind(total_xp)
    .bind(level)
    .bind(timestamp)
    .execute(pool)
    .await?;
    Ok(())
}

/// Retrieve current XP for a user, if they exist.
pub async fn get_xp(pool: &PgPool, user_id: i64) -> Result<Option<DbXP>, sqlx::Error> {
    debug!("queries::get_xp: user_id={}", user_id);
    sqlx::query_as::<_, DbXP>("SELECT * FROM xp WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
}

/// Delete a user's XP record (used when unregistering).
pub async fn delete_xp(pool: &PgPool, user_id: i64) -> Result<(), sqlx::Error> {
    debug!("queries::delete_xp: user_id={}", user_id);
    sqlx::query("DELETE FROM xp WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Increment a user's total XP by the given amount.
///
/// If the user has no XP record yet, creates one with total_xp = amount and level = 1.
pub async fn increment_xp(
    pool: &PgPool,
    user_id: i64,
    amount: f64,
    timestamp: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::increment_xp: user_id={}, amount={}, timestamp={}",
        user_id, amount, timestamp
    );
    sqlx::query(
        "INSERT INTO xp (user_id, total_xp, level, last_updated)
         VALUES ($1, $2, 1, $3)
         ON CONFLICT(user_id) DO UPDATE SET
             total_xp = xp.total_xp + excluded.total_xp,
             last_updated = excluded.last_updated",
    )
    .bind(user_id)
    .bind(amount)
    .bind(timestamp)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update a user's level and last_updated timestamp.
pub async fn update_level(
    pool: &PgPool,
    user_id: i64,
    level: i32,
    timestamp: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::update_level: user_id={}, level={}, timestamp={}",
        user_id, level, timestamp
    );
    sqlx::query("UPDATE xp SET level = $1, last_updated = $2 WHERE user_id = $3")
        .bind(level)
        .bind(timestamp)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

// =========================================================================
// sweep_cursor
// =========================================================================

/// Get the current sweep cursor for a given user/source/stat tuple.
pub async fn get_sweep_cursor(
    pool: &PgPool,
    user_id: i64,
    source: &str,
    stat_name: &str,
) -> Result<Option<DbSweepCursor>, sqlx::Error> {
    debug!(
        "queries::get_sweep_cursor: user_id={}, source={}, stat_name={}",
        user_id, source, stat_name
    );
    sqlx::query_as::<_, DbSweepCursor>(
        "SELECT * FROM sweep_cursor
         WHERE user_id = $1 AND source = $2 AND stat_name = $3",
    )
    .bind(user_id)
    .bind(source)
    .bind(stat_name)
    .fetch_optional(pool)
    .await
}

/// Insert or update a sweep cursor row.
pub async fn upsert_sweep_cursor(
    pool: &PgPool,
    user_id: i64,
    source: &str,
    stat_name: &str,
    stat_value: i64,
    last_snapshot_ts: &DateTime<Utc>,
    updated_at: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::upsert_sweep_cursor: user_id={}, source={}, stat_name={}, stat_value={}, last_snapshot_ts={}, updated_at={}",
        user_id, source, stat_name, stat_value, last_snapshot_ts, updated_at
    );
    sqlx::query(
        "INSERT INTO sweep_cursor (user_id, source, stat_name, stat_value, last_snapshot_ts, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT(user_id, source, stat_name) DO UPDATE SET
             stat_value = excluded.stat_value,
             last_snapshot_ts = excluded.last_snapshot_ts,
             updated_at = excluded.updated_at",
    )
    .bind(user_id)
    .bind(source)
    .bind(stat_name)
    .bind(stat_value)
    .bind(last_snapshot_ts)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert or update a sweep cursor row inside an existing SQL transaction.
pub async fn upsert_sweep_cursor_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    source: &str,
    stat_name: &str,
    stat_value: i64,
    last_snapshot_ts: &DateTime<Utc>,
    updated_at: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::upsert_sweep_cursor_in_tx: user_id={}, source={}, stat_name={}, stat_value={}, last_snapshot_ts={}, updated_at={}",
        user_id, source, stat_name, stat_value, last_snapshot_ts, updated_at
    );
    sqlx::query(
        "INSERT INTO sweep_cursor (user_id, source, stat_name, stat_value, last_snapshot_ts, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT(user_id, source, stat_name) DO UPDATE SET
             stat_value = excluded.stat_value,
             last_snapshot_ts = excluded.last_snapshot_ts,
             updated_at = excluded.updated_at",
    )
    .bind(user_id)
    .bind(source)
    .bind(stat_name)
    .bind(stat_value)
    .bind(last_snapshot_ts)
    .bind(updated_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// =========================================================================
// leaderboard
// =========================================================================

/// Get the top N players in a guild, ranked by total XP descending.
///
/// `offset` is the number of rows to skip (for pagination).
/// `limit` is the number of rows to return per page.
pub async fn get_leaderboard(
    pool: &PgPool,
    guild_id: i64,
    offset: i64,
    limit: i64,
) -> Result<Vec<LeaderboardEntry>, sqlx::Error> {
    debug!(
        "queries::get_leaderboard: guild_id={}, offset={}, limit={}",
        guild_id, offset, limit
    );
    sqlx::query_as::<_, LeaderboardEntry>(
        "SELECT u.discord_user_id,
                u.minecraft_username,
                u.minecraft_uuid,
                COALESCE(x.total_xp, 0.0) AS total_xp,
                COALESCE(x.level, 1) AS level,
                u.hypixel_rank,
                u.hypixel_rank_plus_color
         FROM users u
         LEFT JOIN xp x ON x.user_id = u.id
         WHERE u.guild_id = $1
           AND u.active = TRUE
         ORDER BY total_xp DESC, u.id ASC
         LIMIT $2 OFFSET $3",
    )
    .bind(guild_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

/// Count the total number of registered users in a guild (for pagination math).
pub async fn count_users_in_guild(pool: &PgPool, guild_id: i64) -> Result<i64, sqlx::Error> {
    debug!("queries::count_users_in_guild: guild_id={}", guild_id);
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
    FROM users
    WHERE guild_id = $1 AND active = TRUE",
    )
    .bind(guild_id)
    .fetch_one(pool)
    .await
}

// =========================================================================
// persistent_leaderboards
// =========================================================================

/// Insert or update a persistent leaderboard entry for a guild.
pub async fn upsert_persistent_leaderboard(
    pool: &PgPool,
    guild_id: i64,
    channel_id: i64,
    message_ids: &serde_json::Value,
    status_message_id: i64,
    milestone_message_id: i64,
    created_at: &DateTime<Utc>,
    last_updated: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::upsert_persistent_leaderboard: guild_id={}, channel_id={}, status_message_id={}, milestone_message_id={}, created_at={}, last_updated={}",
        guild_id, channel_id, status_message_id, milestone_message_id, created_at, last_updated
    );

    sqlx::query(
        "INSERT INTO persistent_leaderboards
        (guild_id, channel_id, message_ids, status_message_id, milestone_message_id, created_at, last_updated)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT(guild_id) DO UPDATE SET
            channel_id = excluded.channel_id,
            message_ids = excluded.message_ids,
            status_message_id = excluded.status_message_id,
            milestone_message_id = excluded.milestone_message_id,
            created_at = excluded.created_at,
            last_updated = excluded.last_updated",
    )
    .bind(guild_id)
    .bind(channel_id)
    .bind(message_ids)
    .bind(status_message_id)
    .bind(milestone_message_id)
    .bind(created_at)
    .bind(last_updated)
    .execute(pool)
    .await?;

    Ok(())
}

/// Retrieve the persistent leaderboard row for a guild, if one exists.
pub async fn get_persistent_leaderboard(
    pool: &PgPool,
    guild_id: i64,
) -> Result<Option<DbPersistentLeaderboard>, sqlx::Error> {
    debug!("queries::get_persistent_leaderboard: guild_id={}", guild_id);
    sqlx::query_as::<_, DbPersistentLeaderboard>(
        "SELECT * FROM persistent_leaderboards WHERE guild_id = $1",
    )
    .bind(guild_id)
    .fetch_optional(pool)
    .await
}

/// Delete the persistent leaderboard row for a guild.
pub async fn delete_persistent_leaderboard(
    pool: &PgPool,
    guild_id: i64,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::delete_persistent_leaderboard: guild_id={}",
        guild_id
    );
    sqlx::query("DELETE FROM persistent_leaderboards WHERE guild_id = $1")
        .bind(guild_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Get all persistent leaderboard rows (used by the updater background task).
pub async fn get_all_persistent_leaderboards(
    pool: &PgPool,
) -> Result<Vec<DbPersistentLeaderboard>, sqlx::Error> {
    debug!("queries::get_all_persistent_leaderboards");
    sqlx::query_as::<_, DbPersistentLeaderboard>("SELECT * FROM persistent_leaderboards")
        .fetch_all(pool)
        .await
}

/// Update message IDs and last_updated for a persistent leaderboard.
pub async fn update_persistent_leaderboard_messages(
    pool: &PgPool,
    guild_id: i64,
    message_ids: &serde_json::Value,
    last_updated: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::update_persistent_leaderboard_messages: guild_id={}, message_ids_len={}, last_updated={}",
        guild_id,
        message_ids.as_array().map(|a| a.len()).unwrap_or(0),
        last_updated
    );
    sqlx::query(
        "UPDATE persistent_leaderboards
         SET message_ids = $1, last_updated = $2
         WHERE guild_id = $3",
    )
    .bind(message_ids)
    .bind(last_updated)
    .bind(guild_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update only the milestone_message_id for a persistent leaderboard.
pub async fn update_persistent_leaderboard_milestone_message(
    pool: &PgPool,
    guild_id: i64,
    milestone_message_id: i64,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::update_persistent_leaderboard_milestone_message: guild_id={}, milestone_message_id={}",
        guild_id, milestone_message_id
    );
    sqlx::query(
        "UPDATE persistent_leaderboards
         SET milestone_message_id = $1
         WHERE guild_id = $2",
    )
    .bind(milestone_message_id)
    .bind(guild_id)
    .execute(pool)
    .await?;
    Ok(())
}

// =========================================================================
// milestones
// =========================================================================

/// Insert a new milestone for a guild.
///
/// Returns `Ok(true)` if the milestone was created, `Ok(false)` if a
/// milestone at that level already exists for the guild (no-op).
pub async fn add_milestone(pool: &PgPool, guild_id: i64, level: i32) -> Result<bool, sqlx::Error> {
    debug!(
        "queries::add_milestone: guild_id={}, level={}",
        guild_id, level
    );
    let rows_affected = sqlx::query(
        "INSERT INTO milestones (guild_id, level) VALUES ($1, $2)
         ON CONFLICT(guild_id, level) DO NOTHING",
    )
    .bind(guild_id)
    .bind(level)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rows_affected > 0)
}

/// Update the level of an existing milestone identified by its ID.
///
/// Returns `Ok(true)` if the update succeeded, `Ok(false)` if the milestone
/// was not found or the new level conflicts with an existing one.
pub async fn edit_milestone(
    pool: &PgPool,
    guild_id: i64,
    milestone_id: i64,
    new_level: i32,
) -> Result<bool, sqlx::Error> {
    debug!(
        "queries::edit_milestone: guild_id={}, milestone_id={}, new_level={}",
        guild_id, milestone_id, new_level
    );
    let rows_affected =
        sqlx::query("UPDATE milestones SET level = $1 WHERE id = $2 AND guild_id = $3")
            .bind(new_level)
            .bind(milestone_id)
            .bind(guild_id)
            .execute(pool)
            .await?
            .rows_affected();
    Ok(rows_affected > 0)
}

/// Delete a milestone by its ID within a guild.
///
/// Returns `Ok(true)` if the row was deleted, `Ok(false)` if it was not found.
pub async fn remove_milestone(
    pool: &PgPool,
    guild_id: i64,
    milestone_id: i64,
) -> Result<bool, sqlx::Error> {
    debug!(
        "queries::remove_milestone: guild_id={}, milestone_id={}",
        guild_id, milestone_id
    );
    let rows_affected = sqlx::query("DELETE FROM milestones WHERE id = $1 AND guild_id = $2")
        .bind(milestone_id)
        .bind(guild_id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(rows_affected > 0)
}

/// Retrieve all milestones for a guild, ordered by level ascending.
pub async fn get_milestones(pool: &PgPool, guild_id: i64) -> Result<Vec<DbMilestone>, sqlx::Error> {
    debug!("queries::get_milestones: guild_id={}", guild_id);
    sqlx::query_as::<_, DbMilestone>(
        "SELECT id, guild_id, level FROM milestones
         WHERE guild_id = $1
         ORDER BY level ASC",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await
}

// =========================================================================
// stat_deltas
// =========================================================================

/// Insert a stat delta row inside an existing transaction and return its
/// auto-generated `id`. The returned id must be passed to
/// `insert_xp_event_in_tx` so the XP event can reference this row.
pub async fn insert_stat_delta_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    stat_name: &str,
    old_value: i64,
    new_value: i64,
    delta: i64,
    source: &str,
    created_at: &DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    debug!(
        "queries::insert_stat_delta_in_tx: user_id={}, stat_name={}, delta={}, source={}",
        user_id, stat_name, delta, source
    );
    let row = sqlx::query_as::<_, DbStatDelta>(
        "INSERT INTO stat_deltas (user_id, stat_name, old_value, new_value, delta, source, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(user_id)
    .bind(stat_name)
    .bind(old_value)
    .bind(new_value)
    .bind(delta)
    .bind(source)
    .bind(created_at)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.id)
}

/// Insert a stat delta row and return its ID.
pub async fn insert_stat_delta(
    pool: &PgPool,
    user_id: i64,
    stat_name: &str,
    old_value: i64,
    new_value: i64,
    delta: i64,
    source: &str,
    created_at: &DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    debug!(
        "queries::insert_stat_delta: user_id={}, stat_name={}, delta={}, source={}",
        user_id, stat_name, delta, source
    );
    let row = sqlx::query_as::<_, DbStatDelta>(
        "INSERT INTO stat_deltas (user_id, stat_name, old_value, new_value, delta, source, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(user_id)
    .bind(stat_name)
    .bind(old_value)
    .bind(new_value)
    .bind(delta)
    .bind(source)
    .bind(created_at)
    .fetch_one(pool)
    .await?;
    Ok(row.id)
}

// =========================================================================
// xp_events
// =========================================================================

/// Insert an XP event row inside an existing transaction.
///
/// `delta_id` must reference a row that was already inserted in the same
/// transaction via `insert_stat_delta_in_tx`. The `xp_per_unit` value must
/// be the multiplier that was active at sweep time so that historical XP
/// is never affected by later admin edits to guild config.
pub async fn insert_xp_event_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    stat_name: &str,
    delta_id: i64,
    units: i32,
    xp_per_unit: f64,
    xp_earned: f64,
    created_at: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::insert_xp_event_in_tx: user_id={}, stat_name={}, delta_id={}, units={}, xp_per_unit={}, xp_earned={}",
        user_id, stat_name, delta_id, units, xp_per_unit, xp_earned
    );
    sqlx::query(
        "INSERT INTO xp_events (user_id, stat_name, delta_id, units, xp_per_unit, xp_earned, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(user_id)
    .bind(stat_name)
    .bind(delta_id)
    .bind(units)
    .bind(xp_per_unit)
    .bind(xp_earned)
    .bind(created_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Retrieve all milestones for a guild together with the count of users who
/// have reached each milestone level.
///
/// The count is the number of users in the guild whose current level is
/// greater than or equal to the milestone level.
pub async fn get_milestones_with_counts(
    pool: &PgPool,
    guild_id: i64,
) -> Result<Vec<MilestoneWithCount>, sqlx::Error> {
    debug!("queries::get_milestones_with_counts: guild_id={}", guild_id);
    sqlx::query_as::<_, MilestoneWithCount>(
        "SELECT m.id, m.guild_id, m.level,
                COUNT(x.user_id) AS user_count
         FROM milestones m
         LEFT JOIN users u ON u.guild_id = m.guild_id AND u.active = TRUE
         LEFT JOIN xp x ON x.user_id = u.id AND x.level >= m.level
         WHERE m.guild_id = $1
         GROUP BY m.id
         ORDER BY m.level ASC",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await
}

pub async fn get_users_with_expired_hypixel_stats(
    pool: &PgPool,
    cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<DbUser>, sqlx::Error> {
    debug!(
        "queries::get_users_with_expired_hypixel_stats: cutoff={}, limit={}",
        cutoff, limit
    );

    sqlx::query_as::<_, DbUser>(
        "SELECT *
         FROM users
         WHERE COALESCE(last_hypixel_refresh, 'epoch') < $1
         ORDER BY last_hypixel_refresh ASC NULLS FIRST
         LIMIT $2",
    )
    .bind(cutoff)
    .bind(limit)
    .fetch_all(pool)
    .await
}

// ========================================================================
// Daily snapshots
// =======================================================================

/// Insert daily snapshots for an explicit UTC date, avoiding any dependency on
/// the database server's session timezone. This is the preferred variant used
/// by the scheduled snapshot loop.
pub async fn insert_daily_snapshot_for_date(
    pool: &PgPool,
    date: NaiveDate,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO daily_snapshots (user_id, stat_name, stat_value, snapshot_date)
        SELECT user_id, stat_name, stat_value, $1::date
        FROM (
            SELECT DISTINCT ON (user_id, stat_name)
                user_id,
                stat_name,
                stat_value
            FROM hypixel_stats_snapshot
            ORDER BY user_id, stat_name, timestamp DESC
        ) latest
        ON CONFLICT (user_id, stat_name, snapshot_date) DO NOTHING
        "#,
    )
    .bind(date)
    .execute(pool)
    .await?;

    // Snapshot Discord stats
    sqlx::query(
        r#"
        INSERT INTO daily_snapshots (user_id, stat_name, stat_value, snapshot_date)
        SELECT user_id, stat_name, stat_value, $1::date
        FROM (
            SELECT DISTINCT ON (user_id, stat_name)
                user_id,
                stat_name,
                stat_value
            FROM discord_stats_snapshot
            ORDER BY user_id, stat_name, timestamp DESC
        ) latest
        ON CONFLICT (user_id, stat_name, snapshot_date) DO NOTHING
        "#,
    )
    .bind(date)
    .execute(pool)
    .await?;

    Ok(())
}

/// Convenience wrapper that computes today's UTC date and inserts snapshots.
/// Prefer `insert_daily_snapshot_for_date` when you already have the date.
pub async fn insert_daily_snapshot(pool: &PgPool) -> Result<(), sqlx::Error> {
    let date = chrono::Utc::now().date_naive();
    insert_daily_snapshot_for_date(pool, date).await
}

pub async fn get_daily_snapshot(
    pool: &PgPool,
    user_id: i64,
    date: NaiveDate,
) -> Result<Vec<DbDailySnapshot>, sqlx::Error> {
    let rows = sqlx::query_as::<_, DbDailySnapshot>(
        r#"
        SELECT user_id, stat_name, stat_value, snapshot_date, created_at
        FROM daily_snapshots
        WHERE user_id = $1
        AND snapshot_date = $2
        "#,
    )
    .bind(user_id)
    .bind(date)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn get_stat_delta_between(
    pool: &PgPool,
    user_id: i64,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT
            e.stat_name,
            e.stat_value - s.stat_value AS delta
        FROM daily_snapshots s
        JOIN daily_snapshots e
            ON s.user_id = e.user_id
           AND s.stat_name = e.stat_name
        WHERE s.user_id = $1
        AND s.snapshot_date = $2
        AND e.snapshot_date = $3
        "#,
    )
    .bind(user_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

// =========================================================================
// events
// =========================================================================

/// Create a new event and immediately seed its `event_stats` from the guild's
/// current `xp_config`. Returns the newly inserted `DbEvent`.
pub async fn create_event(
    pool: &PgPool,
    guild_id: i64,
    name: &str,
    description: Option<&str>,
    start_date: &DateTime<Utc>,
    end_date: &DateTime<Utc>,
) -> Result<DbEvent, sqlx::Error> {
    debug!(
        "queries::create_event: guild_id={}, name={}, start={}, end={}",
        guild_id, name, start_date, end_date
    );

    let event: DbEvent = sqlx::query_as::<_, DbEvent>(
        "INSERT INTO events (guild_id, name, description, start_date, end_date)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(guild_id)
    .bind(name)
    .bind(description)
    .bind(start_date)
    .bind(end_date)
    .fetch_one(pool)
    .await?;

    Ok(event)
}

/// Retrieve a single event by its ID and guild.
pub async fn get_event(
    pool: &PgPool,
    guild_id: i64,
    event_id: i64,
) -> Result<Option<DbEvent>, sqlx::Error> {
    debug!(
        "queries::get_event: guild_id={}, event_id={}",
        guild_id, event_id
    );
    sqlx::query_as::<_, DbEvent>("SELECT * FROM events WHERE id = $1 AND guild_id = $2")
        .bind(event_id)
        .bind(guild_id)
        .fetch_optional(pool)
        .await
}

/// Retrieve a single event by its primary key.
pub async fn get_event_by_id(pool: &PgPool, event_id: i64) -> Result<Option<DbEvent>, sqlx::Error> {
    debug!("queries::get_event_by_id: event_id={}", event_id);
    sqlx::query_as::<_, DbEvent>("SELECT * FROM events WHERE id = $1")
        .bind(event_id)
        .fetch_optional(pool)
        .await
}

/// Retrieve a single event by name within a guild.
pub async fn get_event_by_name(
    pool: &PgPool,
    guild_id: i64,
    name: &str,
) -> Result<Option<DbEvent>, sqlx::Error> {
    debug!(
        "queries::get_event_by_name: guild_id={}, name={}",
        guild_id, name
    );
    sqlx::query_as::<_, DbEvent>("SELECT * FROM events WHERE guild_id = $1 AND name = $2")
        .bind(guild_id)
        .bind(name)
        .fetch_optional(pool)
        .await
}

/// List all events for a guild, ordered by start_date descending.
pub async fn list_events(pool: &PgPool, guild_id: i64) -> Result<Vec<DbEvent>, sqlx::Error> {
    debug!("queries::list_events: guild_id={}", guild_id);
    sqlx::query_as::<_, DbEvent>(
        "SELECT * FROM events WHERE guild_id = $1 ORDER BY start_date DESC",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await
}

/// List events for a guild filtered by status.
pub async fn list_events_by_status(
    pool: &PgPool,
    guild_id: i64,
    status: &str,
) -> Result<Vec<DbEvent>, sqlx::Error> {
    debug!(
        "queries::list_events_by_status: guild_id={}, status={}",
        guild_id, status
    );
    sqlx::query_as::<_, DbEvent>(
        "SELECT * FROM events WHERE guild_id = $1 AND status = $2 ORDER BY start_date DESC",
    )
    .bind(guild_id)
    .bind(status)
    .fetch_all(pool)
    .await
}

/// Count all registered users across guild.
///
/// Used by the event sweep scheduler to estimate how long a full sweep will
/// take (1 second per user) without fetching the full row set.
pub async fn count_registered_users(pool: &PgPool) -> Result<i64, sqlx::Error> {
    debug!("queries::count_registered_users (active flag)");
    // Count only users who are marked active. The migrations add an `active`
    // boolean column so users who left can be hidden without deleting history.
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE active = TRUE")
        .fetch_one(pool)
        .await
}

/// Return all pending events (across all guilds) whose start time is still in
/// the future. Ordered by `start_date` ascending so the soonest event is first.
///
/// Used by the event sweep scheduler to decide when to kick off a pre-start
/// full sweep.
pub async fn get_all_pending_events(pool: &PgPool) -> Result<Vec<DbEvent>, sqlx::Error> {
    debug!("queries::get_all_pending_events");
    sqlx::query_as::<_, DbEvent>(
        "SELECT * FROM events
         WHERE status = 'pending' AND start_date > NOW()
         ORDER BY start_date ASC",
    )
    .fetch_all(pool)
    .await
}

/// Return all active events (across all guilds) whose end time is still in
/// the future. Ordered by `end_date` ascending so the soonest-ending event is
/// first.
///
/// Used by the event sweep scheduler to decide when to kick off a pre-end
/// full sweep.
pub async fn get_all_active_events(pool: &PgPool) -> Result<Vec<DbEvent>, sqlx::Error> {
    debug!("queries::get_all_active_events");
    sqlx::query_as::<_, DbEvent>(
        "SELECT * FROM events
         WHERE status = 'active' AND end_date > NOW()
         ORDER BY end_date ASC",
    )
    .fetch_all(pool)
    .await
}

/// Update mutable fields of an event. Only fields with `Some(...)` are changed.
/// Returns `Ok(true)` if the update succeeded, `Ok(false)` if not found or ended.
pub async fn update_event(
    pool: &PgPool,
    guild_id: i64,
    event_id: i64,
    name: Option<&str>,
    description: Option<&str>,
    start_date: Option<&DateTime<Utc>>,
    end_date: Option<&DateTime<Utc>>,
) -> Result<bool, sqlx::Error> {
    debug!(
        "queries::update_event: guild_id={}, event_id={}",
        guild_id, event_id
    );
    let rows = sqlx::query(
        "UPDATE events
         SET name        = COALESCE($3, name),
             description = COALESCE($4, description),
             start_date  = COALESCE($5, start_date),
             end_date    = COALESCE($6, end_date),
             updated_at  = NOW()
         WHERE id = $1 AND guild_id = $2 AND status != 'ended'",
    )
    .bind(event_id)
    .bind(guild_id)
    .bind(name)
    .bind(description)
    .bind(start_date)
    .bind(end_date)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rows > 0)
}

/// Delete an event that has not yet ended.
/// Returns `Ok(true)` if deleted, `Ok(false)` if not found or already ended.
pub async fn delete_event(
    pool: &PgPool,
    guild_id: i64,
    event_id: i64,
) -> Result<bool, sqlx::Error> {
    debug!(
        "queries::delete_event: guild_id={}, event_id={}",
        guild_id, event_id
    );
    let rows =
        sqlx::query("DELETE FROM events WHERE id = $1 AND guild_id = $2 AND status != 'ended'")
            .bind(event_id)
            .bind(guild_id)
            .execute(pool)
            .await?
            .rows_affected();
    Ok(rows > 0)
}

/// Transition pending → active and active → ended events based on wall-clock time,
/// and set the appropriate snapshot dates.
pub async fn update_event_statuses(pool: &PgPool) -> Result<(), sqlx::Error> {
    debug!("queries::update_event_statuses");

    // pending → active
    sqlx::query(
        "UPDATE events
         SET status = 'active',
             start_snapshot_date = (
                 SELECT MAX(snapshot_date)
                 FROM daily_snapshots
                 WHERE snapshot_date <= start_date::date
             ),
             updated_at = NOW()
         WHERE status = 'pending' AND start_date <= NOW()",
    )
    .execute(pool)
    .await?;

    // active → ended
    sqlx::query(
        "UPDATE events
         SET status = 'ended',
             end_snapshot_date = (
                 SELECT MAX(snapshot_date)
                 FROM daily_snapshots
                 WHERE snapshot_date <= end_date::date
             ),
             updated_at = NOW()
         WHERE status = 'active' AND end_date <= NOW()",
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn force_end_event(pool: &PgPool, event_id: i64) -> Result<(), sqlx::Error> {
    debug!("queries::force_end_event: event_id={}", event_id);

    let result = sqlx::query(
        r#"
        UPDATE events
        SET status = 'ended',
            end_snapshot_date = (
                SELECT MAX(snapshot_date)
                FROM daily_snapshots
                WHERE snapshot_date <= NOW()::date
            ),
            updated_at = NOW()
        WHERE id = $1 AND status != 'ended'
        "#,
    )
    .bind(event_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        debug!("force_end_event: no rows updated (event may not exist or already ended)");
    }

    Ok(())
}

pub async fn force_start_event(pool: &PgPool, event_id: i64) -> Result<(), sqlx::Error> {
    debug!("queries::force_start_event: event_id={}", event_id);

    let result = sqlx::query(
        r#"
        UPDATE events
        SET status = 'active',
            start_snapshot_date = COALESCE(
                (
                    SELECT MAX(snapshot_date)
                    FROM daily_snapshots
                    WHERE snapshot_date <= NOW()::date
                ),
                NOW()::date
            ),
            updated_at = NOW()
        WHERE id = $1
          AND status = 'pending'
        "#,
    )
    .bind(event_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        debug!("force_start_event: no rows updated (event may not exist or not pending)");
    }

    Ok(())
}

pub async fn get_latest_event_name(
    pool: &PgPool,
    guild_id: i64,
) -> Result<Option<String>, sqlx::Error> {
    debug!("queries::get_latest_event_name: guild_id={}", guild_id);

    sqlx::query_scalar(
        "SELECT name
         FROM events
         WHERE guild_id = $1
           AND status IN ('active', 'ended')
         ORDER BY start_date DESC
         LIMIT 1",
    )
    .bind(guild_id)
    .fetch_optional(pool)
    .await
}

// =========================================================================
// event_stats
// =========================================================================

/// Add a stat to an event. Returns `Ok(true)` if inserted, `Ok(false)` if already exists.
pub async fn add_event_stat(
    pool: &PgPool,
    event_id: i64,
    stat_name: &str,
    xp_per_unit: f64,
) -> Result<bool, sqlx::Error> {
    debug!(
        "queries::add_event_stat: event_id={}, stat_name={}, xp_per_unit={}",
        event_id, stat_name, xp_per_unit
    );
    let rows = sqlx::query(
        "INSERT INTO event_stats (event_id, stat_name, xp_per_unit)
         VALUES ($1, $2, $3)
         ON CONFLICT (event_id, stat_name) DO NOTHING",
    )
    .bind(event_id)
    .bind(stat_name)
    .bind(xp_per_unit)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rows > 0)
}

/// Remove a stat from an event. Returns `Ok(true)` if removed.
pub async fn remove_event_stat(
    pool: &PgPool,
    event_id: i64,
    stat_name: &str,
) -> Result<bool, sqlx::Error> {
    debug!(
        "queries::remove_event_stat: event_id={}, stat_name={}",
        event_id, stat_name
    );
    let rows = sqlx::query("DELETE FROM event_stats WHERE event_id = $1 AND stat_name = $2")
        .bind(event_id)
        .bind(stat_name)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(rows > 0)
}

/// Update the XP-per-unit for a stat in an event. Returns `Ok(true)` if updated.
pub async fn edit_event_stat(
    pool: &PgPool,
    event_id: i64,
    stat_name: &str,
    xp_per_unit: f64,
) -> Result<bool, sqlx::Error> {
    debug!(
        "queries::edit_event_stat: event_id={}, stat_name={}, xp_per_unit={}",
        event_id, stat_name, xp_per_unit
    );
    let rows = sqlx::query(
        "UPDATE event_stats SET xp_per_unit = $3 WHERE event_id = $1 AND stat_name = $2",
    )
    .bind(event_id)
    .bind(stat_name)
    .bind(xp_per_unit)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rows > 0)
}

/// List all stats configured for an event.
pub async fn get_event_stats(
    pool: &PgPool,
    event_id: i64,
) -> Result<Vec<DbEventStat>, sqlx::Error> {
    debug!("queries::get_event_stats: event_id={}", event_id);
    sqlx::query_as::<_, DbEventStat>(
        "SELECT * FROM event_stats WHERE event_id = $1 ORDER BY stat_name",
    )
    .bind(event_id)
    .fetch_all(pool)
    .await
}

/// Seed event_stats for a new event from a guild's xp_config map.
/// Silently skips stats that already exist (ON CONFLICT DO NOTHING).
pub async fn seed_event_stats_from_xp_config(
    pool: &PgPool,
    event_id: i64,
    xp_config: &std::collections::HashMap<String, f64>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::seed_event_stats_from_xp_config: event_id={}, stats={}",
        event_id,
        xp_config.len()
    );
    for (stat_name, &xp_per_unit) in xp_config {
        sqlx::query(
            "INSERT INTO event_stats (event_id, stat_name, xp_per_unit)
             VALUES ($1, $2, $3)
             ON CONFLICT (event_id, stat_name) DO NOTHING",
        )
        .bind(event_id)
        .bind(stat_name)
        .bind(xp_per_unit)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn get_event_participants(
    pool: &PgPool,
    event_id: i64,
) -> Result<Vec<EventParticipant>, sqlx::Error> {
    debug!("queries::get_event_participants: event_id={}", event_id);

    sqlx::query_as::<_, EventParticipant>(
        "SELECT DISTINCT u.discord_user_id AS user_id,
               u.minecraft_username
        FROM event_xp ex
        JOIN users u ON u.id = ex.user_id
        WHERE ex.event_id = $1
          AND u.active = TRUE
                    AND is_player_allowed(u.id, $1) = TRUE
        ORDER BY u.minecraft_username NULLS LAST, u.discord_user_id",
    )
    .bind(event_id)
    .fetch_all(pool)
    .await
}

// =========================================================================
// event_xp
// =========================================================================

/// Called after every stat delta commit. Looks up all active events for the
/// guild that track `stat_name`, inserts an `event_xp` row for each, and
/// returns the total XP earned across all matching events (for the caller to
/// add to the user's `total_xp` if desired).
///
/// Uses `ON CONFLICT (event_id, delta_id) DO NOTHING` to guarantee idempotency.
pub async fn award_event_xp_for_delta(
    pool: &PgPool,
    guild_id: i64,
    user_id: i64,
    stat_name: &str,
    delta_id: i64,
    difference: i64,
    now: &DateTime<Utc>,
) -> Result<f64, sqlx::Error> {
    debug!(
        "queries::award_event_xp_for_delta: guild_id={}, user_id={}, stat_name={}, delta_id={}, difference={}",
        guild_id, user_id, stat_name, delta_id, difference
    );

    let units = difference;
    if units <= 0 {
        return Ok(0.0);
    }

    // Single guarded insert-select to enforce:
    // - event active for guild and stat
    // - user active
    // - not globally banned
    // - not disqualified for the event
    let rows: Vec<(f64,)> = sqlx::query_as(
        r#"
        INSERT INTO event_xp (event_id, user_id, stat_name, delta_id, units, xp_per_unit, xp_earned, created_at)
        SELECT es.event_id,
               $2 AS user_id,
               es.stat_name,
               $4 AS delta_id,
               $5 AS units,
               es.xp_per_unit,
               es.xp_per_unit * $5::DOUBLE PRECISION AS xp_earned,
               $6 AS created_at
        FROM event_stats es
        JOIN events e ON e.id = es.event_id
        JOIN users u ON u.id = $2 AND u.guild_id = $1
        WHERE e.guild_id = $1
          AND e.status = 'active'
          AND e.start_date <= $6
          AND e.end_date > $6
          AND es.stat_name = $3
          AND u.active = TRUE
          AND is_player_allowed($2, es.event_id) = TRUE
        ON CONFLICT (event_id, delta_id) DO NOTHING
        RETURNING xp_earned
        "#,
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(stat_name)
    .bind(delta_id)
    .bind(units)
    .bind(now)
    .fetch_all(pool)
    .await?;

    let total_xp: f64 = rows.into_iter().map(|(xp,)| xp).sum();
    Ok(total_xp)
}

/// Award admin XP to all active events for a guild.
///
/// This is used when admins manually add or remove XP via `/xp add` or `/xp remove`.
/// It adds the specified amount to the user's event XP for every active event in the guild.
pub async fn award_admin_event_xp(
    pool: &PgPool,
    guild_id: i64,
    user_id: i64,
    amount: i64,
    now: &DateTime<Utc>,
) -> Result<f64, sqlx::Error> {
    debug!(
        "queries::award_admin_event_xp: guild_id={}, user_id={}, amount={}",
        guild_id, user_id, amount
    );

    // Insert a stat_delta for this admin action
    let delta_id = insert_stat_delta(
        pool, user_id, "admin_xp", 0,      // old_value
        amount, // new_value
        amount, // delta
        "admin", now,
    )
    .await?;

    // Guarded insert-select across all active events for this guild where the user is eligible.
    let rows: Vec<(f64,)> = sqlx::query_as(
        r#"
        INSERT INTO event_xp (event_id, user_id, stat_name, delta_id, units, xp_per_unit, xp_earned, created_at)
        SELECT e.id,
               $2 AS user_id,
               $3 AS stat_name,
               $4 AS delta_id,
               1 AS units,
               $5::DOUBLE PRECISION AS xp_per_unit,
               $5::DOUBLE PRECISION AS xp_earned,
               $6 AS created_at
        FROM events e
        JOIN users u ON u.id = $2
        WHERE e.guild_id = $1
            AND e.status = 'active'
            AND u.active = TRUE
            AND is_player_allowed($2, e.id) = TRUE
        ON CONFLICT (event_id, delta_id) DO NOTHING
        RETURNING xp_earned
        "#,
    )
    .bind(guild_id)
    .bind(user_id)
    .bind("admin_xp")
    .bind(delta_id)
    .bind(amount as f64)
    .bind(now)
    .fetch_all(pool)
    .await?;

    let total_xp: f64 = rows.into_iter().map(|(xp,)| xp).sum();

    debug!("Awarded {} total event XP for admin action", total_xp);

    Ok(total_xp)
}

/// Return the event leaderboard for a given event (top N users by total event XP).
pub async fn get_event_leaderboard(
    pool: &PgPool,
    event_id: i64,
    limit: i64,
    offset: i64,
) -> Result<Vec<EventLeaderboardEntry>, sqlx::Error> {
    debug!(
        "queries::get_event_leaderboard: event_id={}, limit={}, offset={}",
        event_id, limit, offset
    );

    sqlx::query_as::<_, EventLeaderboardEntry>(
        r#"
        SELECT
            u.discord_user_id,
            u.minecraft_username,
            u.minecraft_uuid,
            u.hypixel_rank,
            u.hypixel_rank_plus_color,
            COALESCE(SUM(ex.xp_earned), 0.0) AS total_event_xp
        FROM event_xp ex
        JOIN users u ON u.id = ex.user_id
        JOIN events e ON e.id = ex.event_id
        WHERE ex.event_id = $1
            AND e.id = $1
            AND u.active = TRUE
            AND is_player_allowed(ex.user_id, $1) = TRUE
        GROUP BY
            u.discord_user_id,
            u.minecraft_username,
            u.minecraft_uuid,
            u.hypixel_rank,
            u.hypixel_rank_plus_color,
            u.id
        ORDER BY
            total_event_xp DESC,
            u.id ASC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(event_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

/// Return a user's total event XP and per-stat breakdown for a given event.
pub async fn get_user_event_stats(
    pool: &PgPool,
    event_id: i64,
    user_id: i64,
) -> Result<Vec<(String, f64, i64)>, sqlx::Error> {
    debug!(
        "queries::get_user_event_stats: event_id={}, user_id={}",
        event_id, user_id
    );

    // First, test without is_player_allowed to see if that's the issue
    let test_rows = sqlx::query_as::<_, (String, f64, i64)>(
        "SELECT ex.stat_name, COALESCE(SUM(ex.xp_earned), 0.0) AS total_xp, COALESCE(SUM(ex.units), 0)::BIGINT AS total_units
         FROM event_xp ex
         JOIN users u ON u.id = ex.user_id
         WHERE ex.event_id = $1
            AND ex.user_id = $2
            AND u.active = TRUE
         GROUP BY ex.stat_name
         ORDER BY total_xp DESC",
    )
    .bind(event_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    info!(
        "queries::get_user_event_stats (WITHOUT is_player_allowed): event_id={}, user_id={}, rows_returned={}, stats={:?}",
        event_id,
        user_id,
        test_rows.len(),
        test_rows
    );

    let rows = sqlx::query_as::<_, (String, f64, i64)>(
        "SELECT ex.stat_name, COALESCE(SUM(ex.xp_earned), 0.0) AS total_xp, COALESCE(SUM(ex.units), 0)::BIGINT AS total_units
         FROM event_xp ex
         JOIN users u ON u.id = ex.user_id
         WHERE ex.event_id = $1
            AND ex.user_id = $2
            AND u.active = TRUE
            AND is_player_allowed(ex.user_id, $1) = TRUE
         GROUP BY ex.stat_name
         ORDER BY total_xp DESC",
    )
    .bind(event_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    info!(
        "queries::get_user_event_stats (WITH is_player_allowed): event_id={}, user_id={}, rows_returned={}, stats={:?}",
        event_id,
        user_id,
        rows.len(),
        rows
    );

    Ok(rows)
}

/// Return the user's rank (1-indexed) within a specific event leaderboard,
/// ordered by total event XP descending.  Returns `None` if the user has no
/// XP recorded for this event.
pub async fn get_user_event_rank(
    pool: &PgPool,
    event_id: i64,
    user_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    debug!(
        "queries::get_user_event_rank: event_id={}, user_id={}",
        event_id, user_id
    );
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT rank FROM (
            SELECT ex.user_id,
            RANK() OVER (ORDER BY SUM(ex.xp_earned) DESC, ex.user_id ASC)
            FROM event_xp ex
            JOIN users u ON u.id = ex.user_id
            WHERE ex.event_id = $1
                AND u.active = TRUE
                AND is_player_allowed(ex.user_id, $1) = TRUE
            GROUP BY ex.user_id
        ) sub
        WHERE user_id = $2
        "#,
    )
    .bind(event_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// Mark a user as inactive (soft-delete) when they leave a guild.
///
/// Sets `active = FALSE`, records `left_at`, and updates `updated_at`.
/// Use this from your guild-member-remove handler so the user will be
/// excluded from leaderboards without losing historical data.
pub async fn mark_user_inactive(
    pool: &PgPool,
    discord_user_id: i64,
    guild_id: i64,
    left_at: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::mark_user_inactive: discord_user_id={}, guild_id={}, left_at={}",
        discord_user_id, guild_id, left_at
    );

    sqlx::query(
        "UPDATE users
         SET active = FALSE,
             left_at = $3
         WHERE discord_user_id = $1
           AND guild_id = $2",
    )
    .bind(discord_user_id)
    .bind(guild_id)
    .bind(left_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark a user as active (e.g. they rejoined).
pub async fn mark_user_active(
    pool: &PgPool,
    discord_user_id: i64,
    guild_id: i64,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::mark_user_active: discord_user_id={}, guild_id={}",
        discord_user_id, guild_id
    );

    sqlx::query(
        "UPDATE users
         SET active = TRUE,
             left_at = NULL
         WHERE discord_user_id = $1
           AND guild_id = $2",
    )
    .bind(discord_user_id)
    .bind(guild_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn disqualify_user_from_event(
    pool: &PgPool,
    event_id: i64,
    user_id: i64,
    moderator_id: i64,
    guild_id: i64,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::disqualify_user_from_event: event_id={}, user_id={}, moderator_id={}, guild_id={}",
        event_id, user_id, moderator_id, guild_id
    );

    sqlx::query(
        r#"
        INSERT INTO modrec (user_id, moderator_id, guild_id, action_type, event_id, reason)
        VALUES ($1, $2, $3, 'disqualify', $4, $5)
        "#,
    )
    .bind(user_id)
    .bind(moderator_id)
    .bind(guild_id)
    .bind(event_id)
    .bind(reason)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn requalify_user_for_event(
    pool: &PgPool,
    event_id: i64,
    user_id: i64,
    moderator_id: i64,
    guild_id: i64,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::requalify_user_for_event: event_id={}, user_id={}, moderator_id={}, guild_id={}",
        event_id, user_id, moderator_id, guild_id
    );

    sqlx::query(
        r#"
        INSERT INTO modrec (user_id, moderator_id, guild_id, action_type, event_id, reason)
        VALUES ($1, $2, $3, 'undisqualify', $4, $5)
        "#,
    )
    .bind(user_id)
    .bind(moderator_id)
    .bind(guild_id)
    .bind(event_id)
    .bind(reason)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn ban_user_from_events(
    pool: &PgPool,
    user_id: i64,
    moderator_id: i64,
    guild_id: i64,
    expires_at: Option<DateTime<Utc>>,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::ban_user_from_events: user_id={}, moderator_id={}, guild_id={}, expires_at={:?}",
        user_id, moderator_id, guild_id, expires_at
    );

    sqlx::query(
        r#"
        INSERT INTO modrec (user_id, moderator_id, guild_id, action_type, ban_expires_at, reason)
        VALUES ($1, $2, $3, 'ban', $4, $5)
        "#,
    )
    .bind(user_id)
    .bind(moderator_id)
    .bind(guild_id)
    .bind(expires_at)
    .bind(reason)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn unban_user_from_events(
    pool: &PgPool,
    user_id: i64,
    moderator_id: i64,
    guild_id: i64,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::unban_user_from_events: user_id={}, moderator_id={}, guild_id={}",
        user_id, moderator_id, guild_id
    );

    sqlx::query(
        r#"
        INSERT INTO modrec (user_id, moderator_id, guild_id, action_type, reason)
        VALUES ($1, $2, $3, 'unban', $4)
        "#,
    )
    .bind(user_id)
    .bind(moderator_id)
    .bind(guild_id)
    .bind(reason)
    .execute(pool)
    .await?;

    Ok(())
}

/// Get the total number of messages a user has sent during a specific event.
///
/// This sums the `units` column in `event_xp` for the given user and event
/// where the stat is "messages".
pub async fn get_event_user_message_count(
    pool: &PgPool,
    event_id: i64,
    user_id: i64,
) -> Result<i32, sqlx::Error> {
    debug!(
        "queries::get_event_user_message_count: event_id={}, user_id={}",
        event_id, user_id
    );

    // Summing units as i32. We use COALESCE to return 0 if the user hasn't sent any messages yet.
    let count: i32 = sqlx::query_scalar!(
        r#"
        SELECT COALESCE(SUM(units), 0)::INT
        FROM event_xp
        WHERE event_id = $1 
          AND user_id = $2 
          AND stat_name = 'messages_sent'
        "#,
        event_id,
        user_id
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0);

    Ok(count)
}

// =========================================================================
// persistent_event_leaderboards
// =========================================================================

/// Count participants (distinct users) for a given event.
pub async fn count_event_participants(pool: &PgPool, event_id: i64) -> Result<i64, sqlx::Error> {
    debug!("queries::count_event_participants: event_id={}", event_id);
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT ex.user_id)
        FROM event_xp ex
        JOIN users u ON u.id = ex.user_id
        WHERE ex.event_id = $1
          AND u.active = TRUE
          AND is_player_allowed(ex.user_id, $1) = TRUE",
    )
    .bind(event_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Insert or update a persistent event leaderboard entry.
pub async fn upsert_persistent_event_leaderboard(
    pool: &PgPool,
    event_id: i64,
    guild_id: i64,
    channel_id: i64,
    message_ids: &serde_json::Value,
    status_message_id: i64,
    milestone_message_id: i64,
    display_count: i32,
    created_at: &DateTime<Utc>,
    last_updated: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::upsert_persistent_event_leaderboard: event_id={}, guild_id={}, channel_id={}, milestone_message_id={}, display_count={}",
        event_id, guild_id, channel_id, milestone_message_id, display_count
    );
    sqlx::query(
        "INSERT INTO persistent_event_leaderboards
         (event_id, guild_id, channel_id, message_ids, status_message_id, milestone_message_id, display_count, created_at, last_updated)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT(event_id) DO UPDATE SET
             guild_id = excluded.guild_id,
             channel_id = excluded.channel_id,
             message_ids = excluded.message_ids,
             status_message_id = excluded.status_message_id,
             milestone_message_id = excluded.milestone_message_id,
             display_count = excluded.display_count,
             created_at = excluded.created_at,
             last_updated = excluded.last_updated",
    )
    .bind(event_id)
    .bind(guild_id)
    .bind(channel_id)
    .bind(message_ids)
    .bind(status_message_id)
    .bind(milestone_message_id)
    .bind(display_count)
    .bind(created_at)
    .bind(last_updated)
    .execute(pool)
    .await?;
    Ok(())
}

/// Retrieve the persistent event leaderboard row for an event, if one exists.
pub async fn get_persistent_event_leaderboard(
    pool: &PgPool,
    event_id: i64,
) -> Result<Option<DbPersistentEventLeaderboard>, sqlx::Error> {
    debug!(
        "queries::get_persistent_event_leaderboard: event_id={}",
        event_id
    );
    sqlx::query_as::<_, DbPersistentEventLeaderboard>(
        "SELECT * FROM persistent_event_leaderboards WHERE event_id = $1",
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await
}

/// Get all persistent event leaderboard rows (used by the updater background task).
pub async fn get_all_persistent_event_leaderboards(
    pool: &PgPool,
) -> Result<Vec<DbPersistentEventLeaderboard>, sqlx::Error> {
    debug!("queries::get_all_persistent_event_leaderboards");
    sqlx::query_as::<_, DbPersistentEventLeaderboard>("SELECT * FROM persistent_event_leaderboards")
        .fetch_all(pool)
        .await
}

/// Update message IDs and last_updated for a persistent event leaderboard.
pub async fn update_persistent_event_leaderboard_messages(
    pool: &PgPool,
    event_id: i64,
    message_ids: &serde_json::Value,
    last_updated: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::update_persistent_event_leaderboard_messages: event_id={}, last_updated={}",
        event_id, last_updated
    );
    sqlx::query(
        "UPDATE persistent_event_leaderboards
         SET message_ids = $1, last_updated = $2
         WHERE event_id = $3",
    )
    .bind(message_ids)
    .bind(last_updated)
    .bind(event_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Delete the persistent event leaderboard row for an event.
pub async fn delete_persistent_event_leaderboard(
    pool: &PgPool,
    event_id: i64,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::delete_persistent_event_leaderboard: event_id={}",
        event_id
    );
    sqlx::query("DELETE FROM persistent_event_leaderboards WHERE event_id = $1")
        .bind(event_id)
        .execute(pool)
        .await?;
    Ok(())
}

// =========================================================================
// event_status_messages
// =========================================================================

/// Insert or update an event status message entry.
pub async fn upsert_event_status_message(
    pool: &PgPool,
    event_id: i64,
    channel_id: i64,
    message_id: i64,
    created_at: &DateTime<Utc>,
    updated_at: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::upsert_event_status_message: event_id={}, channel_id={}, message_id={}",
        event_id, channel_id, message_id
    );
    sqlx::query(
        "INSERT INTO event_status_messages
         (event_id, channel_id, message_id, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT(event_id) DO UPDATE SET
             channel_id = excluded.channel_id,
             message_id = excluded.message_id,
             updated_at = excluded.updated_at",
    )
    .bind(event_id)
    .bind(channel_id)
    .bind(message_id)
    .bind(created_at)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Retrieve the event status message row for an event, if one exists.
pub async fn get_event_status_message(
    pool: &PgPool,
    event_id: i64,
) -> Result<Option<DbEventStatusMessage>, sqlx::Error> {
    debug!("queries::get_event_status_message: event_id={}", event_id);
    sqlx::query_as::<_, DbEventStatusMessage>(
        "SELECT * FROM event_status_messages WHERE event_id = $1",
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await
}

/// Get all event status message rows (used by the updater background task).
pub async fn get_all_event_status_messages(
    pool: &PgPool,
) -> Result<Vec<DbEventStatusMessage>, sqlx::Error> {
    debug!("queries::get_all_event_status_messages");
    sqlx::query_as::<_, DbEventStatusMessage>("SELECT * FROM event_status_messages")
        .fetch_all(pool)
        .await
}

/// Delete the event status message row for an event.
pub async fn delete_event_status_message(pool: &PgPool, event_id: i64) -> Result<(), sqlx::Error> {
    debug!(
        "queries::delete_event_status_message: event_id={}",
        event_id
    );
    sqlx::query("DELETE FROM event_status_messages WHERE event_id = $1")
        .bind(event_id)
        .execute(pool)
        .await?;
    Ok(())
}

// =========================================================================
// backfill
// =========================================================================

/// Count the number of `stat_deltas` rows that fall within the event's time
/// window and match one of its configured stat names. Used to give admins an
/// estimate before running a manual backfill.
pub async fn count_deltas_for_event(pool: &PgPool, event_id: i64) -> Result<i64, sqlx::Error> {
    debug!("queries::count_deltas_for_event: event_id={}", event_id);

    let event: DbEvent = sqlx::query_as::<_, DbEvent>("SELECT * FROM events WHERE id = $1")
        .bind(event_id)
        .fetch_one(pool)
        .await?;

    let window_end = event.end_date.min(Utc::now());

    let stat_names: Vec<String> =
        sqlx::query_as::<_, (String,)>("SELECT stat_name FROM event_stats WHERE event_id = $1")
            .bind(event_id)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|(s,)| s)
            .collect();

    if stat_names.is_empty() {
        return Ok(0);
    }

    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM stat_deltas
         WHERE stat_name = ANY($1)
           AND created_at >= $2
           AND created_at <= $3
           AND delta > 0",
    )
    .bind(&stat_names)
    .bind(event.start_date)
    .bind(window_end)
    .fetch_one(pool)
    .await?;

    Ok(row.0)
}

/// Process one batch of `stat_deltas` inside a single transaction.
///
/// Returns a map of `user_id → xp_earned` for newly-inserted rows only
/// (skipping deltas that were already in `event_xp` via `ON CONFLICT DO NOTHING`).
async fn process_batch(
    pool: &PgPool,
    event_id: i64,
    batch: &[DbStatDelta],
    stat_map: &HashMap<String, DbEventStat>,
) -> Result<HashMap<i64, f64>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut user_xp: HashMap<i64, f64> = HashMap::new();

    for delta in batch {
        let es = match stat_map.get(&delta.stat_name) {
            Some(s) => s,
            None => continue,
        };

        let units = delta.delta;
        if units <= 0 {
            continue;
        }

        let xp_earned = es.xp_per_unit * units as f64;

        let row: Option<(f64,)> = sqlx::query_as(
            r#"
            INSERT INTO event_xp
                 (event_id, user_id, stat_name, delta_id, units, xp_per_unit, xp_earned, created_at)
            SELECT $1 AS event_id,
                   $2 AS user_id,
                   $3 AS stat_name,
                   $4 AS delta_id,
                   $5 AS units,
                   $6 AS xp_per_unit,
                   $7 AS xp_earned,
                   NOW() AS created_at
            FROM users u
            WHERE u.id = $2
              AND u.active = TRUE
                            AND is_player_allowed(u.id, $1) = TRUE
            ON CONFLICT (event_id, delta_id) DO NOTHING
            RETURNING xp_earned
            "#,
        )
        .bind(event_id)
        .bind(delta.user_id)
        .bind(&delta.stat_name)
        .bind(delta.id)
        .bind(units)
        .bind(es.xp_per_unit)
        .bind(xp_earned)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some((actual_xp,)) = row {
            *user_xp.entry(delta.user_id).or_insert(0.0) += actual_xp;
        }
    }

    tx.commit().await?;
    Ok(user_xp)
}

/// Run `process_batch` with up to `max_retries` attempts, using exponential
/// back-off of 100 ms × attempt number between retries.
async fn process_batch_with_retry(
    pool: &PgPool,
    event_id: i64,
    batch: &[DbStatDelta],
    stat_map: &HashMap<String, DbEventStat>,
    max_retries: u32,
) -> Result<HashMap<i64, f64>, sqlx::Error> {
    let mut last_err: Option<sqlx::Error> = None;

    for attempt in 1..=max_retries {
        match process_batch(pool, event_id, batch, stat_map).await {
            Ok(map) => return Ok(map),
            Err(e) => {
                warn!(
                    event_id,
                    attempt,
                    error = %e,
                    "Backfill batch failed, will retry."
                );
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(100 * attempt as u64)).await;
            }
        }
    }

    Err(last_err.unwrap())
}

/// Retroactively award event XP for all `stat_deltas` rows that fall within
/// the event's time window and match one of its configured stat names.
///
/// Uses cursor-based pagination (500 rows per batch) so it is safe to run
/// against large delta tables without OFFSET skew. Each batch is committed in
/// its own transaction and `ON CONFLICT … DO NOTHING` makes the whole job
/// idempotent — safe to re-run after a crash or via `/edit-events backfill`.
///
/// After all batches finish, `total_xp` is incremented once per affected user
/// and their level is recalculated once.
pub async fn backfill_event_xp(
    pool: &PgPool,
    event_id: i64,
    base_level_xp: f64,
    level_exponent: f64,
) -> Result<BackfillSummary, sqlx::Error> {
    use crate::xp::calculator::calculate_level;

    info!(event_id, "Starting backfill.");

    // Load event.
    let event: DbEvent = sqlx::query_as::<_, DbEvent>("SELECT * FROM events WHERE id = $1")
        .bind(event_id)
        .fetch_one(pool)
        .await?;

    let window_start = event.start_date;
    let window_end = event.end_date.min(Utc::now());

    // Load event stats into a map keyed by stat_name.
    let stats: Vec<DbEventStat> =
        sqlx::query_as::<_, DbEventStat>("SELECT * FROM event_stats WHERE event_id = $1")
            .bind(event_id)
            .fetch_all(pool)
            .await?;

    if stats.is_empty() {
        info!(event_id, "No event stats configured; skipping backfill.");
        return Ok(BackfillSummary::default());
    }

    let stat_map: HashMap<String, DbEventStat> = stats
        .into_iter()
        .map(|s| (s.stat_name.clone(), s))
        .collect();

    let stat_names: Vec<String> = stat_map.keys().cloned().collect();

    // Cursor-based batch loop.
    let mut cursor_id: i64 = 0;
    let mut batch_num: i64 = 0;
    let mut total_deltas: i64 = 0;
    let mut user_xp_map: HashMap<i64, f64> = HashMap::new();

    loop {
        let batch: Vec<DbStatDelta> = sqlx::query_as::<_, DbStatDelta>(
            "SELECT * FROM stat_deltas
             WHERE stat_name = ANY($1)
               AND created_at >= $2
               AND created_at <= $3
               AND delta > 0
               AND id > $4
             ORDER BY id ASC
             LIMIT 500",
        )
        .bind(&stat_names)
        .bind(window_start)
        .bind(window_end)
        .bind(cursor_id)
        .fetch_all(pool)
        .await?;

        if batch.is_empty() {
            break;
        }

        let batch_len = batch.len() as i64;
        cursor_id = batch.last().unwrap().id;

        let batch_xp = process_batch_with_retry(pool, event_id, &batch, &stat_map, 3).await?;

        // Increment XP per-batch for crash safety, and accumulate running totals.
        let now = Utc::now();
        for (&uid, &xp) in &batch_xp {
            if xp > 0.0 {
                if let Err(e) = increment_xp(pool, uid, xp, &now).await {
                    error!(event_id, user_id = uid, error = %e, "Failed to increment XP during backfill batch.");
                }
                *user_xp_map.entry(uid).or_insert(0.0) += xp;
            }
        }

        batch_num += 1;
        total_deltas += batch_len;

        if batch_num % 10 == 0 {
            info!(
                event_id,
                batch_num,
                total_deltas_processed = total_deltas,
                "Backfill progress."
            );
        }

        if batch_len < 500 {
            break;
        }
    }

    // Recalculate levels once per affected user (after all XP has been incremented).
    let now = Utc::now();
    let mut total_xp_awarded = 0.0_f64;
    let users_affected = user_xp_map.len() as i64;

    for (&uid, &xp) in &user_xp_map {
        total_xp_awarded += xp;
        match get_xp(pool, uid).await {
            Ok(Some(xp_row)) => {
                let new_level = calculate_level(xp_row.total_xp, base_level_xp, level_exponent);
                if new_level != xp_row.level {
                    if let Err(e) = update_level(pool, uid, new_level, &now).await {
                        error!(event_id, user_id = uid, error = %e, "Failed to update level during backfill.");
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                error!(event_id, user_id = uid, error = %e, "Failed to fetch XP row during level recalc.");
            }
        }
    }

    let summary = BackfillSummary {
        deltas_processed: total_deltas,
        total_xp_awarded,
        users_affected,
    };

    info!(
        event_id,
        total_deltas_processed = summary.deltas_processed,
        total_xp = summary.total_xp_awarded,
        users_affected = summary.users_affected,
        "Backfill completed."
    );

    Ok(summary)
}

// =========================================================================
// event_milestones
// =========================================================================

/// Insert multiple XP-threshold milestones for an event.
/// Silently skips thresholds that already exist (ON CONFLICT DO NOTHING).
/// Returns the number of rows actually inserted.
pub async fn add_event_milestones(
    pool: &PgPool,
    event_id: i64,
    thresholds: &[f64],
) -> Result<u64, sqlx::Error> {
    debug!(
        "queries::add_event_milestones: event_id={}, count={}",
        event_id,
        thresholds.len()
    );
    let mut inserted = 0u64;
    for &threshold in thresholds {
        let rows = sqlx::query(
            "INSERT INTO event_milestones (event_id, xp_threshold)
             VALUES ($1, $2)
             ON CONFLICT (event_id, xp_threshold) DO NOTHING",
        )
        .bind(event_id)
        .bind(threshold)
        .execute(pool)
        .await?
        .rows_affected();
        inserted += rows;
    }
    Ok(inserted)
}

/// Delete specific XP-threshold milestones for an event.
/// Returns the number of rows deleted.
pub async fn remove_event_milestones(
    pool: &PgPool,
    event_id: i64,
    thresholds: &[f64],
) -> Result<u64, sqlx::Error> {
    debug!(
        "queries::remove_event_milestones: event_id={}, count={}",
        event_id,
        thresholds.len()
    );
    let rows = sqlx::query(
        "DELETE FROM event_milestones
         WHERE event_id = $1 AND xp_threshold = ANY($2)",
    )
    .bind(event_id)
    .bind(thresholds)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rows)
}

/// Get all milestones for an event, ordered ascending by xp_threshold.
pub async fn get_event_milestones(
    pool: &PgPool,
    event_id: i64,
) -> Result<Vec<DbEventMilestone>, sqlx::Error> {
    debug!("queries::get_event_milestones: event_id={}", event_id);
    sqlx::query_as::<_, DbEventMilestone>(
        "SELECT * FROM event_milestones WHERE event_id = $1 ORDER BY xp_threshold ASC",
    )
    .bind(event_id)
    .fetch_all(pool)
    .await
}

/// Get all milestones for an event with the count of users who have reached each one.
pub async fn get_event_milestones_with_counts(
    pool: &PgPool,
    event_id: i64,
) -> Result<Vec<EventMilestoneWithCount>, sqlx::Error> {
    debug!(
        "queries::get_event_milestones_with_counts: event_id={}",
        event_id
    );
    sqlx::query_as::<_, EventMilestoneWithCount>(
        "SELECT em.id, em.event_id, em.xp_threshold,
                COUNT(sub.user_id) AS user_count
         FROM event_milestones em
         LEFT JOIN (
             SELECT ex.user_id, SUM(ex.xp_earned) AS total_xp
             FROM event_xp ex
             JOIN users u ON u.id = ex.user_id
             WHERE ex.event_id = $1
               AND u.active = TRUE
                             AND is_player_allowed(ex.user_id, $1) = TRUE
             GROUP BY ex.user_id
         ) sub ON sub.total_xp >= em.xp_threshold
         WHERE em.event_id = $1
         GROUP BY em.id
         ORDER BY em.xp_threshold ASC",
    )
    .bind(event_id)
    .fetch_all(pool)
    .await
}

/// Get the discord_user_ids of all users who have reached a given XP threshold in an event.
pub async fn get_event_milestone_completers(
    pool: &PgPool,
    event_id: i64,
    xp_threshold: f64,
) -> Result<Vec<(i64, String, f64)>, sqlx::Error> {
    debug!(
        "queries::get_event_milestone_completers: event_id={}, xp_threshold={}",
        event_id, xp_threshold
    );
    let rows: Vec<(i64, String, f64)> = sqlx::query_as(
        "SELECT 
            u.discord_user_id,
            u.minecraft_username,
            SUM(ex.xp_earned) as total_xp
         FROM event_xp ex
         JOIN users u ON u.id = ex.user_id
         WHERE ex.event_id = $1
           AND u.active = TRUE
                     AND is_player_allowed(ex.user_id, $1) = TRUE
         GROUP BY u.id, u.discord_user_id, u.minecraft_username
         HAVING SUM(ex.xp_earned)::DOUBLE PRECISION >= $2
         ORDER BY total_xp DESC",
    )
    .bind(event_id)
    .bind(xp_threshold)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Update only the milestone_message_id for a persistent event leaderboard.
pub async fn update_persistent_event_leaderboard_milestone_message(
    pool: &PgPool,
    event_id: i64,
    milestone_message_id: i64,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::update_persistent_event_leaderboard_milestone_message: event_id={}, milestone_message_id={}",
        event_id, milestone_message_id
    );
    sqlx::query(
        "UPDATE persistent_event_leaderboards
         SET milestone_message_id = $1
         WHERE event_id = $2",
    )
    .bind(milestone_message_id)
    .bind(event_id)
    .execute(pool)
    .await?;
    Ok(())
}

// =========================================================================
// Statistics aggregates
// =========================================================================

/// A single named statistic value used in [`GuildStatistics`].
#[derive(Debug, Clone)]
pub struct StatisticValue {
    pub key: String,
    pub label: String,
    pub value: i64,
}

/// Aggregated statistics for a guild or event, used by the statistics card.
#[derive(Debug, Clone)]
pub struct GuildStatistics {
    /// Raw total messages (every non-bot guild message, since rollout).
    pub total_messages: i64,
    /// Validated messages (passed spam/length checks).
    pub valid_messages: i64,
    /// Total voice chat minutes across all active members.
    pub total_vc_minutes: i64,
    /// Total XP held by active members (guild-wide) or earned in event.
    pub total_xp: f64,
    /// Number of participants; `Some` only for event statistics.
    pub participants: Option<i64>,
    /// Other per-stat totals from `stat_deltas`, sorted descending by value,
    /// excluding the headline stats handled above.
    pub other_stat_changes: Vec<StatisticValue>,
}

/// Compute aggregated guild-wide statistics for a specific time range.
///
/// All aggregation is done in SQL for performance. XP within a range is
/// sourced from `xp_events` (per-delta XP records), while stat totals come
/// from `stat_deltas` filtered by `created_at`.
pub async fn get_guild_statistics_ranged(
    pool: &PgPool,
    guild_id: i64,
    start_dt: DateTime<Utc>,
    end_dt: DateTime<Utc>,
) -> Result<GuildStatistics, sqlx::Error> {
    debug!(
        "queries::get_guild_statistics_ranged: guild_id={}, start={}, end={}",
        guild_id, start_dt, end_dt
    );

    // Total XP earned within the time range (from xp_events).
    let (total_xp,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(xe.xp_earned), 0)
         FROM xp_events xe
         JOIN users u ON u.id = xe.user_id
         WHERE u.guild_id = $1
           AND u.active = TRUE
           AND xe.created_at >= $2
           AND xe.created_at <= $3",
    )
    .bind(guild_id)
    .bind(start_dt)
    .bind(end_dt)
    .fetch_one(pool)
    .await?;

    // Per-stat delta sums within the time range.
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT sd.stat_name, COALESCE(SUM(sd.delta)::bigint, 0) as total
         FROM stat_deltas sd
         JOIN users u ON u.id = sd.user_id
         WHERE u.guild_id = $1
           AND u.active = TRUE
           AND sd.delta > 0
           AND sd.created_at >= $2
           AND sd.created_at <= $3
         GROUP BY sd.stat_name
         ORDER BY total DESC",
    )
    .bind(guild_id)
    .bind(start_dt)
    .bind(end_dt)
    .fetch_all(pool)
    .await?;

    let mut total_messages = 0_i64;
    let mut valid_messages = 0_i64;
    let mut total_vc_minutes = 0_i64;
    let mut other_stat_changes: Vec<StatisticValue> = Vec::new();

    for (key, value) in rows {
        match key.as_str() {
            "total_messages_raw" => total_messages = value,
            "messages_sent" => valid_messages = value,
            "voice_minutes" => total_vc_minutes = value,
            _ => {
                use crate::utils::stats_definitions::display_name_for_key;
                let label = display_name_for_key(&key);
                other_stat_changes.push(StatisticValue { key, label, value });
            }
        }
    }

    Ok(GuildStatistics {
        total_messages,
        valid_messages,
        total_vc_minutes,
        total_xp,
        participants: None,
        other_stat_changes,
    })
}

/// Compute aggregated guild-wide statistics across all active members.
pub async fn get_guild_statistics(
    pool: &PgPool,
    guild_id: i64,
) -> Result<GuildStatistics, sqlx::Error> {
    debug!("queries::get_guild_statistics: guild_id={}", guild_id);

    // Total XP held by active members.
    let (total_xp,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(x.total_xp), 0)
         FROM xp x
         JOIN users u ON u.id = x.user_id
         WHERE u.guild_id = $1",
    )
    .bind(guild_id)
    .fetch_one(pool)
    .await?;

    // Per-stat delta sums for all active members.
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT sd.stat_name, COALESCE(SUM(sd.delta)::bigint, 0) as total
         FROM stat_deltas sd
         JOIN users u ON u.id = sd.user_id
         WHERE u.guild_id = $1 AND sd.delta > 0
         GROUP BY sd.stat_name
         ORDER BY total DESC",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await?;

    let mut total_messages = 0_i64;
    let mut valid_messages = 0_i64;
    let mut total_vc_minutes = 0_i64;
    let mut other_stat_changes: Vec<StatisticValue> = Vec::new();

    for (key, value) in rows {
        match key.as_str() {
            "total_messages_raw" => total_messages = value,
            "messages_sent" => valid_messages = value,
            "voice_minutes" => total_vc_minutes = value,
            _ => {
                use crate::utils::stats_definitions::display_name_for_key;
                let label = display_name_for_key(&key);
                other_stat_changes.push(StatisticValue { key, label, value });
            }
        }
    }

    // other_stat_changes already sorted desc by the ORDER BY clause.

    Ok(GuildStatistics {
        total_messages,
        valid_messages,
        total_vc_minutes,
        total_xp,
        participants: None,
        other_stat_changes,
    })
}

/// Compute aggregated statistics for a specific event.
pub async fn get_event_statistics(
    pool: &PgPool,
    event_id: i64,
) -> Result<GuildStatistics, sqlx::Error> {
    debug!("queries::get_event_statistics: event_id={}", event_id);

    // Total XP earned within this event.
    let (total_xp,): (f64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(ex.xp_earned)::DOUBLE PRECISION, 0.0)
         FROM event_xp ex
         JOIN users u ON u.id = ex.user_id
         WHERE ex.event_id = $1
           AND u.active = TRUE
                     AND is_player_allowed(ex.user_id, $1) = TRUE",
    )
    .bind(event_id)
    .fetch_one(pool)
    .await?;

    // Per-stat unit totals for this event.
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT ex.stat_name, COALESCE(SUM(ex.units)::bigint, 0) AS total
         FROM event_xp ex
         JOIN users u ON u.id = ex.user_id
         WHERE ex.event_id = $1
           AND u.active = TRUE
                     AND is_player_allowed(ex.user_id, $1) = TRUE
         GROUP BY ex.stat_name
         ORDER BY total DESC",
    )
    .bind(event_id)
    .fetch_all(pool)
    .await?;

    let mut total_messages = 0_i64;
    let mut valid_messages = 0_i64;
    let mut total_vc_minutes = 0_i64;
    let mut other_stat_changes: Vec<StatisticValue> = Vec::new();

    for (key, value) in rows {
        match key.as_str() {
            "total_messages_raw" => total_messages = value,
            "messages_sent" => valid_messages = value,
            "voice_minutes" => total_vc_minutes = value,
            _ => {
                use crate::utils::stats_definitions::display_name_for_key;
                let label = display_name_for_key(&key);
                other_stat_changes.push(StatisticValue { key, label, value });
            }
        }
    }

    let participants = count_event_participants(pool, event_id).await?;

    Ok(GuildStatistics {
        total_messages,
        valid_messages,
        total_vc_minutes,
        total_xp,
        participants: Some(participants),
        other_stat_changes,
    })
}

pub async fn remove_global_event_ban(
    pool: &PgPool,
    user_id: i64,
    moderator_id: i64,
    guild_id: i64,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    unban_user_from_events(pool, user_id, moderator_id, guild_id, reason).await
}

pub async fn is_user_disqualified_in_any_active_event(
    pool: &PgPool,
    guild_id: i64,
    user_id: i64,
) -> Result<bool, sqlx::Error> {
    let exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM events e
            WHERE e.guild_id = $1
              AND e.status = 'active'
              AND NOT is_player_allowed($2, e.id)
        )
        "#,
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(exists)
}

pub async fn is_user_disqualified_from_event(
    pool: &PgPool,
    event_id: i64,
    user_id: i64,
) -> Result<bool, sqlx::Error> {
    let allowed: bool = sqlx::query_scalar("SELECT is_player_allowed($1, $2)")
        .bind(user_id)
        .bind(event_id)
        .fetch_one(pool)
        .await?;

    Ok(!allowed)
}

/// Check whether a user currently has an active global event ban based on
/// their latest modrec ban/unban action.
pub async fn is_user_globally_banned(pool: &PgPool, user_id: i64) -> Result<bool, sqlx::Error> {
    let banned: Option<bool> = sqlx::query_scalar(
        r#"
        SELECT CASE
                 WHEN action_type = 'ban'
                      AND (ban_expires_at IS NULL OR ban_expires_at > NOW())
                 THEN TRUE
                 ELSE FALSE
               END
        FROM modrec
        WHERE user_id = $1
          AND action_type IN ('ban', 'unban')
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(banned.unwrap_or(false))
}

pub async fn get_active_punishments(
    pool: &PgPool,
    guild_id: i64,
) -> Result<
    Vec<(
        i64,
        String,
        Option<i64>,
        Option<DateTime<Utc>>,
        String,
        DateTime<Utc>,
    )>,
    sqlx::Error,
> {
    debug!("queries::get_active_punishments: guild_id={}", guild_id);

    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            Option<i64>,
            Option<DateTime<Utc>>,
            String,
            DateTime<Utc>,
        ),
    >(
        r#"
        WITH latest_bans AS (
            SELECT DISTINCT ON (m.user_id)
                m.user_id,
                m.action_type,
                m.ban_expires_at,
                m.reason,
                m.created_at
            FROM modrec m
            WHERE m.guild_id = $1
              AND m.action_type IN ('ban', 'unban')
            ORDER BY m.user_id, m.created_at DESC, m.id DESC
        ),
        latest_dq AS (
            SELECT DISTINCT ON (m.user_id, m.event_id)
                m.user_id,
                m.event_id,
                m.action_type,
                m.reason,
                m.created_at
            FROM modrec m
            WHERE m.guild_id = $1
              AND m.action_type IN ('disqualify', 'undisqualify')
            ORDER BY m.user_id, m.event_id, m.created_at DESC, m.id DESC
        )

        -- Active bans
        SELECT
            u.discord_user_id as user_id,
            lb.action_type::text,
            NULL::BIGINT as event_id,
            lb.ban_expires_at,
            lb.reason,
            lb.created_at
        FROM latest_bans lb
        JOIN users u ON u.id = lb.user_id
        WHERE lb.action_type = 'ban'
          AND (lb.ban_expires_at IS NULL OR lb.ban_expires_at > NOW())

        UNION ALL

        -- Active disqualifications
        SELECT
            u.discord_user_id as user_id,
            ld.action_type::text,
            ld.event_id,
            NULL::TIMESTAMPTZ,
            ld.reason,
            ld.created_at
        FROM latest_dq ld
        JOIN users u ON u.id = ld.user_id
        WHERE ld.action_type = 'disqualify'
        "#,
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

// =========================================================================
// VC Session queries
// =========================================================================

pub async fn add_vc_session(
    pool: &PgPool,
    discord_user_id: i64,
    guild_id: i64,
    join_time: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::add_vc_session: discord_user_id={}, guild_id={}, join_time={}",
        discord_user_id, guild_id, join_time
    );

    sqlx::query(
        r#"
        INSERT INTO vc_sessions (user_id, guild_id, join_time)
        SELECT id, guild_id, $3
        FROM users
        WHERE discord_user_id = $1 AND guild_id = $2
        "#,
    )
    .bind(discord_user_id)
    .bind(guild_id)
    .bind(join_time)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn end_vc_session(
    pool: &PgPool,
    discord_user_id: i64,
    guild_id: i64,
    leave_time: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    debug!(
        "queries::end_vc_session: discord_user_id={}, guild_id={}, leave_time={}",
        discord_user_id, guild_id, leave_time
    );

    let row = sqlx::query(
        r#"
        UPDATE vc_sessions
        SET leave_time = $3
        WHERE id = (
            SELECT s.id
            FROM vc_sessions s
            JOIN users u ON u.id = s.user_id
            WHERE u.discord_user_id = $1
            AND u.guild_id = $2
            AND s.leave_time IS NULL
            ORDER BY s.join_time DESC
            LIMIT 1
        )
        RETURNING join_time;
        "#,
    )
    .bind(discord_user_id)
    .bind(guild_id)
    .bind(leave_time)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.get::<DateTime<Utc>, _>("join_time")))
}

pub async fn get_active_vc_session(
    pool: &PgPool,
    discord_user_id: i64,
    guild_id: i64,
) -> Result<Option<DbVcSession>, sqlx::Error> {
    debug!(
        "queries::get_active_vc_session: discord_user_id={}, guild_id={}",
        discord_user_id, guild_id
    );

    sqlx::query_as::<_, DbVcSession>(
        r#"
        SELECT s.id, s.user_id, s.guild_id, s.join_time, s.leave_time
        FROM vc_sessions s
        JOIN users u ON u.id = s.user_id
        WHERE u.discord_user_id = $1
          AND u.guild_id = $2
          AND s.leave_time IS NULL
        ORDER BY s.join_time DESC
        LIMIT 1
        "#,
    )
    .bind(discord_user_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await
}

pub async fn end_all_active_vc_sessions_for_guild(
    pool: &PgPool,
    guild_id: i64,
    leave_time: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::end_all_active_vc_sessions_for_guild: guild_id={}, leave_time={}",
        guild_id, leave_time
    );

    let result = sqlx::query(
        r#"
        UPDATE vc_sessions
        SET leave_time = $2
        WHERE guild_id = $1
          AND leave_time IS NULL
        "#,
    )
    .bind(guild_id)
    .bind(leave_time)
    .execute(pool)
    .await?;

    info!(
        guild_id,
        sessions_updated = result.rows_affected(),
        "Ended all active VC sessions for guild."
    );

    Ok(())
}

pub async fn get_vc_sessions_for_user(
    pool: &PgPool,
    discord_user_id: i64,
) -> Result<Vec<DbVcSession>, sqlx::Error> {
    debug!(
        "queries::get_vc_sessions_for_user: discord_user_id={}",
        discord_user_id
    );

    sqlx::query_as::<_, DbVcSession>(
        r#"
        SELECT s.id, s.user_id, s.guild_id, s.join_time, s.leave_time
        FROM vc_sessions s
        JOIN users u ON u.id = s.user_id
        WHERE u.discord_user_id = $1
        ORDER BY s.join_time DESC
        "#,
    )
    .bind(discord_user_id)
    .fetch_all(pool)
    .await
}

pub async fn get_vc_sessions_user_for_day(
    pool: &PgPool,
    discord_user_id: i64,
    guild_id: i64,
    date: DateTime<Utc>,
) -> Result<Vec<DbVcSession>, sqlx::Error> {
    debug!(
        "queries::get_vc_sessions_user_for_day: discord_user_id={}, guild_id={}, date={}",
        discord_user_id, guild_id, date
    );

    let date_naive = date.date_naive();

    let day_start = date_naive.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let day_end = (date_naive + chrono::Duration::days(1))
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();

    sqlx::query_as::<_, DbVcSession>(
        r#"
        SELECT s.id, s.user_id, s.guild_id, s.join_time, s.leave_time
        FROM vc_sessions s
        JOIN users u ON u.id = s.user_id
        WHERE u.discord_user_id = $1
          AND u.guild_id = $2
          AND s.join_time < $4
          AND (s.leave_time IS NULL OR s.leave_time > $3)
        ORDER BY s.join_time DESC
        "#,
    )
    .bind(discord_user_id)
    .bind(guild_id)
    .bind(day_start)
    .bind(day_end)
    .fetch_all(pool)
    .await
}

// =========================================================================
// event_message_requirements & leaderboard_positions
// =========================================================================

/// Add a new message requirement for an event
pub async fn add_event_message_requirement(
    pool: &PgPool,
    event_id: i64,
    min_messages: i32,
    positions: Vec<i32>,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        INSERT INTO event_message_requirements (event_id, min_messages, positions)
        VALUES ($1, $2, $3)
        RETURNING id
        "#,
        event_id,
        min_messages,
        &positions
    )
    .fetch_one(pool)
    .await?;

    Ok(row.id)
}

/// Get all message requirements for an event
pub async fn get_event_message_requirements(
    pool: &PgPool,
    event_id: i64,
) -> Result<Vec<EventMessageRequirementDetail>, sqlx::Error> {
    sqlx::query_as!(
        EventMessageRequirementDetail,
        r#"
        SELECT id, event_id, min_messages, positions as "positions!", created_at
        FROM event_message_requirements
        WHERE event_id = $1
        ORDER BY min_messages DESC, id
        "#,
        event_id
    )
    .fetch_all(pool)
    .await
}

/// Validates a user's progress against an event requirement.
///
/// If the user's position is not in the required list, it returns `false`
/// with `messages_required` set to 0.
pub fn check_requirement_completion(
    requirement: &EventMessageRequirementDetail,
    user_position: i32,
    user_messages: i32,
) -> RequirementStatus {
    if !requirement.positions.contains(&user_position) {
        return RequirementStatus {
            is_completed: false,
            messages_required: 0,
            current_messages: user_messages,
        };
    }

    let is_completed = user_messages >= requirement.min_messages;

    let messages_required = if is_completed {
        0
    } else {
        requirement.min_messages - user_messages
    };

    RequirementStatus {
        is_completed,
        messages_required,
        current_messages: user_messages,
    }
}

/// Get a specific requirement by ID
pub async fn get_event_requirement(
    pool: &PgPool,
    requirement_id: i64,
) -> Result<Option<EventMessageRequirementDetail>, sqlx::Error> {
    sqlx::query_as!(
        EventMessageRequirementDetail,
        r#"
        SELECT id, event_id, min_messages, positions as "positions!", created_at
        FROM event_message_requirements
        WHERE id = $1
        "#,
        requirement_id
    )
    .fetch_optional(pool)
    .await
}

/// Remove specific ranks from requirements, deleting requirements that become empty
pub async fn remove_event_message_requirement_positions(
    pool: &PgPool,
    event_id: i64,
    positions_to_remove: Vec<i32>,
) -> Result<Vec<i64>, sqlx::Error> {
    // Use SQL to filter out positions and delete if empty
    let rows = sqlx::query!(
        r#"
        WITH updated AS (
            UPDATE event_message_requirements
            SET positions = (
                SELECT ARRAY_AGG(pos)
                FROM UNNEST(positions) AS pos
                WHERE pos <> ALL($2::int[])
            )
            WHERE event_id = $1
            AND positions && $2::int[]  -- Only update if there's overlap
            RETURNING id, positions
        )
        DELETE FROM event_message_requirements
        WHERE id IN (
            SELECT id FROM updated WHERE positions IS NULL OR array_length(positions, 1) = 0
        )
        RETURNING id
        "#,
        event_id,
        &positions_to_remove
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|r| r.id).collect())
}

pub async fn get_requirement_for_position(
    pool: &PgPool,
    event_id: i64,
    position: i32,
) -> Result<Option<EventMessageRequirementDetail>, sqlx::Error> {
    let requirements = sqlx::query_as!(
        EventMessageRequirementDetail,
        r#"
        SELECT id, event_id, min_messages, positions as "positions!", created_at
        FROM event_message_requirements
        WHERE event_id = $1
        ORDER BY min_messages DESC, id
        "#,
        event_id
    )
    .fetch_all(pool)
    .await?;

    Ok(requirements
        .into_iter()
        .find(|req| req.positions.contains(&position)))
}

pub async fn check_requirement_for_position(
    pool: &PgPool,
    event_id: i64,
    user_position: i32,
    user_messages: i32,
) -> Result<Option<RequirementStatus>, sqlx::Error> {
    // Fetch ONLY the relevant requirement directly from DB
    let requirement = sqlx::query_as!(
        EventMessageRequirementDetail,
        r#"
        SELECT id, event_id, min_messages, positions as "positions!", created_at
        FROM event_message_requirements
        WHERE event_id = $1
        AND $2 = ANY(positions)
        ORDER BY min_messages DESC
        LIMIT 1
        "#,
        event_id,
        user_position
    )
    .fetch_optional(pool)
    .await?;

    // If no requirement applies → return None
    let requirement = match requirement {
        Some(req) => req,
        None => return Ok(None),
    };

    // Check completion
    let is_completed = user_messages >= requirement.min_messages;

    let messages_required = if is_completed {
        0
    } else {
        requirement.min_messages - user_messages
    };

    Ok(Some(RequirementStatus {
        is_completed,
        messages_required,
        current_messages: user_messages,
    }))
}
