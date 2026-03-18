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
use tracing::info;

use crate::commands::logger::logger::logger;
use crate::config::GuildConfig;
use crate::database::queries;
use crate::shared::types::{Context, Error};
use crate::utils::stats_definitions::{
    BEDWARS_METRICS, BEDWARS_MODES, DISCORD_STATS, display_name_for_key,
};

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
async fn autocomplete_configured_stat<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> Vec<serenity::AutocompleteChoice> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id.get() as i64,
        None => return Vec::new(),
    };

    let config = match queries::get_guild(&ctx.data().db, guild_id).await {
        Ok(Some(row)) => serde_json::from_value::<GuildConfig>(row.config_json).unwrap_or_default(),
        _ => GuildConfig::default(),
    };

    let partial_lower = partial.to_lowercase();

    // Build (display_name, raw_key) pairs, filter by partial match on either,
    // sort by display name, then cap at the Discord limit of 25.
    let mut results: Vec<(String, String)> = config
        .xp_config
        .keys()
        .filter_map(|k| {
            let display = display_name_for_key(k);
            let matches = display.to_lowercase().contains(&partial_lower)
                || k.to_lowercase().contains(&partial_lower);
            if matches {
                Some((display, k.clone()))
            } else {
                None
            }
        })
        .collect();

    results.sort_by(|a, b| a.0.cmp(&b.0));
    results.truncate(25);

    // The AutocompleteChoice shows `display` to the user but submits `raw_key`
    // as the value, so the command handler always receives the exact stat key.
    results
        .into_iter()
        .map(|(display, raw_key)| serenity::AutocompleteChoice::new(display, raw_key))
        .collect()
}

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
        .map(|g| serde_json::from_value(g.config_json.clone()).unwrap_or_default())
        .unwrap_or_default();

    Ok((guild_id, config))
}

/// Manage stat XP configuration for this server. Admin only.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "edit-stats",
    subcommands("add_bedwars", "add_discord", "edit_stat", "remove", "list")
)]
pub async fn edit_stats(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Add a Bedwars stat to the XP configuration by picking a mode and metric.
///
/// The final stat key is built as `{mode}_{metric}` (e.g. `eight_two_final_kills_bedwars`).
/// For the Overall mode the metric is used directly (e.g. `wins_bedwars`).
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "add-bedwars",
    required_permissions = "ADMINISTRATOR"
)]
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
    let config_json = serde_json::to_value(config.clone())?;
    queries::update_guild_config(&data.db, guild_id, config_json).await?;
    data.guild_configs
        .insert(guild_id, (config, std::time::Instant::now()));

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

    info!(
        "Added **{mode_display} — {metric_display}** (`{stat_key}`) XP configuration by {}",
        ctx.author().name
    );

    logger(
        ctx.serenity_context(),
        data,
        ctx.guild_id().unwrap(),
        crate::commands::logger::logger::LogType::Info,
        format!(
            "{} added Bedwars stat `{stat_key}` with {} XP/unit",
            ctx.author().name,
            xp_per_unit
        ),
    )
    .await?;

    Ok(())
}

/// Add a Discord activity stat to the XP configuration.
///
/// Discord stats are tracked from server activity (messages, reactions, etc.)
/// rather than fetched from the Hypixel API.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "add-discord",
    required_permissions = "ADMINISTRATOR"
)]
pub async fn add_discord(
    ctx: Context<'_>,
    #[description = "Discord activity stat to track"]
    #[autocomplete = "autocomplete_discord_stat"]
    stat: String,
    #[description = "XP awarded per unit increase"] xp_per_unit: f64,
) -> Result<(), Error> {
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
    let config_json = serde_json::to_value(&config)?;
    queries::update_guild_config(&data.db, guild_id, config_json).await?;
    data.guild_configs
        .insert(guild_id, (config, std::time::Instant::now()));

    let stat_display = DISCORD_STATS
        .iter()
        .find(|s| s.value == stat)
        .map(|s| s.display)
        .unwrap_or(&stat);

    ctx.say(format!(
        "Added Discord stat **{stat_display}** (`{stat}`) → **{xp_per_unit} XP/unit**."
    ))
    .await?;

    info!(
        "Added Discord stat `{stat}` XP configuration by {}",
        ctx.author().name
    );

    logger(
        ctx.serenity_context(),
        data,
        ctx.guild_id().unwrap(),
        crate::commands::logger::logger::LogType::Info,
        format!(
            "{} added Discord stat `{stat}` with {} XP/unit",
            ctx.author().name,
            xp_per_unit
        ),
    )
    .await?;

    Ok(())
}

/// Edit the XP value for an existing configured stat.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "edit",
    required_permissions = "ADMINISTRATOR"
)]
pub async fn edit_stat(
    ctx: Context<'_>,
    #[description = "Stat to modify"]
    #[autocomplete = "autocomplete_configured_stat"]
    stat_name: String,
    #[description = "New XP per unit"] new_xp_value: f64,
) -> Result<(), Error> {
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
    let config_json = serde_json::to_value(&config)?;
    queries::update_guild_config(&data.db, guild_id, config_json).await?;
    data.guild_configs
        .insert(guild_id, (config, std::time::Instant::now()));

    ctx.say(format!(
        "Updated `{stat_name}`: {old_xp} XP/unit → **{new_xp_value} XP/unit**."
    ))
    .await?;

    info!(
        "Updated `{stat_name}` XP configuration by {}",
        ctx.author().name
    );

    logger(
        ctx.serenity_context(),
        data,
        ctx.guild_id().unwrap(),
        crate::commands::logger::logger::LogType::Warn,
        format!(
            "{} changed `{stat_name}` XP from {} → {}",
            ctx.author().name,
            old_xp,
            new_xp_value
        ),
    )
    .await?;

    Ok(())
}

/// Remove a stat from the XP configuration.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Stat to remove"]
    #[autocomplete = "autocomplete_configured_stat"]
    stat_name: String,
) -> Result<(), Error> {
    let (guild_id, mut config) = load_guild_config(&ctx).await?;
    let data = ctx.data();

    if config.xp_config.remove(&stat_name).is_none() {
        ctx.say(format!("Stat `{stat_name}` is not configured."))
            .await?;
        return Ok(());
    }

    let config_json = serde_json::to_value(&config)?;
    queries::update_guild_config(&data.db, guild_id, config_json).await?;
    data.guild_configs
        .insert(guild_id, (config, std::time::Instant::now()));

    ctx.say(format!(
        "Removed `{stat_name}` from XP configuration. Existing snapshots are preserved."
    ))
    .await?;
    info!(
        "Removed `{stat_name}` from XP configuration by {}",
        ctx.author().name
    );

    logger(
        ctx.serenity_context(),
        data,
        ctx.guild_id().unwrap(),
        crate::commands::logger::logger::LogType::Warn,
        format!(
            "{} removed stat `{stat_name}` from XP config",
            ctx.author().name
        ),
    )
    .await?;

    Ok(())
}

/// List all stats currently in the XP configuration.
#[poise::command(slash_command, guild_only, ephemeral)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let guild_row = queries::get_guild(&data.db, guild_id).await?;
    let config: GuildConfig = guild_row
        .as_ref()
        .map(|g| serde_json::from_value(g.config_json.clone()).unwrap_or_default())
        .unwrap_or_default();

    if config.xp_config.is_empty() {
        ctx.say(
            "No stats are currently configured for XP. \
            Use `/edit-stats add-bedwars` or `/edit-stats add-discord` to add one.",
        )
        .await?;
        return Ok(());
    }

    // Build sorted rows: (display_name, raw_key, xp_per_unit)
    let mut rows: Vec<(String, String, f64)> = config
        .xp_config
        .iter()
        .map(|(k, v)| (display_name_for_key(k), k.clone(), *v))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    let description = rows
        .iter()
        .map(|(display, key, xp)| format!("**{}** — {:.0} XP (`{}`)", display, xp, key))
        .collect::<Vec<_>>()
        .join("\n");

    let embed = CreateEmbed::default()
        .title(format!("XP Stats — {} configured", rows.len()))
        .description(description)
        .color(0x00BFFF);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
