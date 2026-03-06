/// Discord activity tracking.
///
/// Listens to Discord events (messages, reactions) and records them as dynamic
/// stats in the `discord_stats_snapshot` table. Stats are always tracked for
/// registered users; whether they contribute to XP is controlled solely by the
/// guild's `xp_config` (set via `/edit-stats`).
///
/// The tracker increments cumulative counters. Each event bumps the counter by
/// one and stores a new snapshot row, mirroring the EAV pattern used for
/// Hypixel stats.
use serenity::all::FullEvent;
use sqlx::PgPool;
use tracing::{debug, error};

use crate::database::queries;
use crate::shared::types::{Data, Error};
use crate::discord_stats::validation::validate_message;

/// Handle a Serenity `FullEvent` and record relevant Discord activity.
///
/// This is called from the Poise event handler for every event. It filters for
/// events we care about and silently ignores the rest.
pub async fn handle_event(event: &FullEvent, data: &Data) -> Result<(), Error> {
    match event {
        FullEvent::Message { new_message } => {
            // Ignore bot messages.
            if new_message.author.bot {
                return Ok(());
            }

            let Some(guild_id) = new_message.guild_id else {
                return Ok(()); // DM, ignore.
            };

            if !validate_message(
                new_message.author.id.get() as i64,
                &new_message.content,
                data,
            ) {
                return Ok(());
            }

            increment_stat(
                &data.db,
                new_message.author.id.get() as i64,
                guild_id.get() as i64,
                "messages_sent",
            )
            .await;
        }

        FullEvent::ReactionAdd { add_reaction } => {
            let Some(guild_id) = add_reaction.guild_id else {
                return Ok(());
            };

            // We may not always have user info on reactions depending on cache
            // state, but `user_id` is always present.
            let Some(user_id) = add_reaction.user_id else {
                return Ok(());
            };

            increment_stat(
                &data.db,
                user_id.get() as i64,
                guild_id.get() as i64,
                "reactions_added",
            )
            .await;
        }

        // All other events are ignored.
        _ => {}
    }

    Ok(())
}

/// Record a command usage for the invoking user. Called from the Poise
/// pre-command hook.
pub async fn record_command_usage(pool: &PgPool, discord_user_id: i64, guild_id: i64) {
    increment_stat(pool, discord_user_id, guild_id, "commands_used").await;
}

/// Increment a stat counter for a user. If the user is not registered in the
/// given guild, the event is silently dropped (we only track registered users).
async fn increment_stat(pool: &PgPool, discord_user_id: i64, guild_id: i64, stat_name: &str) {
    // Look up the internal user id.
    let user = match queries::get_user_by_discord_id(pool, discord_user_id, guild_id).await {
        Ok(Some(u)) => u,
        Ok(None) => return, // Not registered, ignore.
        Err(e) => {
            error!(error = %e, "Discord tracker: failed to look up user.");
            return;
        }
    };

    // Get the latest value for this stat (or start from 0).
    let current = match queries::get_latest_discord_snapshot(pool, user.id, stat_name).await {
        Ok(Some(snap)) => snap.stat_value,
        Ok(None) => 0.0,
        Err(e) => {
            error!(error = %e, "Discord tracker: failed to fetch latest snapshot.");
            return;
        }
    };

    let new_value = current + 1.0;
    let now = chrono::Utc::now();

    if let Err(e) =
    
        queries::insert_discord_snapshot(pool, user.id, stat_name, new_value, now).await
    {
        error!(error = %e, "Discord tracker: failed to insert snapshot.");
    } else {
        debug!(
            user_id = user.id,
            stat_name, new_value, "Discord tracker: recorded stat."
        );
    }
}
