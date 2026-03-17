/// `/leaderboard_remove` — admin command to remove the persistent leaderboard.
///
/// Deletes the stored leaderboard messages and removes the database row so
/// the background updater stops refreshing.
use poise::serenity_prelude::{self as serenity};
use tracing::info;

use crate::commands::logger::logger::{LogType, logger};
use crate::database::queries;
use crate::shared::types::{Context, Error};

/// Remove the persistent leaderboard for this server.
///
/// Deletes the leaderboard messages from the channel and stops automatic
/// updates.
#[poise::command(
    slash_command,
    guild_only,
    rename = "leaderboard-remove",
    check = "crate::utils::permissions::admin_check"
)]
pub async fn leaderboard_remove(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server.")?;

    let existing =
        queries::get_persistent_leaderboard(&ctx.data().db, guild_id.get() as i64).await?;

    match existing {
        Some(lb) => {
            let channel_id = serenity::ChannelId::new(lb.channel_id as u64);

            let msg_ids: Vec<u64> =
                serde_json::from_value(lb.message_ids.clone()).unwrap_or_default();

            let mut deleted = 0;

            // Delete leaderboard page messages
            for msg_id in &msg_ids {
                if channel_id
                    .delete_message(&ctx.http(), serenity::MessageId::new(*msg_id))
                    .await
                    .is_ok()
                {
                    deleted += 1;
                }
            }

            // Delete milestone message
            if lb.milestone_message_id != 0 {
                if channel_id
                    .delete_message(
                        &ctx.http(),
                        serenity::MessageId::new(lb.milestone_message_id as u64),
                    )
                    .await
                    .is_ok()
                {
                    deleted += 1;
                }
            }

            // Delete status message
            if lb.status_message_id != 0 {
                if channel_id
                    .delete_message(
                        &ctx.http(),
                        serenity::MessageId::new(lb.status_message_id as u64),
                    )
                    .await
                    .is_ok()
                {
                    deleted += 1;
                }
            }

            // Remove from database
            queries::delete_persistent_leaderboard(&ctx.data().db, guild_id.get() as i64).await?;

            logger(
                ctx.serenity_context(),
                ctx.data(),
                guild_id,
                LogType::Warn,
                format!(
                    "{} removed the persistent leaderboard in <#{}> ({} messages deleted)",
                    ctx.author().name,
                    lb.channel_id,
                    deleted
                ),
            )
            .await?;

            ctx.send(
                poise::CreateReply::default()
                    .ephemeral(true)
                    .content(format!(
                        "Persistent leaderboard removed. Deleted {} message(s).",
                        deleted
                    )),
            )
            .await?;
        }
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .ephemeral(true)
                    .content("No persistent leaderboard exists for this server. Use `/leaderboard_create` to create one."),
            )
            .await?;

            logger(
                ctx.serenity_context(),
                ctx.data(),
                guild_id,
                LogType::Info,
                format!(
                    "{} attempted to remove a persistent leaderboard but none existed",
                    ctx.author().name
                ),
            )
            .await?;
        }
    }

    info!("Persistent leaderboard removed for guild {}", guild_id);

    Ok(())
}
