/// `/edit-stats` command group — admin only.
///
/// Provides subcommands for managing the guild's stat XP configuration:
/// - `add-bedwars` — configure a Bedwars stat via mode + metric pickers
/// - `add-discord` — configure a Discord activity stat
/// - `edit`        — change the XP value for an existing configured stat
/// - `remove`      — remove a stat (existing snapshots are kept)
/// - `list`        — display all configured stats and their XP values
///
/// All subcommands are ephemeral (visible only to the invoker) and require the
/// invoker's Discord user ID to be in `AppConfig.admin_user_ids`.
use poise::serenity_prelude::{self as serenity, CreateEmbed};

use crate::config::GuildConfig;
use crate::database::queries;
use crate::shared::types::{Context, Error};

// ---------------------------------------------------------------------------
// Static stat definitions
// ---------------------------------------------------------------------------

struct BedwarsMode {
    display: &'static str,
    value: &'static str,
}

struct BedwarsMetric {
    display: &'static str,
    /// The raw Hypixel API suffix (e.g. `"wins_bedwars"`).
    /// For overall this is the full key; for a specific mode it is appended to
    /// the mode prefix: `"{mode}_{value}"`.
    value: &'static str,
}

struct DiscordStat {
    display: &'static str,
    /// The raw stat name used in `discord_stats_snapshot`.
    value: &'static str,
}

/// The six core Bedwars modes exposed to admins.
const BEDWARS_MODES: &[BedwarsMode] = &[
    BedwarsMode { display: "Overall",        value: "overall"     },
    BedwarsMode { display: "Solo (8x1)",     value: "eight_one"   },
    BedwarsMode { display: "Doubles (8x2)",  value: "eight_two"   },
    BedwarsMode { display: "3v3v3v3 (4x3)", value: "four_three"  },
    BedwarsMode { display: "4v4v4v4 (4x4)", value: "four_four"   },
    BedwarsMode { display: "4v4 (2x4)",     value: "two_four"    },
];

/// Trackable Bedwars metrics. Each value is the raw Hypixel API suffix that is
/// either used as-is (overall) or prefixed with the mode (per-mode).
const BEDWARS_METRICS: &[BedwarsMetric] = &[
    BedwarsMetric { display: "Wins",                    value: "wins_bedwars"                       },
    BedwarsMetric { display: "Losses",                  value: "losses_bedwars"                     },
    BedwarsMetric { display: "Games Played",            value: "games_played_bedwars"               },
    BedwarsMetric { display: "Kills",                   value: "kills_bedwars"                      },
    BedwarsMetric { display: "Deaths",                  value: "deaths_bedwars"                     },
    BedwarsMetric { display: "Final Kills",             value: "final_kills_bedwars"                },
    BedwarsMetric { display: "Final Deaths",            value: "final_deaths_bedwars"               },
    BedwarsMetric { display: "Beds Broken",             value: "beds_broken_bedwars"                },
    BedwarsMetric { display: "Beds Lost",               value: "beds_lost_bedwars"                  },
    BedwarsMetric { display: "Winstreak",               value: "winstreak"                          },
    BedwarsMetric { display: "Resources Collected",     value: "resources_collected_bedwars"        },
    BedwarsMetric { display: "Iron Collected",          value: "iron_resources_collected_bedwars"   },
    BedwarsMetric { display: "Gold Collected",          value: "gold_resources_collected_bedwars"   },
    BedwarsMetric { display: "Diamond Collected",       value: "diamond_resources_collected_bedwars"},
    BedwarsMetric { display: "Emerald Collected",       value: "emerald_resources_collected_bedwars"},
    BedwarsMetric { display: "Items Purchased",         value: "items_purchased_bedwars"            },
    BedwarsMetric { display: "Void Kills",              value: "void_kills_bedwars"                 },
    BedwarsMetric { display: "Void Deaths",             value: "void_deaths_bedwars"                },
];

/// Discord activity stats that the bot currently tracks.
/// These match the stat names used in `discord_stats_snapshot`.
const DISCORD_STATS: &[DiscordStat] = &[
    DiscordStat { display: "Messages Sent",  value: "messages_sent"  },
    DiscordStat { display: "Reactions Given", value: "reactions_added" },
    DiscordStat { display: "Commands Used",  value: "commands_used"  },
];

// ---------------------------------------------------------------------------
// Stat key construction
// ---------------------------------------------------------------------------

/// Build the final stat key that gets stored in `xp_config`.
///
/// - `overall` mode: the metric suffix is used directly (`"wins_bedwars"`).
/// - Any other mode: the prefix is prepended (`"eight_two_wins_bedwars"`).
fn build_stat_key(mode: &str, metric: &str) -> String {
    if mode == "overall" {
        metric.to_string()
    } else {
        format!("{mode}_{metric}")
    }
}

// ---------------------------------------------------------------------------
// Autocomplete helpers
// ---------------------------------------------------------------------------

/// Autocomplete for Bedwars mode selection.
/// Returns the 6 core modes with friendly display names.
async fn autocomplete_mode<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> Vec<serenity::AutocompleteChoice> {
    let partial_lower = partial.to_lowercase();
    BEDWARS_MODES
        .iter()
        .filter(|m| {
            m.display.to_lowercase().contains(&partial_lower)
                || m.value.to_lowercase().contains(&partial_lower)
        })
        .map(|m| serenity::AutocompleteChoice::new(m.display, m.value))
        .collect()
}

/// Autocomplete for Bedwars metric selection.
/// Always returns the full metric list regardless of the selected mode.
async fn autocomplete_metric<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> Vec<serenity::AutocompleteChoice> {
    let partial_lower = partial.to_lowercase();
    BEDWARS_METRICS
        .iter()
        .filter(|m| {
            m.display.to_lowercase().contains(&partial_lower)
                || m.value.to_lowercase().contains(&partial_lower)
        })
        .map(|m| serenity::AutocompleteChoice::new(m.display, m.value))
        .collect()
}

/// Autocomplete for Discord stat selection.
/// Returns only the stats the bot currently tracks.
async fn autocomplete_discord_stat<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> Vec<serenity::AutocompleteChoice> {
    let partial_lower = partial.to_lowercase();
    DISCORD_STATS
        .iter()
        .filter(|s| {
            s.display.to_lowercase().contains(&partial_lower)
                || s.value.to_lowercase().contains(&partial_lower)
        })
        .map(|s| serenity::AutocompleteChoice::new(s.display, s.value))
        .collect()
}

/// Autocomplete for stats already in the guild's `xp_config`.
/// Used by `/edit-stats edit` and `/edit-stats remove`.
async fn autocomplete_configured_stat<'a>(ctx: Context<'_>, partial: &'a str) -> Vec<String> {
    if partial.len() < 2 {
        return Vec::new();
    }

    let guild_id = match ctx.guild_id() {
        Some(id) => id.get() as i64,
        None => return Vec::new(),
    };

    let config = match queries::get_guild(&ctx.data().db, guild_id).await {
        Ok(Some(row)) => serde_json::from_str::<GuildConfig>(&row.config_json).unwrap_or_default(),
        _ => GuildConfig::default(),
    };

    let partial_lower = partial.to_lowercase();
    let mut results: Vec<String> = config
        .xp_config
        .keys()
        .filter(|k| k.to_lowercase().contains(&partial_lower))
        .cloned()
        .collect();

    results.sort();
    results.truncate(25);
    results
}

// ---------------------------------------------------------------------------
// Helper — inline admin check
// ---------------------------------------------------------------------------

fn is_admin(ctx: &Context<'_>) -> bool {
    ctx.data()
        .config
        .admin_user_ids
        .contains(&ctx.author().id.get())
}

// ---------------------------------------------------------------------------
// Shared guild config loader
// ---------------------------------------------------------------------------

async fn load_guild_config(ctx: &Context<'_>) -> Result<(i64, GuildConfig), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    queries::upsert_guild(&data.db, guild_id).await?;
    let guild_row = queries::get_guild(&data.db, guild_id).await?;
    let config: GuildConfig = guild_row
        .as_ref()
        .map(|g| serde_json::from_str(&g.config_json).unwrap_or_default())
        .unwrap_or_default();

    Ok((guild_id, config))
}

// ---------------------------------------------------------------------------
// Parent command
// ---------------------------------------------------------------------------

/// Manage stat XP configuration for this server. Admin only.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    subcommands("add_bedwars", "add_discord", "edit_stat", "remove", "list")
)]
pub async fn edit_stats(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

/// Add a Bedwars stat to the XP configuration by picking a mode and metric.
///
/// The final stat key is built as `{mode}_{metric}` (e.g. `eight_two_final_kills_bedwars`).
/// For the Overall mode the metric is used directly (e.g. `wins_bedwars`).
#[poise::command(slash_command, guild_only, ephemeral, rename = "add-bedwars")]
pub async fn add_bedwars(
    ctx: Context<'_>,
    #[description = "Bedwars mode (e.g. Solo, Doubles)"]
    #[autocomplete = "autocomplete_mode"]
    mode: String,
    #[description = "Stat metric to track (e.g. Final Kills, Wins)"]
    #[autocomplete = "autocomplete_metric"]
    metric: String,
    #[description = "XP awarded per unit increase"] xp_per_unit: f64,
) -> Result<(), Error> {
    if !is_admin(&ctx) {
        ctx.say("You do not have permission to use this command.")
            .await?;
        return Ok(());
    }

    // Validate against the fixed lists to guard against autocomplete bypass.
    if !BEDWARS_MODES.iter().any(|m| m.value == mode) {
        ctx.say(format!(
            "`{mode}` is not a valid mode. Please select one from the autocomplete list."
        ))
        .await?;
        return Ok(());
    }
    if !BEDWARS_METRICS.iter().any(|m| m.value == metric) {
        ctx.say(format!(
            "`{metric}` is not a valid metric. Please select one from the autocomplete list."
        ))
        .await?;
        return Ok(());
    }

    let stat_key = build_stat_key(&mode, &metric);

    let (guild_id, mut config) = load_guild_config(&ctx).await?;
    let data = ctx.data();

    if config.xp_config.contains_key(&stat_key) {
        ctx.say(format!(
            "**`{stat_key}`** is already configured ({} XP/unit). \
            Use `/edit-stats edit` to change its value.",
            config.xp_config[&stat_key]
        ))
        .await?;
        return Ok(());
    }

    config.xp_config.insert(stat_key.clone(), xp_per_unit);
    let config_json = serde_json::to_string(&config)?;
    queries::update_guild_config(&data.db, guild_id, &config_json).await?;

    let mode_display = BEDWARS_MODES
        .iter()
        .find(|m| m.value == mode)
        .map(|m| m.display)
        .unwrap_or(&mode);
    let metric_display = BEDWARS_METRICS
        .iter()
        .find(|m| m.value == metric)
        .map(|m| m.display)
        .unwrap_or(&metric);

    ctx.say(format!(
        "Added **{mode_display} — {metric_display}** (`{stat_key}`) → **{xp_per_unit} XP/unit**."
    ))
    .await?;

    Ok(())
}

/// Add a Discord activity stat to the XP configuration.
///
/// Discord stats are tracked from server activity (messages, reactions, etc.)
/// rather than fetched from the Hypixel API.
#[poise::command(slash_command, guild_only, ephemeral, rename = "add-discord")]
pub async fn add_discord(
    ctx: Context<'_>,
    #[description = "Discord activity stat to track"]
    #[autocomplete = "autocomplete_discord_stat"]
    stat: String,
    #[description = "XP awarded per unit increase"] xp_per_unit: f64,
) -> Result<(), Error> {
    if !is_admin(&ctx) {
        ctx.say("You do not have permission to use this command.")
            .await?;
        return Ok(());
    }

    // Validate against the fixed list to guard against autocomplete bypass.
    if !DISCORD_STATS.iter().any(|s| s.value == stat) {
        ctx.say(format!(
            "`{stat}` is not a tracked Discord stat. Please select one from the autocomplete list."
        ))
        .await?;
        return Ok(());
    }

    let (guild_id, mut config) = load_guild_config(&ctx).await?;
    let data = ctx.data();

    if config.xp_config.contains_key(&stat) {
        ctx.say(format!(
            "**`{stat}`** is already configured ({} XP/unit). \
            Use `/edit-stats edit` to change its value.",
            config.xp_config[&stat]
        ))
        .await?;
        return Ok(());
    }

    config.xp_config.insert(stat.clone(), xp_per_unit);
    let config_json = serde_json::to_string(&config)?;
    queries::update_guild_config(&data.db, guild_id, &config_json).await?;

    let stat_display = DISCORD_STATS
        .iter()
        .find(|s| s.value == stat)
        .map(|s| s.display)
        .unwrap_or(&stat);

    ctx.say(format!(
        "Added Discord stat **{stat_display}** (`{stat}`) → **{xp_per_unit} XP/unit**."
    ))
    .await?;

    Ok(())
}

/// Edit the XP value for an existing configured stat.
#[poise::command(slash_command, guild_only, ephemeral, rename = "edit")]
pub async fn edit_stat(
    ctx: Context<'_>,
    #[description = "Stat to modify"]
    #[autocomplete = "autocomplete_configured_stat"]
    stat_name: String,
    #[description = "New XP per unit"] new_xp_value: f64,
) -> Result<(), Error> {
    if !is_admin(&ctx) {
        ctx.say("You do not have permission to use this command.")
            .await?;
        return Ok(());
    }

    let (guild_id, mut config) = load_guild_config(&ctx).await?;
    let data = ctx.data();

    if !config.xp_config.contains_key(&stat_name) {
        ctx.say(format!(
            "Stat `{stat_name}` is not configured. \
            Use `/edit-stats add-bedwars` or `/edit-stats add-discord` to add it."
        ))
        .await?;
        return Ok(());
    }

    let old_xp = config.xp_config[&stat_name];
    config.xp_config.insert(stat_name.clone(), new_xp_value);
    let config_json = serde_json::to_string(&config)?;
    queries::update_guild_config(&data.db, guild_id, &config_json).await?;

    ctx.say(format!(
        "Updated `{stat_name}`: {old_xp} XP/unit → **{new_xp_value} XP/unit**."
    ))
    .await?;

    Ok(())
}

/// Remove a stat from the XP configuration.
#[poise::command(slash_command, guild_only, ephemeral)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Stat to remove"]
    #[autocomplete = "autocomplete_configured_stat"]
    stat_name: String,
) -> Result<(), Error> {
    if !is_admin(&ctx) {
        ctx.say("You do not have permission to use this command.")
            .await?;
        return Ok(());
    }

    let (guild_id, mut config) = load_guild_config(&ctx).await?;
    let data = ctx.data();

    if config.xp_config.remove(&stat_name).is_none() {
        ctx.say(format!("Stat `{stat_name}` is not configured."))
            .await?;
        return Ok(());
    }

    let config_json = serde_json::to_string(&config)?;
    queries::update_guild_config(&data.db, guild_id, &config_json).await?;

    ctx.say(format!(
        "Removed `{stat_name}` from XP configuration. Existing snapshots are preserved."
    ))
    .await?;

    Ok(())
}

/// List all stats currently in the XP configuration.
#[poise::command(slash_command, guild_only, ephemeral)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    if !is_admin(&ctx) {
        ctx.say("You do not have permission to use this command.")
            .await?;
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let guild_row = queries::get_guild(&data.db, guild_id).await?;
    let config: GuildConfig = guild_row
        .as_ref()
        .map(|g| serde_json::from_str(&g.config_json).unwrap_or_default())
        .unwrap_or_default();

    if config.xp_config.is_empty() {
        ctx.say(
            "No stats are currently configured for XP. \
            Use `/edit-stats add-bedwars` or `/edit-stats add-discord` to add one.",
        )
        .await?;
        return Ok(());
    }

    let mut lines: Vec<String> = config
        .xp_config
        .iter()
        .map(|(k, v)| format!("{k}: {v} XP/unit"))
        .collect();
    lines.sort();

    let embed = CreateEmbed::default()
        .title("Configured XP Stats")
        .description(format!("```\n{}\n```", lines.join("\n")))
        .color(0x00BFFF);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
