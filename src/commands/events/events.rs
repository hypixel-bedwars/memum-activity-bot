/// `/event` and `/edit-event` command groups.
///
/// Public commands:
/// - /event list
/// - /event info
/// - /event leaderboard
/// - /event level
///
/// Admin commands:
/// - /edit-event new
/// - /edit-event edit
/// - /edit-event delete
/// - /edit-event start
/// - /edit-event end
/// - /edit-event participants
/// - /edit-event leaderboard persist
use poise::serenity_prelude::{
    self as serenity, CreateActionRow, CreateAttachment, CreateButton, CreateEmbed,
};
use tracing::info;
use uuid::Uuid;

use crate::cards::level_card::{self, LevelCardParams};
use crate::cards::statistics::{self as statistics_card, StatisticsCardParams};

use crate::commands::leaderboard::helpers as lb_helpers;
use crate::database::queries;
use crate::shared::types::{Context, Error};
use crate::utils::stats_definitions::display_name_for_key;

// ========================================================
// Autocomplete
// ========================================================

async fn autocomplete_event_name<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> Vec<serenity::AutocompleteChoice> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id.get() as i64,
        None => return Vec::new(),
    };

    let events = queries::list_events(&ctx.data().db, guild_id)
        .await
        .unwrap_or_default();

    let partial_lower = partial.to_lowercase();

    events
        .iter()
        .filter(|e| e.name.to_lowercase().contains(&partial_lower))
        .take(25)
        .map(|e| {
            let label = format!("{} [{}]", e.name, e.status);
            serenity::AutocompleteChoice::new(label, e.name.clone())
        })
        .collect()
}

#[poise::command(
    slash_command,
    guild_only,
    subcommands("list", "info", "leaderboard", "level", "statistics", "milestones"),
    subcommand_required
)]
pub async fn event(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

// List all events
#[poise::command(slash_command, guild_only)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get() as i64;
    let events = queries::list_events(&ctx.data().db, guild_id).await?;

    if events.is_empty() {
        let embed = CreateEmbed::new()
            .title("No Events Found")
            .description("No events have been created yet.")
            .color(0x5865F2);

        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    let mut description = String::new();

    for event in &events {
        let status = match event.status.as_str() {
            "pending" => "🕒",
            "active" => "🟢",
            "ended" => "🔴",
            _ => "❓",
        };

        description.push_str(&format!(
            "{} **{}** — `{}` | <t:{}:d> → <t:{}:d>\n",
            status,
            event.name,
            event.status,
            event.start_date.timestamp(),
            event.end_date.timestamp()
        ));
    }

    let embed = CreateEmbed::new()
        .title(format!("Events — {} total", events.len()))
        .description(description)
        .color(0x00BFFF);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

#[poise::command(slash_command, guild_only, ephemeral)]
pub async fn info(
    ctx: Context<'_>,
    #[description = "Event to look up"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get() as i64;

    let event = match queries::get_event_by_name(&ctx.data().db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{}** not found.", event_name))
                .await?;
            return Ok(());
        }
    };

    let mut description = format!(
        "**Status:** {}\n\
         **Start:** <t:{}:F>\n\
         **End:** <t:{}:F>",
        event.status,
        event.start_date.timestamp(),
        event.end_date.timestamp()
    );

    if let Some(desc) = event.description {
        description = format!("{}\n\n{}", desc, description);
    }

    let embed = CreateEmbed::new()
        .title(format!("Event: {}", event.name))
        .description(description)
        .color(0x00BFFF);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

#[poise::command(slash_command, guild_only)]
pub async fn leaderboard(
    ctx: Context<'_>,
    #[description = "Event name (defaults to most recent)"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: Option<String>,
    #[description = "Page number (default: 1)"] page: Option<u32>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx.guild_id().unwrap().get() as i64;

    let event_name = match event_name {
        Some(n) => n,
        None => queries::get_latest_event_name(&ctx.data().db, guild_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No active or ended events found"))?,
    };

    let event = queries::get_event_by_name(&ctx.data().db, guild_id, &event_name)
        .await?
        .unwrap();

    let page = page.unwrap_or(1).max(1);

    let (png_bytes, total_pages) = lb_helpers::generate_event_leaderboard_page(
        &ctx.data().db,
        event.id,
        &event.name,
        &event.status,
        event.start_date.timestamp(),
        page,
    )
    .await?;

    let attachment =
        poise::serenity_prelude::CreateAttachment::bytes(png_bytes, "event_leaderboard.png");

    let components = event_lb_pagination_buttons(event.id, page, total_pages);

    ctx.send(
        poise::CreateReply::default()
            .attachment(attachment)
            .components(components),
    )
    .await?;

    Ok(())
}

/// Build pagination buttons for an event leaderboard.
///
/// Returns an empty vec when there is only one page.
pub fn event_lb_pagination_buttons(
    event_id: i64,
    current_page: u32,
    total_pages: u32,
) -> Vec<CreateActionRow> {
    if total_pages <= 1 {
        return vec![];
    }

    let mut buttons: Vec<CreateButton> = Vec::new();

    if current_page > 1 {
        buttons.push(
            CreateButton::new(format!("event_lb_{event_id}_page_{}", current_page - 1))
                .label("◀ Prev")
                .style(serenity::ButtonStyle::Secondary),
        );
    }

    buttons.push(
        CreateButton::new(format!("event_lb_{event_id}_page_{current_page}"))
            .label(format!("Page {current_page}/{total_pages}"))
            .style(serenity::ButtonStyle::Primary)
            .disabled(true),
    );

    if current_page < total_pages {
        buttons.push(
            CreateButton::new(format!("event_lb_{event_id}_page_{}", current_page + 1))
                .label("Next ▶")
                .style(serenity::ButtonStyle::Secondary),
        );
    }

    vec![CreateActionRow::Buttons(buttons)]
}

/// Fetch the player's Minotar face avatar (80x80 px) for the event level card.
/// Non-fatal — returns `None` on any error.
async fn fetch_event_avatar(uuid: &Uuid) -> Option<Vec<u8>> {
    let url = format!("https://minotar.net/avatar/{}/80", uuid);
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "discord-level-bot")
        .send()
        .await
        .ok()?;
    if resp.status().is_success() {
        resp.bytes().await.ok().map(|b| b.to_vec())
    } else {
        None
    }
}

/// Show your stats and rank for a specific event, with a level card image.
#[poise::command(slash_command, guild_only)]
pub async fn level(
    ctx: Context<'_>,
    #[description = "Event to look up (defaults to the most recent event)"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: Option<String>,
    #[description = "User to look up (defaults to you)"] user: Option<serenity::User>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx.guild_id().unwrap().get() as i64;
    let target = user.as_ref().unwrap_or_else(|| ctx.author());
    let user_id = target.id.get() as i64;
    let author_name = ctx.author().name.clone();

    // Resolve event name
    let event_name = match event_name {
        Some(n) => n,
        None => queries::get_latest_event_name(&ctx.data().db, guild_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No active or ended events found"))?,
    };

    let event = match queries::get_event_by_name(&ctx.data().db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            let embed = CreateEmbed::default()
                .title("Event Not Found")
                .color(0xFF4444)
                .description(format!("Event **{}** was not found.", event_name));
            ctx.send(poise::CreateReply::default().embed(embed)).await?;
            return Ok(());
        }
    };

    // Resolve registered user
    let db_user = match queries::get_user_by_discord_id(&ctx.data().db, user_id, guild_id).await? {
        Some(u) => u,
        None => {
            let embed = CreateEmbed::default()
                .title("Not Registered")
                .color(0xFF4444)
                .description(
                    "You are not registered. Use `/register` to link a Minecraft account.",
                );
            ctx.send(poise::CreateReply::default().embed(embed)).await?;
            return Ok(());
        }
    };

    // Per-stat XP breakdown for this event
    let stats = queries::get_user_event_stats(&ctx.data().db, event.id, db_user.id).await?;

    let total_xp: f64 = stats.iter().map(|(_, xp, _)| *xp).sum();

    // Build stat_deltas: display name + count, sorted desc by XP (already sorted from DB), up to 8
    let mut stat_deltas: Vec<(String, i64)> = stats
        .into_iter()
        .filter(|(_, xp, _)| *xp > 0.0)
        .map(|(key, _, count)| (display_name_for_key(&key), count))
        .collect();
    // Already sorted by XP from the database query
    stat_deltas.truncate(8);

    // User's rank within the event leaderboard
    let rank = queries::get_user_event_rank(&ctx.data().db, event.id, db_user.id).await?;

    // Fetch avatar
    let avatar_bytes = if let Some(tex) = &db_user.head_texture {
        if let Some(encoded) = tex.strip_prefix("data:image/png;base64,") {
            use base64::{Engine as _, engine::general_purpose};
            general_purpose::STANDARD.decode(encoded).ok()
        } else {
            None
        }
    } else {
        fetch_event_avatar(&db_user.minecraft_uuid).await
    };

    // Resolve Minecraft username
    let mc_name = match &db_user.minecraft_username {
        Some(name) => name.clone(),
        None => match ctx
            .data()
            .hypixel
            .resolve_uuid(&db_user.minecraft_uuid)
            .await
        {
            Ok(name) => name,
            Err(_) => db_user.minecraft_uuid.to_string(),
        },
    };

    // Build card params — reuse the level card with event-specific data:
    //   level             = 0   (not applicable for events; hides level display)
    //   xp_this_level     = 0.0 (progress bar will be empty)
    //   xp_for_next_level = 1.0 (avoids division-by-zero in renderer)
    //   stat_deltas       = per-stat event XP earned
    //   rank              = rank within event leaderboard
    //   milestone_progress = empty (not applicable for events)
    //   xp_gained         = total event XP (shown top-right on card)
    let params = LevelCardParams {
        minecraft_username: mc_name,
        level: 0,
        total_xp,
        xp_this_level: 0.0,
        xp_for_next_level: 1.0,
        stat_deltas,
        xp_gained: total_xp,
        avatar_bytes,
        rank,
        milestone_progress: vec![],
        hypixel_rank: db_user.hypixel_rank.clone(),
        hypixel_rank_plus_color: db_user.hypixel_rank_plus_color.clone(),
        event_mode: true,
    };

    let png_bytes = level_card::render(&params);
    let attachment = CreateAttachment::bytes(png_bytes, "event_level_card.png");

    ctx.send(
        poise::CreateReply::default()
            .content(format!("**{}** — Event Level Card", event.name))
            .attachment(attachment),
    )
    .await?;

    info!(
        "Sent event level card for user {} (Discord ID {}) in guild {} for event '{}'",
        author_name, user_id, guild_id, event_name
    );

    Ok(())
}

/// Show aggregated statistics for a specific event as a card image.
#[poise::command(slash_command, guild_only)]
pub async fn statistics(
    ctx: Context<'_>,
    #[description = "Event name (defaults to most recent)"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: Option<String>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx.guild_id().unwrap().get() as i64;

    let event_name = match event_name {
        Some(n) => n,
        None => queries::get_latest_event_name(&ctx.data().db, guild_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No active or ended events found"))?,
    };

    let event = match queries::get_event_by_name(&ctx.data().db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            let embed = poise::serenity_prelude::CreateEmbed::default()
                .title("Event Not Found")
                .color(0xFF4444)
                .description(format!("Event **{}** was not found.", event_name));
            ctx.send(poise::CreateReply::default().embed(embed)).await?;
            return Ok(());
        }
    };

    let stats = queries::get_event_statistics(&ctx.data().db, event.id).await?;

    let subtitle = if let Some(participants) = stats.participants {
        Some(format!("{} participants", participants))
    } else {
        None
    };

    let params = StatisticsCardParams {
        title: format!("{} — Statistics", event.name),
        subtitle,
        stats,
    };

    let png_bytes = statistics_card::render(&params);
    let attachment = CreateAttachment::bytes(png_bytes, "event_statistics.png");

    ctx.send(poise::CreateReply::default().attachment(attachment))
        .await?;

    info!(
        "Sent event statistics card for guild {} event '{}'",
        guild_id, event_name
    );

    Ok(())
}

/// Show milestone completers for an event as a Discord embed.
///
/// Each milestone gets one embed field listing how many players reached it
/// and their Discord mentions (up to Discord's 25-field / 1024-char limits).
#[poise::command(slash_command, guild_only, ephemeral)]
pub async fn milestones(
    ctx: Context<'_>,
    #[description = "Event name (defaults to most recent)"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx.guild_id().unwrap().get() as i64;

    let event_name = match event_name {
        Some(n) => n,
        None => queries::get_latest_event_name(&ctx.data().db, guild_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No active or ended events found"))?,
    };

    let event = match queries::get_event_by_name(&ctx.data().db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{}** not found.", event_name))
                .await?;
            return Ok(());
        }
    };

    let milestones = queries::get_event_milestones(&ctx.data().db, event.id).await?;

    if milestones.is_empty() {
        ctx.say(format!(
            "No milestones have been configured for **{}** yet. Admins can add them with `/edit-event milestones-add`.",
            event.name
        ))
        .await?;
        return Ok(());
    }

    let mut embed = poise::serenity_prelude::CreateEmbed::new()
        .title(format!("Milestones — {}", event.name))
        .color(0x00BFFF);

    // Build one field per milestone (cap at Discord's 25-field limit).
    for milestone in milestones.iter().take(25) {
        let completers = queries::get_event_milestone_completers(
            &ctx.data().db,
            event.id,
            milestone.xp_threshold,
        )
        .await
        .unwrap_or_default();

        let count = completers.len();
        let field_name = format!("{} XP", milestone.xp_threshold as i64);

        // Build mention list, respecting Discord's 1024-char field value limit.
        let mut value = if count == 0 {
            "No players have reached this milestone yet.".to_string()
        } else {
            let mut s = format!("**{count} player{}**\n", if count == 1 { "" } else { "s" });
            for uid in &completers {
                let mention = format!("<@{uid}> ");
                if s.len() + mention.len() > 1020 {
                    s.push_str("…");
                    break;
                }
                s.push_str(&mention);
            }
            s
        };

        // Discord field values must be non-empty and ≤ 1024 chars.
        value.truncate(1024);

        embed = embed.field(field_name, value, false);
    }

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
