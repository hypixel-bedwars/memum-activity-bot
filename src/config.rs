/// Application and guild configuration.
///
/// - `AppConfig` is loaded once from environment variables at startup.
/// - `GuildConfig` is stored as JSON in the `guilds` table and can be modified
///   at runtime by server admins. It controls which stats contribute to XP
///   and how much XP each stat awards.
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

    /// Base XP required to reach level 2. Higher levels scale exponentially.
    pub base_level_xp: f64,

    /// Exponential scaling factor for the leveling curve.
    pub level_exponent: f64,
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
            base_level_xp: env::var("BASE_LEVEL_XP")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .expect("BASE_LEVEL_XP must be a valid f64"),
            level_exponent: env::var("LEVEL_EXPONENT")
                .unwrap_or_else(|_| "1.5".to_string())
                .parse()
                .expect("LEVEL_EXPONENT must be a valid f64"),
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

    /// XP rewards per stat (per unit increase).
    ///
    /// Example: `{ "wins": 50.0, "kills": 5.0, "beds_broken": 25.0 }`
    ///
    /// Stats not present in this map award zero XP.
    #[serde(default = "default_xp_config")]
    pub xp_config: HashMap<String, f64>,

    /// Whether Discord activity stats (messages, reactions, etc.) should
    /// contribute to XP. Defaults to `false`.
    #[serde(default)]
    pub discord_stats_enabled: bool,
}

impl Default for GuildConfig {
    fn default() -> Self {
        Self {
            registered_role_id: None,
            xp_config: default_xp_config(),
            discord_stats_enabled: false,
        }
    }
}

/// The default XP configuration used when a guild has not customized theirs.
fn default_xp_config() -> HashMap<String, f64> {
    let mut map = HashMap::new();
    map.insert("wins".to_string(), 50.0);
    map.insert("kills".to_string(), 5.0);
    map.insert("beds_broken".to_string(), 25.0);
    map.insert("messages_sent".to_string(), 1.0);
    map.insert("reactions_added".to_string(), 1.0);
    map.insert("commands_used".to_string(), 2.0);
    map
}
