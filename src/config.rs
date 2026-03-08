/// Application and guild configuration.
///
/// - `AppConfig` is loaded once from environment variables at startup.
/// - `GuildConfig` is stored as JSON in the `guilds` table and can be modified
///   at runtime by server admins. It controls which stats contribute to XP
///   and how much XP each stat awards.
use std::collections::HashMap;
use std::env;

use serde::{Deserialize, Serialize};

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

    /// SQLx database connection string (e.g. `postgres://user:pass@localhost/db`).
    pub database_url: String,

    /// How often the Hypixel stat sweeper runs, in seconds.
    pub hypixel_sweep_interval_seconds: u64,

    /// Base XP required to reach level 2. Higher levels scale exponentially.
    pub base_level_xp: f64,

    /// Exponential scaling factor for the leveling curve.
    pub level_exponent: f64,

    /// Discord role IDs that grant admin access to bot commands.
    /// Parsed from `ADMIN_ROLE_IDS` (comma-separated role snowflakes).
    pub admin_role_ids: Vec<u64>,

    /// How long leaderboard images are cached before regeneration, in seconds.
    /// Defaults to 60 if `LEADERBOARD_CACHE_SECONDS` is unset.
    pub leaderboard_cache_seconds: u64,

    /// Number of players shown in persistent leaderboards. Each page shows 10,
    /// so this controls how many pages the persistent message cycles through.
    /// Defaults to 10 if `PERSISTENT_LEADERBOARD_PLAYERS` is unset.
    pub persistent_leaderboard_players: u64,

    pub min_message_length: u64,

    pub message_cooldown_seconds: u64,

    /// Minimum time between Hypixel API refreshes for a single user, in seconds.
    ///
    /// Commands such as `/level` and `/stats` will only trigger a live Hypixel
    /// fetch if the user's `last_hypixel_refresh` is older than this value.
    /// Defaults to 60 if `HYPIXEL_REFRESH_COOLDOWN_SECONDS` is unset.
    pub hypixel_refresh_cooldown_seconds: u64,
    
    pub enable_hypixel_sweeper: bool,
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
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://user:pass@localhost/db".to_string()),
            hypixel_sweep_interval_seconds: env::var("HYPIXEL_SWEEP_INTERVAL_SECONDS")
                .unwrap_or_else(|_| "60".to_string())
                .parse()
                .expect("HYPIXEL_SWEEP_INTERVAL_SECONDS must be a valid u64"),
            base_level_xp: env::var("BASE_LEVEL_XP")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .expect("BASE_LEVEL_XP must be a valid f64"),
            level_exponent: env::var("LEVEL_EXPONENT")
                .unwrap_or_else(|_| "1.5".to_string())
                .parse()
                .expect("LEVEL_EXPONENT must be a valid f64"),
            admin_role_ids: env::var("ADMIN_ROLE_IDS")
                .unwrap_or_default()
                .split(',')
                .filter_map(|s| s.trim().parse::<u64>().ok())
                .collect(),
            leaderboard_cache_seconds: env::var("LEADERBOARD_CACHE_SECONDS")
                .unwrap_or_else(|_| "60".to_string())
                .parse()
                .expect("LEADERBOARD_CACHE_SECONDS must be a valid u64"),
            persistent_leaderboard_players: env::var("PERSISTENT_LEADERBOARD_PLAYERS")
                .unwrap_or_else(|_| "10".to_string())
                .parse()
                .expect("PERSISTENT_LEADERBOARD_PLAYERS must be a valid u64"),
            min_message_length: env::var("MIN_MESSAGE_LENGTH")
                .unwrap_or_else(|_| "5".to_string())
                .parse()
                .expect("MIN_MESSAGE_LENGTH must be a valid u64"),
            message_cooldown_seconds: env::var("MESSAGE_COOLDOWN_SECONDS")
                .unwrap_or_else(|_| "30".to_string())
                .parse()
                .expect("MESSAGE_COOLDOWN_SECONDS must be a valid u64"),
            hypixel_refresh_cooldown_seconds: env::var("HYPIXEL_REFRESH_COOLDOWN_SECONDS")
                .unwrap_or_else(|_| "60".to_string())
                .parse()
                .expect("HYPIXEL_REFRESH_COOLDOWN_SECONDS must be a valid u64"),
            enable_hypixel_sweeper: env::var("ENABLE_HYPIXEL_SWEEPER")
				.unwrap_or_else(|_| "false".to_string())
				.parse()
				.expect("ENABLE_HYPIXEL_SWEEPER must be a valid bool"),
        }
    }
}

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

    /// Role ID that gates nickname-based auto-registration via the Register
    /// button. When `Some`, users who possess this role may register by
    /// pressing the button — the bot reads their nickname and extracts the
    /// Minecraft username automatically. When `None` (the default) the
    /// feature is disabled and users must use `/register <username>`.
    #[serde(default)]
    pub nickname_registration_role_id: Option<u64>,
}

impl Default for GuildConfig {
    fn default() -> Self {
        Self {
            registered_role_id: None,
            xp_config: default_xp_config(),
            discord_stats_enabled: false,
            nickname_registration_role_id: None,
        }
    }
}

/// The default XP configuration used when a guild has not customized theirs.
///
/// Keys use raw Hypixel API stat names. Discord stats (`messages_sent`,
/// `reactions_added`, `commands_used`) are intentionally excluded — admins
/// must opt in via `/edit-stats add`.
fn default_xp_config() -> HashMap<String, f64> {
    let mut map = HashMap::new();
    map.insert("wins_bedwars".to_string(), 50.0);
    map.insert("kills_bedwars".to_string(), 5.0);
    map.insert("beds_broken_bedwars".to_string(), 25.0);
    map
}
