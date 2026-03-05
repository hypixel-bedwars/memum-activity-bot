// Hypixel API response models and internal stat representations.
//
// The Hypixel API returns deeply nested JSON. We only deserialize the fields
// we need and use `#[serde(default)]` liberally so missing fields don't cause
// errors (not all players have all stats).
use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

/// Response from the Mojang username-to-UUID endpoint.
///
/// `GET https://api.mojang.com/users/profiles/minecraft/{username}`
#[derive(Debug, Deserialize)]
pub struct MojangProfile {
    /// The player's UUID (no dashes).
    pub id: String,
    /// The player's current username.
    pub name: String,
}

/// Top-level response from `GET https://api.hypixel.net/v2/player?uuid={uuid}`.
#[derive(Debug, Deserialize)]
pub struct HypixelPlayerResponse {
    pub success: bool,
    pub player: Option<HypixelPlayer>,
}

/// The `player` object inside the Hypixel API response.
#[derive(Debug, Deserialize)]
pub struct HypixelPlayer {
    #[serde(default)]
    pub stats: Option<HypixelStats>,

    /// Social media block: API key is `socialMedia`.
    #[serde(rename = "socialMedia")]
    #[serde(default)]
    pub social_media: Option<HypixelSocialMedia>,
}

/// Social media block inside `player`.
#[derive(Debug, Deserialize)]
pub struct HypixelSocialMedia {
    #[serde(default)]
    pub links: HashMap<String, String>,

    // There is a `prompt` boolean in some responses; include it in case it's
    // useful later.
    #[serde(default)]
    pub prompt: bool,
}

/// Container for all game mode stats. We only care about Bedwars.
#[derive(Debug, Deserialize)]
pub struct HypixelStats {
    #[serde(rename = "Bedwars")]
    pub bedwars: Option<HypixelBedwarsRaw>,
}

/// Raw Bedwars stats as a flat key-value map straight from the Hypixel API.
///
/// Using a `HashMap<String, serde_json::Value>` instead of a typed struct means
/// any stat keys added by Hypixel are captured automatically without code changes.
/// Non-numeric fields (objects, arrays, strings, booleans) are filtered out in
/// `BedwarsStats::from_raw`.
#[derive(Debug, Deserialize)]
pub struct HypixelBedwarsRaw(pub HashMap<String, Value>);

/// Substrings that identify dream-mode stats (e.g. `_voidless_kills_bedwars`).
const DREAM_MODE_SUBSTRINGS: &[&str] = &[
    "_voidless_",
    "_lucky_",
    "_rush_",
    "_ultimate_",
    "_armed_",
    "_swap_",
];

/// Key prefixes that identify dream-mode stats (e.g. `castle_kills_bedwars`).
const DREAM_MODE_PREFIXES: &[&str] = &["castle_"];

/// Returns `true` if the stat key belongs to a dream mode and should be excluded.
fn is_dream_mode_stat(key: &str) -> bool {
    for sub in DREAM_MODE_SUBSTRINGS {
        if key.contains(sub) {
            return true;
        }
    }
    for prefix in DREAM_MODE_PREFIXES {
        if key.starts_with(prefix) {
            return true;
        }
    }
    false
}

/// Cleaned-up Bedwars stats used internally.
///
/// Stats are stored as a `HashMap<String, f64>` so new stats can be added
/// dynamically without changing this struct. Keys use raw API names
/// (e.g. `"wins_bedwars"`, `"kills_bedwars"`, `"beds_broken_bedwars"`).
#[derive(Debug, Clone)]
pub struct BedwarsStats {
    /// Dynamic stat map keyed by raw Hypixel API stat name.
    pub stats: HashMap<String, f64>,
}

impl BedwarsStats {
    /// Build a `BedwarsStats` from the raw API response.
    ///
    /// Only numeric values are kept (`Value::as_f64().is_some()`). Dream-mode
    /// stats are excluded. Keys retain their original API names.
    pub fn from_raw(raw: &HypixelBedwarsRaw) -> Self {
        let mut stats = HashMap::new();
        for (key, value) in &raw.0 {
            if is_dream_mode_stat(key) {
                continue;
            }
            if let Some(f) = value.as_f64() {
                stats.insert(key.clone(), f);
            }
        }
        Self { stats }
    }

    /// Return an empty stat set (used when a player has no Bedwars data).
    pub fn empty() -> Self {
        Self {
            stats: HashMap::new(),
        }
    }

    // Convenience accessors ------------------------------------------------

    pub fn wins(&self) -> f64 {
        self.stats.get("wins_bedwars").copied().unwrap_or(0.0)
    }

    pub fn kills(&self) -> f64 {
        self.stats.get("kills_bedwars").copied().unwrap_or(0.0)
    }

    pub fn beds_broken(&self) -> f64 {
        self.stats
            .get("beds_broken_bedwars")
            .copied()
            .unwrap_or(0.0)
    }
}

/// PlayerData is the compact internal structure returned by the Hypixel client
/// and stored in the TTL cache.
#[derive(Debug, Clone)]
pub struct PlayerData {
    pub bedwars: BedwarsStats,
    /// social links (e.g. "DISCORD" -> "va80_")
    pub social_links: HashMap<String, String>,
}
