/// Shared stat name definitions and display-name lookups.
///
/// This module is the single source of truth for:
/// - The list of Bedwars modes and metrics the bot understands
/// - The list of Discord activity stats
/// - A helper to convert a raw DB/API key into a human-readable label
///
/// Consumed by `edit_stats.rs` (autocomplete + validation) and
/// `stats.rs` / `level.rs` (display labels).

pub struct BedwarsMode {
    pub display: &'static str,
    pub value: &'static str,
}

pub struct BedwarsMetric {
    pub display: &'static str,
    /// Raw Hypixel API suffix (e.g. `"wins_bedwars"`).
    /// For overall mode this is the full key; for a specific mode it is appended
    /// to the mode prefix: `"{mode}_{value}"`.
    pub value: &'static str,
}

pub struct DiscordStat {
    pub display: &'static str,
    /// Raw stat name stored in `discord_stats_snapshot`.
    pub value: &'static str,
}

pub const BEDWARS_MODES: &[BedwarsMode] = &[
    BedwarsMode {
        display: "Overall",
        value: "overall",
    },
    BedwarsMode {
        display: "Solo (8x1)",
        value: "eight_one",
    },
    BedwarsMode {
        display: "Doubles (8x2)",
        value: "eight_two",
    },
    BedwarsMode {
        display: "3v3v3v3 (4x3)",
        value: "four_three",
    },
    BedwarsMode {
        display: "4v4v4v4 (4x4)",
        value: "four_four",
    },
    BedwarsMode {
        display: "4v4 (2x4)",
        value: "two_four",
    },
];

pub const BEDWARS_METRICS: &[BedwarsMetric] = &[
    BedwarsMetric {
        display: "Wins",
        value: "wins_bedwars",
    },
    BedwarsMetric {
        display: "Losses",
        value: "losses_bedwars",
    },
    BedwarsMetric {
        display: "Games Played",
        value: "games_played_bedwars",
    },
    BedwarsMetric {
        display: "Kills",
        value: "kills_bedwars",
    },
    BedwarsMetric {
        display: "Deaths",
        value: "deaths_bedwars",
    },
    BedwarsMetric {
        display: "Final Kills",
        value: "final_kills_bedwars",
    },
    BedwarsMetric {
        display: "Final Deaths",
        value: "final_deaths_bedwars",
    },
    BedwarsMetric {
        display: "Beds Broken",
        value: "beds_broken_bedwars",
    },
    BedwarsMetric {
        display: "Beds Lost",
        value: "beds_lost_bedwars",
    },
    BedwarsMetric {
        display: "Winstreak",
        value: "winstreak",
    },
    BedwarsMetric {
        display: "Resources Collected",
        value: "resources_collected_bedwars",
    },
    BedwarsMetric {
        display: "Iron Collected",
        value: "iron_resources_collected_bedwars",
    },
    BedwarsMetric {
        display: "Gold Collected",
        value: "gold_resources_collected_bedwars",
    },
    BedwarsMetric {
        display: "Diamond Collected",
        value: "diamond_resources_collected_bedwars",
    },
    BedwarsMetric {
        display: "Emerald Collected",
        value: "emerald_resources_collected_bedwars",
    },
    BedwarsMetric {
        display: "Items Purchased",
        value: "items_purchased_bedwars",
    },
    BedwarsMetric {
        display: "Void Kills",
        value: "void_kills_bedwars",
    },
    BedwarsMetric {
        display: "Void Deaths",
        value: "void_deaths_bedwars",
    },
];

pub const DISCORD_STATS: &[DiscordStat] = &[
    DiscordStat {
        display: "Messages Sent",
        value: "messages_sent",
    },
    DiscordStat {
        display: "Reactions Given",
        value: "reactions_added",
    },
    DiscordStat {
        display: "Commands Used",
        value: "commands_used",
    },
    DiscordStat {
        display: "Voice Minutes",
        value: "voice_minutes",
    },
];

/// Flat slice of Discord stat key strings for quick membership checks.
pub const DISCORD_STAT_KEYS: &[&str] = &[
    "messages_sent",
    "reactions_added",
    "commands_used",
    "voice_minutes",
];

/// Return a human-friendly display label for a raw stat key.
///
/// Resolution order:
/// 1. Discord stats (exact match)
/// 2. Bedwars metrics (overall — direct match on `metric.value`)
/// 3. Bedwars metrics (per-mode — key == `"{mode}_{metric}"`)
/// 4. Fallback: snake_case → Title Case
pub fn display_name_for_key(key: &str) -> String {
    // 1. Discord
    for s in DISCORD_STATS {
        if s.value == key {
            return s.display.to_string();
        }
    }

    // 2. Overall Bedwars metric (key == metric.value)
    for m in BEDWARS_METRICS {
        if key == m.value {
            return m.display.to_string();
        }
    }

    // 3. Per-mode Bedwars metric (key == "{mode}_{metric.value}")
    for mode in BEDWARS_MODES {
        if mode.value == "overall" {
            continue;
        }
        let prefix = format!("{}_", mode.value);
        if let Some(suffix) = key.strip_prefix(prefix.as_str()) {
            for m in BEDWARS_METRICS {
                if suffix == m.value {
                    return format!("{} — {}", mode.display, m.display);
                }
            }
        }
    }

    // 4. Fallback: snake_case → Title Case
    key.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Returns `true` if `key` is a known Discord activity stat.
pub fn is_discord_stat(key: &str) -> bool {
    DISCORD_STAT_KEYS.contains(&key)
}
