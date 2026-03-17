// Hypixel API response models and internal stat representations.
//
// The Hypixel API returns deeply nested JSON. We only deserialize the fields
// we need and use `#[serde(default)]` liberally so missing fields don't cause
// errors (not all players have all stats).
use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

/// Response from the Mojang username-to-UUID endpoint.
///
/// `GET https://api.mojang.com/users/profiles/minecraft/{username}`
#[derive(Debug, Deserialize)]
pub struct MojangProfile {
    /// The player's UUID (no dashes).
    pub id: Uuid,
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

    /// The player's rank package, e.g. `"VIP"`, `"VIP_PLUS"`, `"MVP"`,
    /// `"MVP_PLUS"`. For MVP++ this field is absent and
    /// `monthly_package_rank` is `"SUPERSTAR"` instead.
    ///
    /// API key: `newPackageRank`
    #[serde(rename = "newPackageRank")]
    #[serde(default)]
    pub new_package_rank: Option<String>,

    /// Monthly rank — `"SUPERSTAR"` means the player has MVP++.
    ///
    /// API key: `monthlyPackageRank`
    #[serde(rename = "monthlyPackageRank")]
    #[serde(default)]
    pub monthly_package_rank: Option<String>,

    /// The colour of the `+` on the player's rank badge (e.g. `"RED"`,
    /// `"GOLD"`, `"DARK_GREEN"`). Only present for MVP+ / MVP++.
    ///
    /// API key: `rankPlusColor`
    #[serde(rename = "rankPlusColor")]
    #[serde(default)]
    pub rank_plus_color: Option<String>,
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
    pub stats: HashMap<String, i64>,
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
            if let Some(f) = value.as_i64() {
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

    // Convenience accessors

    pub fn wins(&self) -> i64 {
        self.stats.get("wins_bedwars").copied().unwrap_or(0)
    }

    pub fn kills(&self) -> i64 {
        self.stats.get("kills_bedwars").copied().unwrap_or(0)
    }

    pub fn beds_broken(&self) -> i64 {
        self.stats.get("beds_broken_bedwars").copied().unwrap_or(0)
    }
}

/// A player's Hypixel rank, normalised from the various API fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HypixelRank {
    None,
    Vip,
    VipPlus,
    Mvp,
    MvpPlus,
    /// MVP++ (monthly rank "SUPERSTAR")
    MvpPlusPlus,
}

impl HypixelRank {
    /// Parse from the raw API fields `new_package_rank` and
    /// `monthly_package_rank`. MVP++ players have `monthly_package_rank ==
    /// "SUPERSTAR"` and no `new_package_rank`.
    pub fn from_api(new_package_rank: Option<&str>, monthly_package_rank: Option<&str>) -> Self {
        if monthly_package_rank == Some("SUPERSTAR") {
            return Self::MvpPlusPlus;
        }
        match new_package_rank {
            Some("MVP_PLUS") => Self::MvpPlus,
            Some("MVP") => Self::Mvp,
            Some("VIP_PLUS") => Self::VipPlus,
            Some("VIP") => Self::Vip,
            _ => Self::None,
        }
    }

    /// The raw string stored in the database (mirrors the Hypixel API values,
    /// with `"SUPERSTAR"` used for MVP++).
    pub fn as_db_str(&self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Vip => Some("VIP"),
            Self::VipPlus => Some("VIP_PLUS"),
            Self::Mvp => Some("MVP"),
            Self::MvpPlus => Some("MVP_PLUS"),
            Self::MvpPlusPlus => Some("SUPERSTAR"),
        }
    }

    /// The RGBA colour used to render the rank name on cards.
    ///
    /// - VIP / VIP+  → green  `#55FF55`
    /// - MVP / MVP+  → blue   `#55FFFF`
    /// - MVP++       → gold   `#FFD700`
    /// - None        → white
    pub fn name_color(&self) -> image::Rgba<u8> {
        match self {
            Self::None => image::Rgba([0xff, 0xff, 0xff, 0xff]),
            Self::Vip | Self::VipPlus => image::Rgba([0x55, 0xff, 0x55, 0xff]),
            Self::Mvp | Self::MvpPlus => image::Rgba([0x55, 0xff, 0xff, 0xff]),
            Self::MvpPlusPlus => image::Rgba([0xff, 0xaa, 0x00, 0xff]),
        }
    }

    /// Human-readable display label (e.g. `"[MVP++]"`).
    pub fn display_label(&self) -> &'static str {
        match self {
            Self::None => "",
            Self::Vip => "[VIP]",
            Self::VipPlus => "[VIP+]",
            Self::Mvp => "[MVP]",
            Self::MvpPlus => "[MVP+]",
            Self::MvpPlusPlus => "[MVP++]",
        }
    }
}

/// The RGBA colour for a rank `+` symbol given the raw API colour name.
///
/// Falls back to white for unrecognised values.
pub fn plus_color_to_rgba(color: Option<&str>) -> image::Rgba<u8> {
    match color {
        Some("RED") => image::Rgba([0xff, 0x55, 0x55, 0xff]),
        Some("GOLD") => image::Rgba([0xff, 0xaa, 0x00, 0xff]),
        Some("GREEN") => image::Rgba([0x55, 0xff, 0x55, 0xff]),
        Some("YELLOW") => image::Rgba([0xff, 0xff, 0x55, 0xff]),
        Some("LIGHT_PURPLE") => image::Rgba([0xff, 0x55, 0xff, 0xff]),
        Some("WHITE") => image::Rgba([0xff, 0xff, 0xff, 0xff]),
        Some("BLUE") => image::Rgba([0x55, 0x55, 0xff, 0xff]),
        Some("DARK_GREEN") => image::Rgba([0x00, 0xaa, 0x00, 0xff]),
        Some("DARK_RED") => image::Rgba([0xaa, 0x00, 0x00, 0xff]),
        Some("DARK_AQUA") => image::Rgba([0x00, 0xaa, 0xaa, 0xff]),
        Some("DARK_PURPLE") => image::Rgba([0xaa, 0x00, 0xaa, 0xff]),
        Some("BLACK") => image::Rgba([0x00, 0x00, 0x00, 0xff]),
        _ => image::Rgba([0xff, 0xff, 0xff, 0xff]),
    }
}

/// PlayerData is the compact internal structure returned by the Hypixel client
/// and stored in the TTL cache.
#[derive(Debug, Clone)]
pub struct PlayerData {
    pub bedwars: BedwarsStats,
    /// social links (e.g. "DISCORD" -> "va80_")
    pub social_links: HashMap<String, String>,
    /// The player's normalised Hypixel rank.
    pub rank: HypixelRank,
    /// The raw plus-colour string from the API (e.g. `"DARK_GREEN"`).
    /// `None` for ranks that don't have a `+`.
    pub rank_plus_color: Option<String>,
}
