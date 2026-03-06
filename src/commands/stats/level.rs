/// `/level` command.
///
/// Shows a user's XP, level, progress to the next level, and stat changes
/// since the last sweep.  Attaches a generated PNG level card image.
use poise::serenity_prelude::{self as serenity, CreateAttachment, CreateEmbed};
use tracing::{info, debug};
use uuid::Uuid;

use crate::config::GuildConfig;
use crate::database::queries;
use crate::level_card::{self, LevelCardParams};
use crate::shared::types::{Context, Error};
use crate::stats_definitions::{display_name_for_key, is_discord_stat};
use crate::xp::calculator::{calculate_level, xp_for_level};

/// Fetch the player's Crafatar face avatar (80×80 px).
/// Non-fatal — returns `None` on any error.
async fn fetch_avatar(uuid: &Uuid) -> Option<Vec<u8>> {
    let url = format!("https://minotar.net/avatar/{}/80", uuid);

    let client = reqwest::Client::new();

    let resp = client
        .get(&url)
        .header("User-Agent", "discord-level-bot")
        .send()
        .await
        .ok()?;

    debug!("Fetched avatar for UUID {}: HTTP {}", uuid, resp.status());

    if resp.status().is_success() {
        resp.bytes().await.ok().map(|b| b.to_vec())
    } else {
        None
    }
}

/// Show your XP level and progress, with a level card image.
#[poise::command(slash_command, guild_only)]
pub async fn level(
    ctx: Context<'_>,
    #[description = "User to look up (defaults to you)"] user: Option<serenity::User>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let guild_id_i64 = guild_id.get() as i64;

    let target = user.as_ref().unwrap_or_else(|| ctx.author());
    let data = ctx.data();

    // ── resolve registered user ───────────────────────────────────────────────
    let db_user =
        queries::get_user_by_discord_id(&data.db, target.id.get() as i64, guild_id_i64).await?;

    let db_user = match db_user {
        Some(u) => u,
        None => {
            let embed = CreateEmbed::default()
                .title("Not Registered")
                .color(0xFF4444)
                .description(format!(
                    "**{}** is not registered. Use `/register` to link a Minecraft account.",
                    target.name
                ));
            ctx.send(poise::CreateReply::default().embed(embed)).await?;
            return Ok(());
        }
    };

    // ── XP & level data ───────────────────────────────────────────────────────
    let xp_row = queries::get_xp(&data.db, db_user.id).await?;
    #[allow(unused_variables)]
    let (total_xp, last_updated) = xp_row
        .as_ref()
        .map(|x| (x.total_xp, x.last_updated.clone()))
        .unwrap_or_else(|| (0.0, chrono::Utc::now()));

    let base_xp = data.config.base_level_xp;
    let exponent = data.config.level_exponent;
    let level = calculate_level(total_xp, base_xp, exponent);

    let xp_at_level = xp_for_level(level, base_xp, exponent);
    let xp_at_next = xp_for_level(level + 1, base_xp, exponent);
    let xp_this_level = total_xp - xp_at_level;
    let xp_for_next_level = xp_at_next - xp_at_level;

    // ── guild config — which stats to show deltas for ─────────────────────────
    let guild_row = queries::get_guild(&data.db, guild_id_i64).await?;
    let guild_config: GuildConfig = guild_row
        .as_ref()
        .map(|g| serde_json::from_value(g.config_json.clone()).unwrap_or_default())
        .unwrap_or_default();

    let active_keys: Vec<String> = {
        let mut keys: Vec<String> = guild_config.xp_config.keys().cloned().collect();
        keys.sort();
        keys
    };

    // ── compute stat deltas since last sweep ──────────────────────────────────
    // "since last sweep" = latest snapshot − snapshot just before `last_updated`
    let mut stat_deltas: Vec<(String, f64)> = Vec::new();
    let mut xp_gained: f64 = 0.0;

    for key in &active_keys {
        let (latest_val, initial_val) = if is_discord_stat(key) {
            let latest = queries::get_latest_discord_snapshot(&data.db, db_user.id, key)
                .await?
                .map(|s| s.stat_value)
                .unwrap_or(0.0);

            let initial = queries::get_first_discord_snapshot(&data.db, db_user.id, key)
                .await?
                .map(|s| s.stat_value)
                .unwrap_or(0.0);

            (latest, initial)
        } else {
            let latest = queries::get_latest_hypixel_snapshot(&data.db, db_user.id, key)
                .await?
                .map(|s| s.stat_value)
                .unwrap_or(0.0);

            let initial = queries::get_first_hypixel_snapshot(&data.db, db_user.id, key)
                .await?
                .map(|s| s.stat_value)
                .unwrap_or(0.0);

            (latest, initial)
        };

        let delta = (latest_val - initial_val).max(0.0);

        if delta > 0.0 {
            let per_unit = guild_config.xp_config.get(key).copied().unwrap_or(0.0);
            xp_gained += delta.round() * per_unit;
            stat_deltas.push((display_name_for_key(key), delta));
        }
    }

    // Keep at most 8 deltas, sorted by delta descending.
    stat_deltas.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    stat_deltas.truncate(8);

    // ── fetch avatar ──────────────────────────────────────────────────────────
    let avatar_bytes = if let Some(tex) = &db_user.head_texture {
        if let Some(encoded) = tex.strip_prefix("data:image/png;base64,") {
            use base64::{engine::general_purpose, Engine as _};
            general_purpose::STANDARD.decode(encoded).ok()
        } else {
            None
        }
    } else {
        fetch_avatar(&db_user.minecraft_uuid).await
    };

    // ── render level card ─────────────────────────────────────────────────────
    let mc_name = match &db_user.minecraft_username {
        Some(name) => name.clone(),
        None => match data.hypixel.resolve_uuid(&db_user.minecraft_uuid).await {
            Ok(name) => name,
            Err(_) => db_user.minecraft_uuid.to_string(),
        },
    };

    let params = LevelCardParams {
        minecraft_username: mc_name,
        level,
        total_xp,
        xp_this_level,
        xp_for_next_level,
        stat_deltas,
        xp_gained,
        avatar_bytes,
    };

    let png_bytes = level_card::render(&params);
    let attachment = CreateAttachment::bytes(png_bytes, "level_card.png");

    // == SEND IMAGE ONLY (no embed) ===========================================
    ctx.send(poise::CreateReply::default().attachment(attachment))
        .await?;
    
    info!(
		"Sent level card for user {} (Discord ID {}, Minecraft username {:#?}) in guild {}",
		target.name, target.id, db_user.minecraft_username, guild_id
	);

    Ok(())
}
