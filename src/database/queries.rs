/// Database query functions.
///
/// All functions accept a `&SqlitePool` so they can be called from any context
/// that has access to the shared `Data` struct. Queries are organized by table.
///
/// Some functions are not yet called but exist as part of the public query API
/// for extensions and future commands.
use sqlx::SqlitePool;

use super::models::{DbGuild, DbPoints, DbStatsSnapshot, DbUser};

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

/// Register a new user. Uses `ON CONFLICT` to update the Minecraft UUID if the
/// user re-registers in the same guild.
pub async fn register_user(
    pool: &SqlitePool,
    discord_user_id: i64,
    minecraft_uuid: &str,
    guild_id: i64,
    registered_at: &str,
) -> Result<DbUser, sqlx::Error> {
    sqlx::query(
        "INSERT INTO users (discord_user_id, minecraft_uuid, guild_id, registered_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(discord_user_id, guild_id) DO UPDATE SET minecraft_uuid = excluded.minecraft_uuid",
    )
    .bind(discord_user_id)
    .bind(minecraft_uuid)
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
// points
// =========================================================================

/// Insert or update points for a user. Adds the given `points_to_add` to the
/// existing total (or creates a new row starting from zero).
pub async fn upsert_points(
    pool: &SqlitePool,
    user_id: i64,
    points_to_add: f64,
    timestamp: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO points (user_id, total_points, last_updated)
         VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET
             total_points = points.total_points + excluded.total_points,
             last_updated = excluded.last_updated",
    )
    .bind(user_id)
    .bind(points_to_add)
    .bind(timestamp)
    .execute(pool)
    .await?;
    Ok(())
}

/// Retrieve current points for a user, if they exist.
pub async fn get_points(pool: &SqlitePool, user_id: i64) -> Result<Option<DbPoints>, sqlx::Error> {
    sqlx::query_as::<_, DbPoints>("SELECT * FROM points WHERE user_id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await
}
