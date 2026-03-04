/// Hypixel API response models and internal stat representations.
///
/// The Hypixel API returns deeply nested JSON. We only deserialize the fields
/// we need and use `#[serde(default)]` liberally so missing fields don't cause
/// errors (not all players have all stats).
use std::collections::HashMap;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Mojang API
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Hypixel API response
// ---------------------------------------------------------------------------

/// Top-level response from `GET https://api.hypixel.net/v2/player?uuid={uuid}`.
#[derive(Debug, Deserialize)]
pub struct HypixelPlayerResponse {
    pub success: bool,
    pub player: Option<HypixelPlayer>,
}

/// The `player` object inside the Hypixel API response.
#[derive(Debug, Deserialize)]
pub struct HypixelPlayer {
    pub stats: Option<HypixelStats>,
}

/// Container for all game mode stats. We only care about Bedwars.
#[derive(Debug, Deserialize)]
pub struct HypixelStats {
    #[serde(rename = "Bedwars")]
    pub bedwars: Option<HypixelBedwarsRaw>,
}

/// Raw Bedwars stats as they appear in the Hypixel API response.
/// Field names match the API's key names exactly.
#[derive(Debug, Deserialize)]
pub struct HypixelBedwarsRaw {
    #[serde(default)]
    pub wins_bedwars: f64,
    #[serde(default)]
    pub kills_bedwars: f64,
    #[serde(default)]
    pub beds_broken_bedwars: f64,
    #[serde(default)]
    pub final_kills_bedwars: f64,
    #[serde(default)]
    pub deaths_bedwars: f64,
    #[serde(default)]
    pub losses_bedwars: f64,
    #[serde(default)]
    pub games_played_bedwars: f64,
}

// ---------------------------------------------------------------------------
// Internal representation
// ---------------------------------------------------------------------------

/// Cleaned-up Bedwars stats used internally.
///
/// Stats are stored as a `HashMap<String, f64>` so new stats can be added
/// dynamically (e.g. via the sweeper) without changing this struct. The named
/// accessor methods are conveniences for the most commonly used stats.
#[derive(Debug, Clone)]
pub struct BedwarsStats {
    /// Dynamic stat map. Keys use short names (e.g. "wins", "kills").
    pub stats: HashMap<String, f64>,
}

impl BedwarsStats {
    /// Build a `BedwarsStats` from the raw API response.
    pub fn from_raw(raw: &HypixelBedwarsRaw) -> Self {
        let mut stats = HashMap::new();
        stats.insert("wins".to_string(), raw.wins_bedwars);
        stats.insert("kills".to_string(), raw.kills_bedwars);
        stats.insert("beds_broken".to_string(), raw.beds_broken_bedwars);
        stats.insert("final_kills".to_string(), raw.final_kills_bedwars);
        stats.insert("deaths".to_string(), raw.deaths_bedwars);
        stats.insert("losses".to_string(), raw.losses_bedwars);
        stats.insert("games_played".to_string(), raw.games_played_bedwars);
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
        self.stats.get("wins").copied().unwrap_or(0.0)
    }

    pub fn kills(&self) -> f64 {
        self.stats.get("kills").copied().unwrap_or(0.0)
    }

    pub fn beds_broken(&self) -> f64 {
        self.stats.get("beds_broken").copied().unwrap_or(0.0)
    }
}
