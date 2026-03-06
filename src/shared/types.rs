/// Shared type definitions used across the entire bot.
///
/// This module defines the core `Data` struct that Poise passes to every command,
/// the common error type, the Poise context alias, and the `StatDelta` struct that
/// serves as the universal interface between stat sources and the XP calculator.
use std::sync::Arc;

use sqlx::PgPool;

use crate::commands::leaderboard::leaderboard::LeaderboardCache;
use crate::config::AppConfig;
use crate::database::models::MessageValidationState;
use crate::hypixel::client::HypixelClient;

// ---------------------------------------------------------------------------
// Poise type aliases
// ---------------------------------------------------------------------------

/// The error type used throughout the bot. We use a boxed trait object so that
/// any error type that implements `std::error::Error + Send + Sync` can be
/// propagated without manual conversion.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Convenience alias for the Poise context, parameterized with our `Data` and
/// `Error` types. Every slash-command handler receives this.
pub type Context<'a> = poise::Context<'a, Data, Error>;

// ---------------------------------------------------------------------------
// Shared application data
// ---------------------------------------------------------------------------

/// Central data struct injected into every Poise command invocation.
///
/// This is constructed once during bot setup and then shared (via `Arc` internally
/// by Poise) for the lifetime of the process.
pub struct Data {
    /// SQLite connection pool for all database operations.
    pub db: PgPool,

    /// Pre-configured Hypixel API client (with built-in cache and rate limiter).
    pub hypixel: Arc<HypixelClient>,

    /// Application-level configuration loaded from environment variables.
    pub config: AppConfig,

    /// Timed cache for leaderboard page images, keyed by `(guild_id, page)`.
    pub leaderboard_cache: LeaderboardCache,
    
    /// State for message validation, used by the Discord activity tracker to determine
    /// if a message is valid for XP (e.g. not a bot command, not a duplicate, etc.).
    pub message_validation: MessageValidationState,
}

// ---------------------------------------------------------------------------
// StatDelta — universal stat-change representation
// ---------------------------------------------------------------------------

/// Represents a change in a single stat for a single user between two snapshots.
///
/// Both the Hypixel sweeper and the Discord activity tracker produce `StatDelta`
/// values. The points calculator consumes them uniformly regardless of source,
/// making it trivial to add new stat sources in the future.
#[derive(Debug, Clone)]
pub struct StatDelta {
    /// Internal database user id (from the `users` table).
    pub user_id: i64,

    /// Name of the stat (e.g. "wins", "kills", "messages_sent").
    pub stat_name: String,

    /// The previous value of the stat.
    pub old_value: f64,

    /// The current value of the stat.
    pub new_value: f64,

    /// The computed difference (`new_value - old_value`).
    pub difference: f64,
}

impl StatDelta {
    /// Create a new `StatDelta`, automatically computing the difference.
    pub fn new(user_id: i64, stat_name: String, old_value: f64, new_value: f64) -> Self {
        Self {
            user_id,
            stat_name,
            old_value,
            new_value,
            difference: new_value - old_value,
        }
    }
}
