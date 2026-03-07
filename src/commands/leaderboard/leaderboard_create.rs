/// `/leaderboard_create [channel]` — admin command to set up a persistent leaderboard.
///
/// Sends leaderboard page images to the specified channel and stores the
/// message IDs in the database so the background updater can edit them
/// periodically.
use poise::serenity_prelude::{self as serenity, CreateAttachment, CreateMessage};
use tracing::info;

use crate::database::queries;
use crate::shared::types::{Context, Error};

use super::helpers::{self, PAGE_SIZE};

/// Create a persistent leaderboard in the specified channel.
///
/// The bot sends one message per page (up to `PERSISTENT_LEADERBOARD_PLAYERS / 10`
/// pages) and stores the message IDs for automatic updates.
#[poise::command(
    slash_command,
    guild_only,
    rename = "leaderboard_create",
    check = "crate::permissions::admin_check"
)]
pub async fn leaderboard_create(
    ctx: Context<'_>,
    #[description = "Channel to post the leaderboard in"] channel: serenity::Channel,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server.")?;

    let channel_id = channel.id();

    // Check if there's already a persistent leaderboard for this guild
    if let Some(existing) =
        queries::get_persistent_leaderboard(&ctx.data().db, guild_id.get() as i64).await?
    {
        // Try to clean up old messages
        let old_msg_ids: Vec<u64> =
            serde_json::from_value(existing.message_ids.clone()).unwrap_or_default();
        let old_channel = serenity::ChannelId::new(existing.channel_id as u64);
        for msg_id in old_msg_ids {
            let _ = old_channel
                .delete_message(&ctx.http(), serenity::MessageId::new(msg_id))
                .await;
        }
    }

    let persistent_players = ctx.data().config.persistent_leaderboard_players;
    let total_pages = ((persistent_players as f64) / PAGE_SIZE as f64)
        .ceil()
        .max(1.0) as u32;

    let mut message_ids: Vec<u64> = Vec::new();

    for page in 1..=total_pages {
        let result =
            helpers::generate_leaderboard_page(&ctx.data().db, guild_id.get() as i64, page).await;

        let (png_bytes, _) = match result {
            Ok(v) => v,
            Err(e) => {
                ctx.send(
                    poise::CreateReply::default()
                        .ephemeral(true)
                        .content(format!("Failed to generate leaderboard page {page}: {e}")),
                )
                .await?;
                return Ok(());
            }
        };

        let attachment = CreateAttachment::bytes(png_bytes, format!("leaderboard_page_{page}.png"));
        let msg = channel_id
            .send_message(&ctx.http(), CreateMessage::new().add_file(attachment))
            .await?;

        message_ids.push(msg.id.get());
    }

    let unix_time = time::OffsetDateTime::now_utc().unix_timestamp();

    let status_msg = channel_id
        .send_message(
            &ctx.http(),
            CreateMessage::new().content(format!(
                "Last Fully Updated: <t:{unix_time}>\n\
                 -# This is the last updated date of the most outdated player data."
            )),
        )
        .await?;

    let status_message_id = status_msg.id.get();

    // Store in database
    let now = chrono::Utc::now();
    let message_ids_json = serde_json::json!(message_ids);

    queries::upsert_persistent_leaderboard(
        &ctx.data().db,
        guild_id.get() as i64,
        channel_id.get() as i64,
        &message_ids_json,
        status_message_id as i64,
        &now,
        &now,
    )
    .await?;

    ctx.send(
        poise::CreateReply::default()
            .ephemeral(true)
            .content(format!(
                "Persistent leaderboard created in <#{}>! It will auto-update every {} seconds.\n\
                 Showing top {} players across {} page(s).",
                channel_id.get(),
                ctx.data().config.leaderboard_cache_seconds,
                persistent_players,
                total_pages,
            )),
    )
    .await?;

    info!(
        "Created persistent leaderboard for guild {} in channel {}",
        guild_id, channel_id
    );

    // send a follow-up message that has the time when it was last updated (so the admin can see that it worked and know when it will update next)
    // it will look something like this
    //
    // Last Fully Updated: <t:1724958002>
    // -# This is the last updated date of the most outdated player data. Data for any player can be updated using the /tournament command.

    Ok(())
}
