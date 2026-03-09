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
use tracing::debug;
use uuid::Uuid;

use super::models::{
    DbDailySnapshot, DbGuild, DbMilestone, DbPersistentLeaderboard, DbStatDelta, DbStatsSnapshot,
    DbSweepCursor, DbUser, DbXP, LeaderboardEntry, MilestoneWithCount,
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
         LIMIT $2"
    )
    .bind(cutoff)
    .bind(limit)
    .fetch_all(pool)
    .await
}

// ========================================================================
// Daily snapshots
// =======================================================================

pub async fn insert_daily_snapshot(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO daily_snapshots (user_id, stat_name, stat_value, snapshot_date)
        SELECT user_id, stat_name, stat_value, CURRENT_DATE
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
    .execute(pool)
    .await?;

    Ok(())
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
