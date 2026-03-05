/// Database row models.
///
/// Each struct maps 1-to-1 to a database table and derives `sqlx::FromRow`
/// so that query results can be deserialized automatically.
///
/// Fields are intentionally public so consuming code can access any column.
use sqlx::FromRow;

// ---------------------------------------------------------------------------
// guilds
// ---------------------------------------------------------------------------

/// A row from the `guilds` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbGuild {
    pub guild_id: i64,
    pub registered_role_id: Option<i64>,
    pub config_json: String,
}

// ---------------------------------------------------------------------------
// users
// ---------------------------------------------------------------------------

/// A row from the `users` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbUser {
    pub id: i64,
    pub discord_user_id: i64,
    pub minecraft_uuid: String,
    /// Minecraft display name stored at registration time. `None` for rows that
    /// pre-date migration 002.
    pub minecraft_username: Option<String>,
    pub guild_id: i64,
    pub registered_at: String,
}

// ---------------------------------------------------------------------------
// hypixel_stats_snapshot / discord_stats_snapshot
// ---------------------------------------------------------------------------

/// A single stat snapshot row. Used for both `hypixel_stats_snapshot` and
/// `discord_stats_snapshot` since they share the same schema.
#[derive(Debug, Clone, FromRow)]
pub struct DbStatsSnapshot {
    pub id: i64,
    pub user_id: i64,
    pub stat_name: String,
    pub stat_value: f64,
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// xp
// ---------------------------------------------------------------------------

/// A row from the `xp` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbXP {
    pub user_id: i64,
    pub total_xp: f64,
    pub level: i64,
    pub last_updated: String,
}

// ---------------------------------------------------------------------------
// sweep_cursor
// ---------------------------------------------------------------------------

/// A row from the `sweep_cursor` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbSweepCursor {
    pub user_id: i64,
    pub source: String,
    pub stat_name: String,
    pub stat_value: f64,
    pub last_snapshot_ts: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// persistent_leaderboards
// ---------------------------------------------------------------------------

/// A row from the `persistent_leaderboards` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbPersistentLeaderboard {
    pub guild_id: i64,
    pub channel_id: i64,
    /// JSON array of Discord message IDs (one per page).
    pub message_ids: String,
    pub status_message_id: i64,
    pub created_at: String,
    pub last_updated: String,
}

// ---------------------------------------------------------------------------
// Leaderboard entry (query result, not a table)
// ---------------------------------------------------------------------------

/// A single leaderboard row returned by the ranking query.
/// Combines user info with their XP data.
#[derive(Debug, Clone, FromRow)]
pub struct LeaderboardEntry {
    pub discord_user_id: i64,
    pub minecraft_username: Option<String>,
    pub minecraft_uuid: String,
    pub total_xp: f64,
    pub level: i64,
}
