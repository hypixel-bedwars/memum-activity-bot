use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
/// Database query functions.
///
/// All functions accept a `&PgPool` so they can be called from any context
/// that has access to the shared `Data` struct. Queries are organized by table.
///
/// Some functions are not yet called but exist as part of the public query API
/// for extensions and future commands.
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashMap;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::models::{
    BackfillSummary, DbDailySnapshot, DbEvent, DbEventStat, DbGuild, DbMilestone,
    DbPersistentEventLeaderboard, DbPersistentLeaderboard, DbStatDelta, DbStatsSnapshot,
    DbSweepCursor, DbUser, DbXP, EventLeaderboardEntry, EventParticipant, LeaderboardEntry,
    MilestoneWithCount,
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

/// Retrieve the configured logging channel for a guild, if any.
/// Retrieve all guild IDs that have a logging channel configured.
/// Used by background sweepers to broadcast log messages to every
/// guild that has opted in to logging.
pub async fn get_guilds_with_log_channel(pool: &PgPool) -> Result<Vec<i64>, sqlx::Error> {
    debug!("queries::get_guilds_with_log_channel");

    let rows: Vec<(i64,)> =
        sqlx::query_as("SELECT guild_id FROM guilds WHERE log_channel_id IS NOT NULL")
            .fetch_all(pool)
            .await?;

    Ok(rows.into_iter().map(|(id,)| id).collect())
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
             minecraft_username = excluded.minecraft_username",
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
    before_ts: &str,
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
    before_ts: &str,
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

/// Look up a user by Discord ID within a specific guild.
pub async fn get_user_by_discord_id(
    pool: &PgPool,
    discord_user_id: i64,
    guild_id: i64,
) -> Result<Option<DbUser>, sqlx::Error> {
    debug!(
        "queries::get_user_by_discord_id: discord_user_id={}, guild_id={}",
        discord_user_id, guild_id
    );
    sqlx::query_as::<_, DbUser>("SELECT * FROM users WHERE discord_user_id = $1 AND guild_id = $2")
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

/// Record that the user invoked a stat-related command right now.
///
/// Called at the start of `/level` and `/stats` so that the background
/// sweeper can identify recently active users and prioritise their refresh
/// slot in the next sweep cycle.
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
    updated_at: &str,
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

/// Unregister a user by deleting their row from the database.
pub async fn unregister_user(
    pool: &PgPool,
    discord_user_id: i64,
    guild_id: i64,
) -> Result<(), sqlx::Error> {
    let user: Option<DbUser> =
        sqlx::query_as("SELECT * FROM users WHERE discord_user_id = $1 AND guild_id = $2")
            .bind(discord_user_id)
            .bind(guild_id)
            .fetch_optional(pool)
            .await?;

    let Some(user) = user else {
        return Ok(());
    };

    let user_id = user.id;

    // delete dependent rows
    sqlx::query("DELETE FROM sweep_cursor WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM xp WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM stat_deltas WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM xp_events WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM hypixel_stats_snapshot WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM discord_stats_snapshot WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;

    // finally delete user
    sqlx::query(
        "DELETE FROM users
         WHERE discord_user_id = $1 AND guild_id = $2",
    )
    .bind(discord_user_id)
    .bind(guild_id)
    .execute(pool)
    .await?;

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
			 WHERE u.guild_id = $1
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
    stat_value: f64,
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
    stat_value: f64,
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
    stat_value: f64,
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
    stat_value: f64,
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
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE guild_id = $1")
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
    old_value: f64,
    new_value: f64,
    delta: f64,
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
         LEFT JOIN users u ON u.guild_id = m.guild_id
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
) -> Result<Vec<(String, f64)>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, f64)>(
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
    difference: f64,
    now: &DateTime<Utc>,
) -> Result<f64, sqlx::Error> {
    debug!(
        "queries::award_event_xp_for_delta: guild_id={}, user_id={}, stat_name={}, delta_id={}, difference={}",
        guild_id, user_id, stat_name, delta_id, difference
    );

    // Find all active events for this guild that include this stat.
    let stats: Vec<DbEventStat> = sqlx::query_as::<_, DbEventStat>(
        "SELECT es.*
         FROM event_stats es
         JOIN events e ON e.id = es.event_id
         WHERE e.guild_id = $1
           AND e.status = 'active'
           AND es.stat_name = $2",
    )
    .bind(guild_id)
    .bind(stat_name)
    .fetch_all(pool)
    .await?;

    if stats.is_empty() {
        return Ok(0.0);
    }

    let units = difference.round() as i32;
    if units <= 0 {
        return Ok(0.0);
    }

    let mut total_xp = 0.0_f64;

    for es in &stats {
        let xp_earned = es.xp_per_unit * (units as f64);

        sqlx::query(
            "INSERT INTO event_xp
                 (event_id, user_id, stat_name, delta_id, units, xp_per_unit, xp_earned, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (event_id, delta_id) DO NOTHING",
        )
        .bind(es.event_id)
        .bind(user_id)
        .bind(stat_name)
        .bind(delta_id)
        .bind(units)
        .bind(es.xp_per_unit)
        .bind(xp_earned)
        .bind(now)
        .execute(pool)
        .await?;

        total_xp += xp_earned;
    }

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
        "SELECT u.discord_user_id,
                u.minecraft_username,
                u.minecraft_uuid,
                u.hypixel_rank,
                u.hypixel_rank_plus_color,
                COALESCE(SUM(ex.xp_earned), 0.0) AS total_event_xp
         FROM event_xp ex
         JOIN users u ON u.id = ex.user_id
         WHERE ex.event_id = $1
         GROUP BY u.discord_user_id, u.minecraft_username, u.minecraft_uuid,
                  u.hypixel_rank, u.hypixel_rank_plus_color
         ORDER BY total_event_xp DESC
         LIMIT $2 OFFSET $3",
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
) -> Result<Vec<(String, f64)>, sqlx::Error> {
    debug!(
        "queries::get_user_event_stats: event_id={}, user_id={}",
        event_id, user_id
    );
    let rows = sqlx::query_as::<_, (String, f64)>(
        "SELECT stat_name, SUM(xp_earned) AS total_xp
         FROM event_xp
         WHERE event_id = $1 AND user_id = $2
         GROUP BY stat_name
         ORDER BY total_xp DESC",
    )
    .bind(event_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
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
        "SELECT rank FROM (
             SELECT user_id, RANK() OVER (ORDER BY SUM(xp_earned) DESC) AS rank
             FROM event_xp
             WHERE event_id = $1
             GROUP BY user_id
         ) sub
         WHERE user_id = $2",
    )
    .bind(event_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

// =========================================================================
// persistent_event_leaderboards
// =========================================================================

/// Count participants (distinct users) for a given event.
pub async fn count_event_participants(pool: &PgPool, event_id: i64) -> Result<i64, sqlx::Error> {
    debug!("queries::count_event_participants: event_id={}", event_id);
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(DISTINCT user_id) FROM event_xp WHERE event_id = $1")
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
    created_at: &DateTime<Utc>,
    last_updated: &DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    debug!(
        "queries::upsert_persistent_event_leaderboard: event_id={}, guild_id={}, channel_id={}",
        event_id, guild_id, channel_id
    );
    sqlx::query(
        "INSERT INTO persistent_event_leaderboards
         (event_id, guild_id, channel_id, message_ids, status_message_id, created_at, last_updated)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT(event_id) DO UPDATE SET
             guild_id = excluded.guild_id,
             channel_id = excluded.channel_id,
             message_ids = excluded.message_ids,
             status_message_id = excluded.status_message_id,
             created_at = excluded.created_at,
             last_updated = excluded.last_updated",
    )
    .bind(event_id)
    .bind(guild_id)
    .bind(channel_id)
    .bind(message_ids)
    .bind(status_message_id)
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

        let units = delta.delta.round() as i32;
        if units <= 0 {
            continue;
        }

        let xp_earned = es.xp_per_unit * units as f64;

        let row: Option<(f64,)> = sqlx::query_as(
            "INSERT INTO event_xp
                 (event_id, user_id, stat_name, delta_id, units, xp_per_unit, xp_earned, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
             ON CONFLICT (event_id, delta_id) DO NOTHING
             RETURNING xp_earned",
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
