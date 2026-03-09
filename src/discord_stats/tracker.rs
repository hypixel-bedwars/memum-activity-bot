use serenity::all::FullEvent;
use sqlx::PgPool;
use tracing::{debug, error, warn};

use chrono::Utc;

use crate::config::GuildConfig;
use crate::database::queries;
use crate::discord_stats::validation::validate_message;
use crate::shared::types::{Data, Error, StatDelta};
use crate::xp::calculator::{XPConfig, calculate_level, calculate_xp_rewards};

/// Handle a Serenity `FullEvent` and record relevant Discord activity.
pub async fn handle_event(event: &FullEvent, data: &Data) -> Result<(), Error> {
    match event {
        FullEvent::Message { new_message } => {
            if new_message.author.bot {
                return Ok(());
            }

            let Some(guild_id) = new_message.guild_id else {
                return Ok(());
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
                data,
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

            let Some(user_id) = add_reaction.user_id else {
                return Ok(());
            };

            increment_stat(
                &data.db,
                data,
                user_id.get() as i64,
                guild_id.get() as i64,
                "reactions_added",
            )
            .await;
        }

        _ => {}
    }

    Ok(())
}

/// Record command usage (called from command hook)
pub async fn record_command_usage(pool: &PgPool, data: &Data, discord_user_id: i64, guild_id: i64) {
    increment_stat(pool, data, discord_user_id, guild_id, "commands_used").await;
}

/// Increment a stat and immediately apply XP.
async fn increment_stat(
    pool: &PgPool,
    data: &Data,
    discord_user_id: i64,
    guild_id: i64,
    stat_name: &str,
) {
    let now = Utc::now();

    // ----------------------------------------------------
    // Lookup user
    // ----------------------------------------------------
    let user = match queries::get_user_by_discord_id(pool, discord_user_id, guild_id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            warn!(
                discord_user_id,
                guild_id,
                "user not registered, skipping stat increment"
            );
            return;
        },
        Err(e) => {
            error!(error = %e, "failed to fetch user");
            return;
        }
    };

    // ----------------------------------------------------
    // Get current stat value (outside transaction — pool-only query)
    // ----------------------------------------------------
    let current = match queries::get_latest_discord_snapshot(pool, user.id, stat_name).await {
        Ok(Some(s)) => s.stat_value,
        Ok(None) => 0.0,
        Err(e) => {
            error!(error = %e, "failed to fetch snapshot");
            return;
        }
    };

    let new_value = current + 1.0;

    // ----------------------------------------------------
    // Insert snapshot (pool-only query — must happen before transaction
    // so the cumulative value is recorded unconditionally, even if the
    // XP pipeline below aborts early due to no rewards being configured)
    // ----------------------------------------------------
    if let Err(e) = queries::insert_discord_snapshot(pool, user.id, stat_name, new_value, now).await
    {
        error!(error = %e, "failed to insert snapshot");
        return;
    }

    // ----------------------------------------------------
    // Build stat delta and check whether it yields any XP before opening
    // a transaction — avoids a no-op transaction for untracked stats
    // ----------------------------------------------------
    let delta = StatDelta::new(user.id, stat_name.to_string(), current, new_value);

    // Guard: only proceed if the difference is positive (it always is here,
    // but be explicit for safety and documentation purposes)
    if delta.difference <= 0.0 {
        return;
    }

    // ----------------------------------------------------
    // Load guild XP config so we use per-guild multipliers
    // ----------------------------------------------------
    let guild_config: GuildConfig = match queries::get_guild(pool, guild_id).await {
        Ok(Some(g)) => serde_json::from_value(g.config_json).unwrap_or_default(),
        Ok(None) => GuildConfig::default(),
        Err(e) => {
            error!(error = %e, "failed to fetch guild config");
            GuildConfig::default()
        }
    };

    let xp_config = XPConfig::new(guild_config.xp_config.clone());

    // ----------------------------------------------------
    // Calculate XP rewards up-front so we can skip the transaction
    // entirely when the stat is not configured for XP
    // ----------------------------------------------------
    let rewards = calculate_xp_rewards(&[delta.clone()], &xp_config);

    let total_xp: f64 = rewards.iter().map(|r| r.xp_earned).sum();

    // ----------------------------------------------------
    // Begin transaction — wraps stat_delta + xp_events + xp + level
    // ----------------------------------------------------
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            error!(error = %e, "failed to start transaction");
            return;
        }
    };

    // ----------------------------------------------------
    // Insert stat_delta row
    // ----------------------------------------------------
    let delta_id = match queries::insert_stat_delta_in_tx(
        &mut tx,
        user.id,
        stat_name,
        current,
        new_value,
        delta.difference,
        "discord",
        &now,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            error!(error = %e, "failed inserting stat delta");
            return;
        }
    };

    // ----------------------------------------------------
    // Insert one xp_event row per reward
    // ----------------------------------------------------
    for reward in &rewards {
        if let Err(e) = queries::insert_xp_event_in_tx(
            &mut tx,
            user.id,
            &reward.stat_name,
            delta_id,
            reward.units as i32,
            reward.xp_per_unit,
            reward.xp_earned,
            &now,
        )
        .await
        {
            error!(error = %e, "failed inserting xp event");
            return;
        }
    }

    // ----------------------------------------------------
    // Commit transaction — stat_delta + xp_events are now durable
    // ----------------------------------------------------
    if let Err(e) = tx.commit().await {
        error!(error = %e, "transaction commit failed");
        return;
    }

    // ----------------------------------------------------
    // Apply XP to user (pool-only query — runs after transaction commit)
    // ----------------------------------------------------
    if total_xp > 0.0 {
        if let Err(e) = queries::increment_xp(pool, user.id, total_xp, &now).await {
            error!(error = %e, "failed incrementing xp");
            return;
        }

        // ----------------------------------------------------
        // Fetch updated XP total and recalculate level
        // ----------------------------------------------------
        let xp_row = match queries::get_xp(pool, user.id).await {
            Ok(Some(x)) => x,
            Ok(None) => {
                error!(user_id = user.id, "xp row missing after increment");
                return;
            }
            Err(e) => {
                error!(error = %e, "failed fetching xp row");
                return;
            }
        };

        let new_level = calculate_level(
            xp_row.total_xp,
            data.config.base_level_xp,
            data.config.level_exponent,
        );

        // Only write to DB when the level has actually changed
        if new_level != xp_row.level {
            if let Err(e) = queries::update_level(pool, user.id, new_level, &now).await {
                error!(error = %e, "failed updating level");
                return;
            }
        }
    }

    debug!(
        user_id = user.id,
        stat_name,
        new_value,
        xp_awarded = total_xp,
        "Discord stat processed"
    );
}
