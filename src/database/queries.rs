/// Database query functions.
///
/// All functions accept a `&SqlitePool` so they can be called from any context
/// that has access to the shared `Data` struct. Queries are organized by table.
///
/// Some functions are not yet called but exist as part of the public query API
/// for extensions and future commands.
use sqlx::{Sqlite, SqlitePool, Transaction};

use super::models::{DbGuild, DbStatsSnapshot, DbSweepCursor, DbUser, DbXP};

// =========================================================================
// guilds
// =========================================================================

/// Insert a guild row if it does not already exist. If the guild already exists,
/// this is a no-op (the existing row is preserved).
pub async fn upsert_guild(pool: &SqlitePool, guild_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO guilds (guild_id) VALUES (?) ON CONFLICT(guild_id) DO NOTHING")
        .bind(guild_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Retrieve a guild row by its Discord snowflake.
pub async fn get_guild(pool: &SqlitePool, guild_id: i64) -> Result<Option<DbGuild>, sqlx::Error> {
    sqlx::query_as::<_, DbGuild>("SELECT * FROM guilds WHERE guild_id = ?")
        .bind(guild_id)
        .fetch_optional(pool)
        .await
}

/// Update the `config_json` column for a guild.
pub async fn update_guild_config(
    pool: &SqlitePool,
    guild_id: i64,
    config_json: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE guilds SET config_json = ? WHERE guild_id = ?")
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
    pool: &SqlitePool,
    discord_user_id: i64,
    minecraft_uuid: &str,
    minecraft_username: &str,
    guild_id: i64,
    registered_at: &str,
) -> Result<DbUser, sqlx::Error> {
    sqlx::query(
        "INSERT INTO users (discord_user_id, minecraft_uuid, minecraft_username, guild_id, registered_at)
         VALUES (?, ?, ?, ?, ?)
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
    sqlx::query_as::<_, DbUser>("SELECT * FROM users WHERE discord_user_id = ? AND guild_id = ?")
        .bind(discord_user_id)
        .bind(guild_id)
        .fetch_one(pool)
        .await
}

/// Get the most recent Hypixel stat snapshot for a user with a timestamp
/// strictly before `before_ts`. Used to compute "since last sweep" deltas.
pub async fn get_hypixel_snapshot_before(
    pool: &SqlitePool,
    user_id: i64,
    stat_name: &str,
    before_ts: &str,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM hypixel_stats_snapshot
         WHERE user_id = ? AND stat_name = ? AND timestamp < ?
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
    pool: &SqlitePool,
    user_id: i64,
    stat_name: &str,
    before_ts: &str,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM discord_stats_snapshot
         WHERE user_id = ? AND stat_name = ? AND timestamp < ?
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
    pool: &SqlitePool,
    user_id: i64,
    stat_name: &str,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM hypixel_stats_snapshot
         WHERE user_id = ? AND stat_name = ?
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
    pool: &SqlitePool,
    user_id: i64,
    stat_name: &str,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM discord_stats_snapshot
         WHERE user_id = ? AND stat_name = ?
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
    pool: &SqlitePool,
    discord_user_id: i64,
    guild_id: i64,
) -> Result<Option<DbUser>, sqlx::Error> {
    sqlx::query_as::<_, DbUser>("SELECT * FROM users WHERE discord_user_id = ? AND guild_id = ?")
        .bind(discord_user_id)
        .bind(guild_id)
        .fetch_optional(pool)
        .await
}

/// Get all registered users across every guild. Used by the sweeper.
pub async fn get_all_registered_users(pool: &SqlitePool) -> Result<Vec<DbUser>, sqlx::Error> {
    sqlx::query_as::<_, DbUser>("SELECT * FROM users")
        .fetch_all(pool)
        .await
}

/// Unregister a user by deleting their row from the database.
pub async fn unregister_user(
    pool: &SqlitePool,
    discord_user_id: i64,
    guild_id: i64,
) -> Result<(), sqlx::Error> {
    // Get the internal user id
    let user_id: Option<i64> =
        sqlx::query_scalar("SELECT id FROM users WHERE discord_user_id = ? AND guild_id = ?")
            .bind(discord_user_id)
            .bind(guild_id)
            .fetch_optional(pool)
            .await?;

    if let Some(uid) = user_id {
        // delete dependent rows first
        sqlx::query("DELETE FROM hypixel_stats_snapshot WHERE user_id = ?")
            .bind(uid)
            .execute(pool)
            .await?;

        sqlx::query("DELETE FROM discord_stats_snapshot WHERE user_id = ?")
            .bind(uid)
            .execute(pool)
            .await?;

        sqlx::query("DELETE FROM xp WHERE user_id = ?")
            .bind(uid)
            .execute(pool)
            .await?;

        sqlx::query("DELETE FROM sweep_cursor WHERE user_id = ?")
            .bind(uid)
            .execute(pool)
            .await?;

        // now delete the user
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(uid)
            .execute(pool)
            .await?;
    }

    Ok(())
}

/// Get all registered users within a specific guild.
pub async fn get_all_users_in_guild(
    pool: &SqlitePool,
    guild_id: i64,
) -> Result<Vec<DbUser>, sqlx::Error> {
    sqlx::query_as::<_, DbUser>("SELECT * FROM users WHERE guild_id = ?")
        .bind(guild_id)
        .fetch_all(pool)
        .await
}

// =========================================================================
// hypixel_stats_snapshot
// =========================================================================

/// Insert a new Hypixel stat snapshot row.
pub async fn insert_hypixel_snapshot(
    pool: &SqlitePool,
    user_id: i64,
    stat_name: &str,
    stat_value: f64,
    timestamp: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO hypixel_stats_snapshot (user_id, stat_name, stat_value, timestamp)
         VALUES (?, ?, ?, ?)",
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
    pool: &SqlitePool,
    user_id: i64,
    stat_name: &str,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM hypixel_stats_snapshot
         WHERE user_id = ? AND stat_name = ?
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
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<DbStatsSnapshot>, sqlx::Error> {
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT h.*
         FROM hypixel_stats_snapshot h
         INNER JOIN (
             SELECT user_id, stat_name, MAX(timestamp) AS max_ts
             FROM hypixel_stats_snapshot
             WHERE user_id = ?
             GROUP BY user_id, stat_name
         ) latest
         ON h.user_id = latest.user_id
            AND h.stat_name = latest.stat_name
            AND h.timestamp = latest.max_ts",
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
    pool: &SqlitePool,
    user_id: i64,
    stat_name: &str,
    stat_value: f64,
    timestamp: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO discord_stats_snapshot (user_id, stat_name, stat_value, timestamp)
         VALUES (?, ?, ?, ?)",
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
    pool: &SqlitePool,
    user_id: i64,
    stat_name: &str,
) -> Result<Option<DbStatsSnapshot>, sqlx::Error> {
    sqlx::query_as::<_, DbStatsSnapshot>(
        "SELECT * FROM discord_stats_snapshot
         WHERE user_id = ? AND stat_name = ?
         ORDER BY timestamp DESC
         LIMIT 1",
    )
    .bind(user_id)
    .bind(stat_name)
    .fetch_optional(pool)
    .await
}

// =========================================================================
// xp
// =========================================================================

/// Insert or update XP for a user. Adds the given `xp_to_add` to the
/// existing total (or creates a new row starting from zero).
pub async fn upsert_xp(
    pool: &SqlitePool,
    user_id: i64,
    xp_to_add: f64,
    timestamp: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO xp (user_id, total_xp, last_updated)
         VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET
             total_xp = xp.total_xp + excluded.total_xp,
             last_updated = excluded.last_updated",
    )
    .bind(user_id)
    .bind(xp_to_add)
    .bind(timestamp)
    .execute(pool)
    .await?;
    Ok(())
}

/// Set the XP total and level for a user (used after computing new totals).
pub async fn set_xp_and_level(
    pool: &SqlitePool,
    user_id: i64,
    total_xp: f64,
    level: i64,
    timestamp: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO xp (user_id, total_xp, level, last_updated)
         VALUES (?, ?, ?, ?)
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
pub async fn get_xp(pool: &SqlitePool, user_id: i64) -> Result<Option<DbXP>, sqlx::Error> {
    sqlx::query_as::<_, DbXP>("SELECT * FROM xp WHERE user_id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await
}

/// Delete a user's XP record (used when unregistering).
pub async fn delete_xp(pool: &SqlitePool, user_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM xp WHERE user_id = ?")
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
    pool: &SqlitePool,
    user_id: i64,
    source: &str,
    stat_name: &str,
) -> Result<Option<DbSweepCursor>, sqlx::Error> {
    sqlx::query_as::<_, DbSweepCursor>(
        "SELECT * FROM sweep_cursor
         WHERE user_id = ? AND source = ? AND stat_name = ?",
    )
    .bind(user_id)
    .bind(source)
    .bind(stat_name)
    .fetch_optional(pool)
    .await
}

/// Insert or update a sweep cursor row.
pub async fn upsert_sweep_cursor(
    pool: &SqlitePool,
    user_id: i64,
    source: &str,
    stat_name: &str,
    stat_value: f64,
    last_snapshot_ts: &str,
    updated_at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO sweep_cursor (user_id, source, stat_name, stat_value, last_snapshot_ts, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
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
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    source: &str,
    stat_name: &str,
    stat_value: f64,
    last_snapshot_ts: &str,
    updated_at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO sweep_cursor (user_id, source, stat_name, stat_value, last_snapshot_ts, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
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
