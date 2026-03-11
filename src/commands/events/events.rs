/// `/events` command group -- public, guild-only.
///
/// Provides subcommands for viewing guild events:
/// - `list` -- list all events with their status
/// - `view` -- show an event's details, tracked stats, and leaderboard
/// - `me`   -- show your personal stats for a specific event
use poise::serenity_prelude::{self as serenity, CreateEmbed};

use crate::database::queries;
use crate::shared::types::{Context, Error};
use crate::utils::stats_definitions::display_name_for_key;

// =========================================================================
// Autocomplete helpers
// =========================================================================

/// Autocomplete for event names -- returns all events (any status).
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

// =========================================================================
// Parent command
// =========================================================================

/// View guild events and leaderboards.
#[poise::command(
    slash_command,
    guild_only,
    subcommands("list", "view", "me"),
    subcommand_required
)]
pub async fn events(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

// =========================================================================
// Subcommands
// =========================================================================

/// List all events for this server.
#[poise::command(slash_command, guild_only, ephemeral)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let events = queries::list_events(&data.db, guild_id).await?;

    if events.is_empty() {
        ctx.say("No events have been created yet.").await?;
        return Ok(());
    }

    let mut description = String::new();

    for event in &events {
        let status_emoji = match event.status.as_str() {
            "pending" => ":clock3:",
            "active" => ":green_circle:",
            "ended" => ":red_circle:",
            _ => ":grey_question:",
        };

        description.push_str(&format!(
            "{} **{}** -- {} | <t:{}:d> to <t:{}:d>\n",
            status_emoji,
            event.name,
            event.status,
            event.start_date.timestamp(),
            event.end_date.timestamp(),
        ));

        if let Some(ref desc) = event.description {
            if !desc.is_empty() {
                description.push_str(&format!("  > {}\n", desc));
            }
        }
    }

    let embed = CreateEmbed::default()
        .title(format!("Events -- {} total", events.len()))
        .description(description)
        .color(0x00BFFF);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// View details and leaderboard for a specific event.
#[poise::command(slash_command, guild_only, ephemeral)]
pub async fn view(
    ctx: Context<'_>,
    #[description = "Event to view"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();

    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{event_name}** not found."))
                .await?;
            return Ok(());
        }
    };

    // Event info section
    let status_emoji = match event.status.as_str() {
        "pending" => ":clock3:",
        "active" => ":green_circle:",
        "ended" => ":red_circle:",
        _ => ":grey_question:",
    };

    let mut info = format!(
        "{} **Status:** {}\n\
         **Start:** <t:{}:F>\n\
         **End:** <t:{}:F>",
        status_emoji,
        event.status,
        event.start_date.timestamp(),
        event.end_date.timestamp(),
    );

    if let Some(ref desc) = event.description {
        if !desc.is_empty() {
            info = format!("*{}*\n\n{}", desc, info);
        }
    }

    // Tracked stats section
    let stats = queries::get_event_stats(&data.db, event.id).await?;
    if !stats.is_empty() {
        info.push_str("\n\n**Tracked Stats:**\n");
        for s in &stats {
            let display = display_name_for_key(&s.stat_name);
            info.push_str(&format!("- {} -- `{} XP/unit`\n", display, s.xp_per_unit));
        }
    }

    // Leaderboard section (top 10)
    let leaderboard = queries::get_event_leaderboard(&data.db, event.id, 10).await?;

    if leaderboard.is_empty() {
        info.push_str("\n\n**Leaderboard:** No participants yet.");
    } else {
        info.push_str("\n\n**Leaderboard (Top 10):**\n");
        for (i, entry) in leaderboard.iter().enumerate() {
            let rank = i + 1;
            let name = entry.minecraft_username.as_deref().unwrap_or("Unknown");
            let mention = format!("<@{}>", entry.discord_user_id);
            info.push_str(&format!(
                "**#{rank}** {name} ({mention}) -- **{:.1} XP**\n",
                entry.total_event_xp
            ));
        }
    }

    let embed = CreateEmbed::default()
        .title(format!("Event: {}", event.name))
        .description(info)
        .color(0x00BFFF);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

/// Show your personal stats for a specific event.
#[poise::command(slash_command, guild_only, ephemeral)]
pub async fn me(
    ctx: Context<'_>,
    #[description = "Event to check"]
    #[autocomplete = "autocomplete_event_name"]
    event_name: String,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;
    let data = ctx.data();
    let discord_user_id = ctx.author().id.get() as i64;

    // Look up event
    let event = match queries::get_event_by_name(&data.db, guild_id, &event_name).await? {
        Some(e) => e,
        None => {
            ctx.say(format!("Event **{event_name}** not found."))
                .await?;
            return Ok(());
        }
    };

    // Look up user
    let user = match queries::get_user_by_discord_id(&data.db, discord_user_id, guild_id).await? {
        Some(u) => u,
        None => {
            ctx.say("You are not registered. Use `/register` to get started.")
                .await?;
            return Ok(());
        }
    };

    // Get per-stat breakdown
    let user_stats = queries::get_user_event_stats(&data.db, event.id, user.id).await?;

    if user_stats.is_empty() {
        ctx.say(format!(
            "You have no recorded stats for event **{}**.",
            event.name
        ))
        .await?;
        return Ok(());
    }

    let total_xp: f64 = user_stats.iter().map(|(_, xp)| xp).sum();

    let mut description = format!(
        "**Event:** {}\n**Status:** {}\n**Your Total Event XP:** {:.1}\n\n**Breakdown:**\n",
        event.name, event.status, total_xp
    );

    for (stat_name, xp) in &user_stats {
        let display = display_name_for_key(stat_name);
        description.push_str(&format!("- {} -- **{:.1} XP**\n", display, xp));
    }

    // Find their rank on the leaderboard
    let leaderboard = queries::get_event_leaderboard(&data.db, event.id, 1000).await?;
    if let Some(pos) = leaderboard
        .iter()
        .position(|e| e.discord_user_id == discord_user_id)
    {
        description.push_str(&format!(
            "\n**Your Rank:** #{} out of {} participants",
            pos + 1,
            leaderboard.len()
        ));
    }

    let embed = CreateEmbed::default()
        .title(format!("Your Stats: {}", event.name))
        .description(description)
        .color(0x00BFFF);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
