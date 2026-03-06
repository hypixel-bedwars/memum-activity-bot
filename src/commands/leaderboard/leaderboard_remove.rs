/// `/leaderboard_remove` — admin command to remove the persistent leaderboard.
///
/// Deletes the stored leaderboard messages and removes the database row so
/// the background updater stops refreshing.
use poise::serenity_prelude::{self as serenity};
use tracing::info;

use crate::database::queries;
use crate::shared::types::{Context, Error};

/// Remove the persistent leaderboard for this server.
///
/// Deletes the leaderboard messages from the channel and stops automatic
/// updates.
#[poise::command(slash_command, guild_only, rename = "leaderboard_remove")]
pub async fn leaderboard_remove(ctx: Context<'_>) -> Result<(), Error> {
    // Admin check
    if !ctx
        .data()
        .config
        .admin_user_ids
        .contains(&ctx.author().id.get())
    {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("You do not have permission to use this command."),
        )
        .await?;
        return Ok(());
    }

    ctx.defer_ephemeral().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server.")?;

    let existing =
        queries::get_persistent_leaderboard(&ctx.data().db, guild_id.get() as i64).await?;

    match existing {
        Some(lb) => {
            // Try to delete the messages
            let msg_ids: Vec<u64> = serde_json::from_value(lb.message_ids.clone()).unwrap_or_default();
            let channel_id = serenity::ChannelId::new(lb.channel_id as u64);

            let mut deleted = 0;
            for msg_id in &msg_ids {
                if channel_id
                    .delete_message(&ctx.http(), serenity::MessageId::new(*msg_id))
                    .await
                    .is_ok()
                {
                    deleted += 1;
                }
            }

            // Remove from database
            queries::delete_persistent_leaderboard(&ctx.data().db, guild_id.get() as i64).await?;

            ctx.send(
                poise::CreateReply::default()
                    .ephemeral(true)
                    .content(format!(
                        "Persistent leaderboard removed. Deleted {deleted}/{} message(s) and stopped automatic updates.",
                        msg_ids.len()
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
        }
    }
    
    info!("Persistent leaderboard removed for guild {}", guild_id);

    Ok(())
}
