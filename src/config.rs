/// Application and guild configuration.
///
/// - `AppConfig` is loaded once from environment variables at startup.
/// - `GuildConfig` is stored as JSON in the `guilds` table and can be modified
///   at runtime by server admins. It controls which stats contribute to points
///   and how many points each stat awards.
use std::collections::HashMap;
use std::env;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Application-level config (from .env)
// ---------------------------------------------------------------------------

/// Top-level configuration sourced from environment variables.
///
/// This is constructed once during startup and stored in the shared `Data`
/// struct so every command and background task has access.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Discord bot token.
    pub discord_token: String,

    /// Hypixel API key for fetching player stats.
    pub hypixel_api_key: String,

    /// SQLx database connection string (e.g. `sqlite:bot.db`).
    pub database_url: String,

    /// How often the stat sweeper runs, in seconds.
    pub sweep_interval_seconds: u64,
}

impl AppConfig {
    /// Load configuration from environment variables.
    ///
    /// Panics with a descriptive message if any required variable is missing.
    pub fn from_env() -> Self {
        Self {
            discord_token: env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN must be set in .env"),
            hypixel_api_key: env::var("HYPIXEL_API_KEY")
                .expect("HYPIXEL_API_KEY must be set in .env"),
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:bot.db".to_string()),
            sweep_interval_seconds: env::var("SWEEP_INTERVAL_SECONDS")
                .unwrap_or_else(|_| "300".to_string())
                .parse()
                .expect("SWEEP_INTERVAL_SECONDS must be a valid u64"),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-guild config (stored as JSON in the database)
// ---------------------------------------------------------------------------

/// Configuration for a single Discord guild, stored as JSON in `guilds.config_json`.
///
/// New fields can be added freely — just give them a `#[serde(default)]`
/// attribute so that existing rows deserialize without error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildConfig {
    /// The role to assign to users when they register.
    /// `None` means no role assignment.
    #[serde(default)]
    pub registered_role_id: Option<u64>,

    /// Maps stat names to the number of points each unit of that stat awards.
    ///
    /// Example: `{ "wins": 10, "kills": 1, "beds_broken": 5 }`
    ///
    /// Stats not present in this map award zero points.
    #[serde(default = "default_points_config")]
    pub points_config: HashMap<String, f64>,

    /// Whether Discord activity stats (messages, reactions, etc.) should
    /// contribute to points. Defaults to `false`.
    #[serde(default)]
    pub discord_stats_enabled: bool,
}

impl Default for GuildConfig {
    fn default() -> Self {
        Self {
            registered_role_id: None,
            points_config: default_points_config(),
            discord_stats_enabled: false,
        }
    }
}

/// The default points configuration used when a guild has not customized theirs.
fn default_points_config() -> HashMap<String, f64> {
    let mut map = HashMap::new();
    map.insert("wins".to_string(), 10.0);
    map.insert("kills".to_string(), 1.0);
    map.insert("beds_broken".to_string(), 5.0);
    map
}
