/// Shared type definitions used across the entire bot.
///
/// This module defines the core `Data` struct that Poise passes to every command,
/// the common error type, the Poise context alias, and the `StatDelta` struct that
/// serves as the universal interface between stat sources and the XP calculator.
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use dashmap::DashMap;
use sqlx::PgPool;

use crate::commands::leaderboard::leaderboard::LeaderboardCache;
use crate::config::{AppConfig, GuildConfig};
use crate::database::models::{MessageValidationState, VoiceSessionState};
use crate::hypixel::client::HypixelClient;
use poise::serenity_prelude as serenity;

/// The error type used throughout the bot. We use a boxed trait object so that
/// any error type that implements `std::error::Error + Send + Sync` can be
/// propagated without manual conversion.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Convenience alias for the Poise context, parameterized with our `Data` and
/// `Error` types. Every slash-command handler receives this.
pub type Context<'a> = poise::Context<'a, Data, Error>;

/// Central data struct injected into every Poise command invocation.
///
/// This is constructed once during bot setup and then shared (via `Arc` internally
/// by Poise) for the lifetime of the process.
#[derive(Clone)]
pub struct Data {
    /// Postgres connection pool for all database operations.
    pub db: PgPool,

    /// Pre-configured Hypixel API client (with built-in cache and rate limiter).
    pub hypixel: Arc<HypixelClient>,

    /// Application-level configuration loaded from environment variables.
    pub config: AppConfig,

    /// Timed cache for leaderboard page images, keyed by `(guild_id, page)`.
    pub leaderboard_cache: LeaderboardCache,

    /// In-memory cache for guild configurations, keyed by `guild_id`.
    /// Each entry pairs the `GuildConfig` with the `Instant` it was cached so
    /// that the tracker can re-fetch after the TTL (see `GUILD_CONFIG_TTL` in
    /// `discord_stats/tracker.rs`) without needing a bot restart.
    pub guild_configs: DashMap<i64, (GuildConfig, Instant)>,

    /// State for message validation, used by the Discord activity tracker to determine
    /// if a message is valid for XP (e.g. not a bot command, not a duplicate, etc.).
    pub message_validation: MessageValidationState,

    /// In-memory voice session tracker — maps discord_user_id to the UTC time
    /// they joined a voice channel. Populated on VoiceStateUpdate join events,
    /// consumed on leave events to compute `voice_minutes`.
    pub voice_sessions: VoiceSessionState,

    /// Discord HTTP client for sending messages outside command contexts.
    pub http: Arc<serenity::Http>,

    /// Set to `true` while `run_full_hypixel_sweep` is in progress.
    ///
    /// Shared across the event sweep scheduler and the regular stale sweep so
    /// that neither runs concurrently with a full sweep. Use
    /// `AtomicBool::swap` to claim the flag atomically before spawning a
    /// sweep, and `store(false)` once the sweep finishes.
    pub is_full_sweep_running: Arc<AtomicBool>,
}

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
    pub old_value: i64,

    /// The current value of the stat.
    pub new_value: i64,

    /// The computed difference (`new_value - old_value`).
    pub difference: i64,
}

impl StatDelta {
    /// Create a new `StatDelta`, automatically computing the difference.
    pub fn new(user_id: i64, stat_name: String, old_value: i64, new_value: i64) -> Self {
        Self {
            user_id,
            stat_name,
            old_value,
            new_value,
            difference: new_value - old_value,
        }
    }
}
