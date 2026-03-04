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
// points
// ---------------------------------------------------------------------------

/// A row from the `points` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbPoints {
    pub user_id: i64,
    pub total_points: f64,
    pub last_updated: String,
}
