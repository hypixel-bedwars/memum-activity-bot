/// `/leaderboard` — user-facing leaderboard command.
///
/// Shows a paginated image leaderboard of the top players in the guild,
/// ranked by total XP. Uses Discord buttons for pagination and a timed
/// cache to avoid regenerating images on every invocation.
use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude::{self as serenity, CreateAttachment, CreateInteractionResponse, CreateInteractionResponseMessage};

use crate::shared::cache::TimedCache;
use crate::shared::types::{Context, Error};

use super::helpers;

/// Type alias for the leaderboard image cache.
/// Key: `(guild_id, page)` — Value: `(png_bytes, total_pages)`.
pub type LeaderboardCache = Arc<TimedCache<(u64, u32), (Vec<u8>, u32)>>;

/// Create a new leaderboard cache with the given TTL in seconds.
pub fn new_cache(ttl_seconds: u64) -> LeaderboardCache {
    Arc::new(TimedCache::new(Duration::from_secs(ttl_seconds)))
}

/// Show a leaderboard image with pagination buttons.
#[poise::command(slash_command, guild_only)]
pub async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server.")?;

    let page: u32 = 1;
    let (png_bytes, total_pages) = get_or_generate(ctx.data(), guild_id.get(), page).await?;

    let attachment = CreateAttachment::bytes(png_bytes, "leaderboard.png".to_string());

    let components = pagination_buttons(page, total_pages);

    ctx.send(
        poise::CreateReply::default()
            .attachment(attachment)
            .components(components),
    )
    .await?;

    Ok(())
}

/// Get a cached leaderboard page or generate a fresh one.
pub async fn get_or_generate(
    data: &crate::shared::types::Data,
    guild_id: u64,
    page: u32,
) -> Result<(Vec<u8>, u32), Error> {
    let cache = &data.leaderboard_cache;
    let key = (guild_id, page);

    if let Some(cached) = cache.get(&key).await {
        return Ok(cached);
    }

    let (png_bytes, total_pages) =
        helpers::generate_leaderboard_page(&data.db, guild_id as i64, page).await?;

    cache.insert(key, (png_bytes.clone(), total_pages)).await;
    Ok((png_bytes, total_pages))
}

/// Build pagination button components.
pub fn pagination_buttons(
    current_page: u32,
    total_pages: u32,
) -> Vec<serenity::CreateActionRow> {
    let prev_disabled = current_page <= 1;
    let next_disabled = current_page >= total_pages;

    let prev_button = serenity::CreateButton::new(format!("lb_page_{}", current_page.saturating_sub(1).max(1)))
        .label("Previous")
        .style(serenity::ButtonStyle::Secondary)
        .disabled(prev_disabled);

    let page_indicator = serenity::CreateButton::new("lb_page_indicator")
        .label(format!("{} / {}", current_page, total_pages))
        .style(serenity::ButtonStyle::Secondary)
        .disabled(true);

    let next_button = serenity::CreateButton::new(format!("lb_page_{}", current_page + 1))
        .label("Next")
        .style(serenity::ButtonStyle::Secondary)
        .disabled(next_disabled);

    vec![serenity::CreateActionRow::Buttons(vec![
        prev_button,
        page_indicator,
        next_button,
    ])]
}

/// Handle leaderboard pagination button interactions.
///
/// This is called from the global event handler when a component interaction
/// with a `lb_page_` prefix custom ID is received.
pub async fn handle_pagination(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    data: &crate::shared::types::Data,
) -> Result<(), Error> {
    let custom_id = &component.data.custom_id;

    // Parse page number from custom_id: "lb_page_3" -> 3
    let page: u32 = custom_id
        .strip_prefix("lb_page_")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let guild_id = component
        .guild_id
        .ok_or("Not in a guild")?;

    // Defer the update (acknowledges the interaction, keeps the message)
    component
        .create_response(
            ctx,
            CreateInteractionResponse::Defer(
                CreateInteractionResponseMessage::new(),
            ),
        )
        .await?;

    let (png_bytes, total_pages) = get_or_generate(data, guild_id.get(), page).await?;

    let attachment = CreateAttachment::bytes(png_bytes, "leaderboard.png".to_string());
    let components = pagination_buttons(page, total_pages);

    component
        .edit_response(
            ctx,
            serenity::EditInteractionResponse::new()
                .new_attachment(attachment)
                .components(components),
        )
        .await?;

    Ok(())
}
