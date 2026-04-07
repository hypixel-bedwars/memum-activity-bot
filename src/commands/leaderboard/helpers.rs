use chrono::{DateTime, Utc};
/// Shared leaderboard generation logic.
///
/// Provides helpers that query the database, fetch avatars, and render
/// leaderboard page images and the standalone milestone card.
/// Used by both the `/leaderboard` user command and the persistent
/// leaderboard background updater.
use sqlx::PgPool;

use crate::cards::leaderboard_card::{
    self, EventMilestoneCardParams, EventMilestoneEntry, LeaderboardCardParams, LeaderboardRow,
    MilestoneCardParams, MilestoneEntry,
};
use crate::database::queries;

/// Players per leaderboard page (fixed).
pub const PAGE_SIZE: i64 = 10;

/// Generate a leaderboard PNG for a specific page of a guild.
///
/// Returns `(png_bytes, total_pages)`.
pub async fn generate_leaderboard_page(
    pool: &PgPool,
    guild_id: i64,
    page: u32,
) -> Result<(Vec<u8>, u32), Box<dyn std::error::Error + Send + Sync>> {
    let total_users = queries::count_users_in_guild(pool, guild_id).await?;
    let total_pages = ((total_users as f64) / PAGE_SIZE as f64).ceil().max(1.0) as u32;

    let clamped_page = page.clamp(1, total_pages);
    let offset = ((clamped_page - 1) as i64) * PAGE_SIZE;

    let entries = queries::get_leaderboard(pool, guild_id, offset, PAGE_SIZE).await?;

    // Fetch avatars concurrently.
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let avatar_futures: Vec<_> = entries
        .iter()
        .map(|entry| {
            let url = format!("https://minotar.net/avatar/{}/{}", entry.minecraft_uuid, 80);
            let client = http.clone();
            async move {
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        resp.bytes().await.ok().map(|b| b.to_vec())
                    }
                    _ => None,
                }
            }
        })
        .collect();

    let avatars = futures::future::join_all(avatar_futures).await;

    let rows: Vec<LeaderboardRow> = entries
        .iter()
        .zip(avatars.into_iter())
        .enumerate()
        .map(|(i, (entry, avatar))| {
            let rank = offset as u32 + i as u32 + 1;
            let username = entry
                .minecraft_username
                .clone()
                .unwrap_or_else(|| format!("User#{}", entry.discord_user_id));
            LeaderboardRow {
                rank,
                username,
                level: entry.level,
                total_xp: entry.total_xp,
                avatar_bytes: avatar,
                hypixel_rank: entry.hypixel_rank.clone(),
                hypixel_rank_plus_color: entry.hypixel_rank_plus_color.clone(),
                requirement_met: Some(false), // Guild leaderboard doesn't have milestones, so this is always false.
            }
        })
        .collect();

    let params = LeaderboardCardParams {
        rows,
        page: clamped_page,
        total_pages,
        title: None,
        show_level: true,
        custom_empty_message: None,
        display_limit: None, // Regular leaderboard shows all users
    };

    let png_bytes = leaderboard_card::render(&params);
    Ok((png_bytes, total_pages))
}

/// Generate an event leaderboard PNG for a specific page.
///
/// `event_status` should be `"active"`, `"pending"`, or `"ended"`.
/// `event_start_ts` is the Unix timestamp of the event start (used for the
/// pending-state empty message).
///
/// Returns `(png_bytes, total_pages)`.
pub async fn generate_event_leaderboard_page(
    pool: &PgPool,
    event_id: i64,
    event_name: &str,
    event_status: &str,
    event_start_ts: i64,
    page: u32,
    display_limit: Option<i64>,
) -> Result<(Vec<u8>, u32), Box<dyn std::error::Error + Send + Sync>> {
    let total_participants = queries::count_event_participants(pool, event_id).await?;
    // Apply display limit if provided
    let effective_count = display_limit
        .map(|limit| total_participants.min(limit))
        .unwrap_or(total_participants);
    let total_pages = ((effective_count as f64) / PAGE_SIZE as f64)
        .ceil()
        .max(1.0) as u32;

    let clamped_page = page.clamp(1, total_pages);
    let offset = ((clamped_page - 1) as i64) * PAGE_SIZE;

    let entries = queries::get_event_leaderboard(pool, event_id, PAGE_SIZE, offset).await?;

    // Fetch avatars concurrently from Minotar using the stored UUID.
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let avatar_futures: Vec<_> = entries
        .iter()
        .map(|entry| {
            let url = format!("https://minotar.net/avatar/{}/{}", entry.minecraft_uuid, 80);
            let client = http.clone();
            async move {
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        resp.bytes().await.ok().map(|b| b.to_vec())
                    }
                    _ => None,
                }
            }
        })
        .collect();

    let avatars = futures::future::join_all(avatar_futures).await;

    let rows: Vec<LeaderboardRow> = entries
        .iter()
        .zip(avatars.into_iter())
        .enumerate()
        .map(|(i, (entry, avatar))| {
            let rank = offset as u32 + i as u32 + 1;
            let username = entry
                .minecraft_username
                .clone()
                .unwrap_or_else(|| format!("User#{}", entry.discord_user_id));

            LeaderboardRow {
                rank,
                username,
                // Level is hidden for event leaderboards; pass 0 as a placeholder.
                level: 0,
                total_xp: entry.total_event_xp,
                avatar_bytes: avatar,
                hypixel_rank: entry.hypixel_rank.clone(),
                hypixel_rank_plus_color: entry.hypixel_rank_plus_color.clone(),
                requirement_met: None, // Not used for now
            }
        })
        .collect();

    // conver ts to UTC time
    let dt = DateTime::<Utc>::from_timestamp(event_start_ts, 0);

    // Build the custom empty-state message based on event status.
    let custom_empty_message = if rows.is_empty() {
        Some(match event_status {
            "pending" => format!(
                "Event starts {}",
                dt.map_or_else(
                    || "Unknown".into(),
                    |dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
                )
            ),
            "ended" => "Event has ended — no participants recorded.".to_string(),
            _ => "No participants yet.".to_string(),
        })
    } else {
        None
    };

    let params = LeaderboardCardParams {
        rows,
        page: clamped_page,
        total_pages,
        // Only show title on first page
        title: if clamped_page == 1 {
            Some(event_name.to_string())
        } else {
            None
        },
        show_level: false,
        custom_empty_message,
        display_limit,
    };

    let png_bytes = leaderboard_card::render(&params);
    Ok((png_bytes, total_pages))
}

/// Generate a standalone milestone card PNG for a guild.
///
/// Returns the PNG bytes. Non-fatal errors (e.g. empty milestone list) still
/// produce a valid card with an appropriate empty-state message.
pub async fn generate_milestone_card(
    pool: &PgPool,
    guild_id: i64,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let milestone_data = queries::get_milestones_with_counts(pool, guild_id)
        .await
        .unwrap_or_default();

    let milestones: Vec<MilestoneEntry> = milestone_data
        .into_iter()
        .map(|m| MilestoneEntry {
            level: m.level,
            user_count: m.user_count,
        })
        .collect();

    let total_users = queries::count_users_in_guild(pool, guild_id).await?;

    let params = MilestoneCardParams {
        milestones,
        total_users,
    };

    Ok(leaderboard_card::render_milestone_card(&params))
}

/// Generate a standalone event milestone card PNG.
///
/// Returns `None` if no milestones are configured for this event.
/// Non-fatal errors (e.g. DB failure) produce `Ok(None)`.
pub async fn generate_event_milestone_card(
    pool: &PgPool,
    event_id: i64,
    event_name: &str,
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
    let milestone_data = queries::get_event_milestones_with_counts(pool, event_id)
        .await
        .unwrap_or_default();

    if milestone_data.is_empty() {
        return Ok(None);
    }

    let milestones: Vec<EventMilestoneEntry> = milestone_data
        .into_iter()
        .map(|m| EventMilestoneEntry {
            xp_threshold: m.xp_threshold,
            user_count: m.user_count,
        })
        .collect();

    let total_participants = queries::count_event_participants(pool, event_id).await?;

    let params = EventMilestoneCardParams {
        milestones,
        total_participants,
        event_name: event_name.to_string(),
    };

    Ok(Some(leaderboard_card::render_event_milestone_card(&params)))
}
