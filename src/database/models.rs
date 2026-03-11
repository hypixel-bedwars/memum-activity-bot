use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
/// Database row models.
///
/// Each struct maps 1-to-1 to a database table and derives `sqlx::FromRow`
/// so that query results can be deserialized automatically.
///
/// Fields are intentionally public so consuming code can access any column.
use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// guilds
// ---------------------------------------------------------------------------

/// A row from the `guilds` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbGuild {
    pub guild_id: i64,
    pub registered_role_id: Option<i64>,
    /// Optional Discord channel ID configured to receive guild-level logs.
    pub log_channel_id: Option<i64>,
    pub config_json: Value,
}

impl DbGuild {
    /// Return the configured logging channel, if any.
    pub fn log_channel(&self) -> Option<i64> {
        self.log_channel_id
    }

    /// Set or clear the logging channel on this in-memory model.
    ///
    /// Note: this only mutates the struct. Persisting to the database must be
    /// done via the appropriate query function (e.g. update_guild_config or a
    /// dedicated query that updates the `guilds` table).
    pub fn set_log_channel(&mut self, channel_id: Option<i64>) {
        self.log_channel_id = channel_id;
    }
}

// ---------------------------------------------------------------------------
// users
// ---------------------------------------------------------------------------

/// A row from the `users` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbUser {
    pub id: i64,
    pub discord_user_id: i64,
    pub minecraft_uuid: Uuid,
    /// Minecraft display name stored at registration time. `None` for rows that
    /// pre-date migration 002.
    pub minecraft_username: Option<String>,
    pub guild_id: i64,
    pub registered_at: DateTime<Utc>,

    // Optional cached head texture (base64 data URL or raw encoded PNG). New column.
    pub head_texture: Option<String>,
    // RFC3339 timestamp of when head_texture was last updated.
    pub head_texture_updated_at: Option<DateTime<Utc>>,

    /// Timestamp of the most recent successful Hypixel API fetch for this user.
    /// `None` means the user has never been swept.
    pub last_hypixel_refresh: Option<DateTime<Utc>>,

    /// Timestamp of the most recent stat command (/level, /stats) used by this
    /// user in this guild. `None` means no stat command has been used since
    /// migration 008.
    pub last_command_activity: Option<DateTime<Utc>>,

    /// The player's Hypixel rank package as a raw string (e.g. `"VIP"`,
    /// `"VIP_PLUS"`, `"MVP"`, `"MVP_PLUS"`, `"SUPERSTAR"` for MVP++).
    /// `None` means either no rank or not yet fetched (pre-migration 009).
    pub hypixel_rank: Option<String>,

    /// The colour of the `+` symbol in the player's rank badge, as returned
    /// by the Hypixel API's `rankPlusColor` field (e.g. `"RED"`, `"GOLD"`,
    /// `"DARK_GREEN"`). Only meaningful for MVP+ and MVP++; `None` otherwise.
    pub hypixel_rank_plus_color: Option<String>,
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
    pub timestamp: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// xp
// ---------------------------------------------------------------------------

/// A row from the `xp` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbXP {
    pub user_id: i64,
    pub total_xp: f64,
    pub level: i32,
    pub last_updated: DateTime<Utc>,
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
    pub last_snapshot_ts: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
    pub message_ids: Value,
    pub status_message_id: i64,
    /// Discord message ID of the separate milestone card message.
    /// `0` means no milestone message has been sent yet.
    pub milestone_message_id: i64,
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
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
    pub minecraft_uuid: Uuid,
    pub total_xp: f64,
    pub level: i32,
    /// The player's Hypixel rank package string (e.g. `"VIP"`, `"MVP_PLUS"`, `"SUPERSTAR"`).
    pub hypixel_rank: Option<String>,
    /// The colour of the `+` symbol in the player's rank badge (e.g. `"GOLD"`, `"RED"`).
    pub hypixel_rank_plus_color: Option<String>,
}

// ---------------------------------------------------------------------------
// milestones
// ---------------------------------------------------------------------------

/// A row from the `milestones` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbMilestone {
    pub id: i64,
    pub guild_id: i64,
    /// The level threshold that defines this milestone.
    pub level: i32,
}

/// A milestone row joined with the count of users who have reached it.
/// Returned by the `get_milestones_with_counts` query.
#[derive(Debug, Clone, FromRow)]
pub struct MilestoneWithCount {
    pub id: i64,
    pub guild_id: i64,
    pub level: i32,
    /// Number of users in this guild whose level is >= this milestone's level.
    pub user_count: i64,
}

// ---------------------------------------------------------------------------
// stat_deltas
// ---------------------------------------------------------------------------

/// A row from the `stat_deltas` table.
///
/// Inserted once per positive stat change detected by a sweeper. Immutable
/// after creation — never updated.
#[derive(Debug, Clone, FromRow)]
pub struct DbStatDelta {
    pub id: i64,
    pub user_id: i64,
    pub stat_name: String,
    pub old_value: f64,
    pub new_value: f64,
    pub delta: f64,
    /// The sweeper source that produced this delta (e.g. `"hypixel"`, `"discord"`).
    pub source: String,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// xp_events
// ---------------------------------------------------------------------------

/// A row from the `xp_events` table.
///
/// Records exactly how much XP was awarded for a single `stat_deltas` row,
/// including the multiplier that was active at the time. Immutable after
/// creation — admin edits to guild multipliers do not affect historical rows.
#[derive(Debug, Clone, FromRow)]
pub struct DbXPEvent {
    pub id: i64,
    pub user_id: i64,
    pub stat_name: String,
    /// FK → `stat_deltas.id`.
    pub delta_id: i64,
    pub units: i32,
    pub xp_per_unit: f64,
    pub xp_earned: f64,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Message Validation
// ---------------------------------------------------------------------------

// Note for future self: Right now your cooldown is per user globally, so if you wanna do this for
// multiple guilds you might want to change the key to (user_id, guild_id) or something like that.
pub struct MessageValidationState {
    pub last_counted: Arc<Mutex<HashMap<i64, OffsetDateTime>>>,
    pub last_message: Arc<Mutex<HashMap<i64, String>>>,
}

impl Clone for MessageValidationState {
    fn clone(&self) -> Self {
        Self {
            last_counted: Arc::clone(&self.last_counted),
            last_message: Arc::clone(&self.last_message),
        }
    }
}

impl Default for MessageValidationState {
    fn default() -> Self {
        Self {
            last_counted: Arc::new(Mutex::new(HashMap::new())),
            last_message: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// ---------------------------------------------------------------------------
// Daily snapshots
// ---------------------------------------------------------------------------
//
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DbDailySnapshot {
    pub user_id: i64,
    pub stat_name: String,
    pub stat_value: f64,
    pub snapshot_date: NaiveDate,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// events
// ---------------------------------------------------------------------------

/// A row from the `events` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbEvent {
    pub id: i64,
    pub guild_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub start_date: DateTime<Utc>,
    pub end_date: DateTime<Utc>,
    pub start_snapshot_date: Option<NaiveDate>,
    pub end_snapshot_date: Option<NaiveDate>,
    /// One of `"pending"`, `"active"`, or `"ended"`.
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// event_stats
// ---------------------------------------------------------------------------

/// A row from the `event_stats` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbEventStat {
    pub id: i64,
    pub event_id: i64,
    pub stat_name: String,
    pub xp_per_unit: f64,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// event_xp
// ---------------------------------------------------------------------------

/// A row from the `event_xp` table.
#[derive(Debug, Clone, FromRow)]
pub struct DbEventXP {
    pub id: i64,
    pub event_id: i64,
    pub user_id: i64,
    pub stat_name: String,
    pub delta_id: Option<i64>,
    pub units: i32,
    pub xp_per_unit: f64,
    pub xp_earned: f64,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Event leaderboard entry (query result, not a table)
// ---------------------------------------------------------------------------

/// A single entry in an event leaderboard — user plus their total event XP.
#[derive(Debug, Clone, FromRow)]
pub struct EventLeaderboardEntry {
    pub discord_user_id: i64,
    pub minecraft_username: Option<String>,
    pub total_event_xp: f64,
}

// ---------------------------------------------------------------------------
// Voice session state
// ---------------------------------------------------------------------------

/// In-memory map from `discord_user_id` to the UTC timestamp when they
/// joined a voice channel. Used to compute voice_minutes on leave.
pub type VoiceSessionState = Arc<Mutex<HashMap<i64, DateTime<Utc>>>>;

// ---------------------------------------------------------------------------
// Backfill
// ---------------------------------------------------------------------------

/// Summary returned after a retroactive event XP backfill completes.
#[derive(Debug, Clone, Default)]
pub struct BackfillSummary {
    pub deltas_processed: i64,
    pub total_xp_awarded: f64,
    pub users_affected: i64,
}
