use serenity::all::{FullEvent, GuildId, RoleId};
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

            // Count every non-bot guild message as a raw total (before validation).
            increment_stat_by(
                &data.db,
                data,
                new_message.author.id.get() as i64,
                guild_id.get() as i64,
                "total_messages_raw",
                1.0,
            )
            .await;

            if !validate_message(
                new_message.author.id.get() as i64,
                &new_message.content,
                data,
            ) {
                return Ok(());
            }

            increment_stat_by(
                &data.db,
                data,
                new_message.author.id.get() as i64,
                guild_id.get() as i64,
                "messages_sent",
                1.0,
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

            increment_stat_by(
                &data.db,
                data,
                user_id.get() as i64,
                guild_id.get() as i64,
                "reactions_added",
                1.0,
            )
            .await;
        }

        FullEvent::VoiceStateUpdate { old, new } => {
            handle_voice_state_update(data, old.as_ref(), new).await;
        }

        FullEvent::GuildMemberRemoval { guild_id, user, .. } => {
            let discord_user_id = user.id.get() as i64;
            let guild_i64 = guild_id.get() as i64;
            let now = chrono::Utc::now();

            // mark them left (soft delete)
            if let Err(e) =
                queries::mark_user_inactive(&data.db, discord_user_id, guild_i64, &now).await
            {
                error!(error = %e, "Failed to mark user left on GuildMemberRemoval");
            }
        }

        FullEvent::GuildMemberAddition { new_member } => {
            let discord_user_id = new_member.user.id.get() as i64;
            let guild_i64 = new_member.guild_id.get() as i64;

            // Only reactivate if the user was previously registered in this guild.
            match queries::get_user_by_discord_id(&data.db, discord_user_id, guild_i64).await {
                Ok(Some(_db_user)) => {
                    // Reactivate them (soft un-delete)
                    if let Err(e) =
                        queries::mark_user_active(&data.db, discord_user_id, guild_i64).await
                    {
                        error!(error = %e, "Failed to mark user active on GuildMemberAddition");
                    }

                    // Optional: restore registered role if configured and if we can fetch the member
                    if let Ok(Some(guild_row)) = queries::get_guild(&data.db, guild_i64).await {
                        let guild_config: crate::config::GuildConfig =
                            serde_json::from_value(guild_row.config_json).unwrap_or_default();

                        if let Some(role_id) = guild_config.registered_role_id {
                            if let Ok(member) = GuildId::new(guild_i64 as u64)
                                .member(&data.http, new_member.user.id)
                                .await
                            {
                                // ignore role-add errors but you can log them if desired
                                let _ = member.add_role(&data.http, RoleId::new(role_id)).await;
                            }
                        }
                    }
                }

                // User never registered for this guild — nothing to do.
                Ok(None) => {}

                // DB error while checking registration
                Err(e) => {
                    error!(error = %e, "Failed checking user registration on GuildMemberAddition");
                }
            }
        }

        _ => {}
    }

    Ok(())
}

/// Handle a voice state transition and record voice_minutes when a user leaves.
async fn handle_voice_state_update(
    data: &Data,
    old: Option<&serenity::all::VoiceState>,
    new: &serenity::all::VoiceState,
) {
    // Ignore bots — serenity doesn't expose `member.user.bot` directly on
    // VoiceState, but guild_id lets us gate on guild context at least.
    let Some(guild_id) = new.guild_id else {
        return;
    };

    let discord_user_id = new.user_id.get() as i64;

    let was_in_voice = old.as_ref().and_then(|v| v.channel_id).is_some();
    let is_in_voice = new.channel_id.is_some();

    match (was_in_voice, is_in_voice) {
        // User joined a voice channel — record start time.
        (false, true) => {
            let mut sessions = data.voice_sessions.lock().unwrap();
            sessions.insert(discord_user_id, Utc::now());
            debug!(discord_user_id, "Voice session started.");
        }

        // User left all voice channels — compute duration and record minutes.
        (true, false) => {
            let join_time = {
                let mut sessions = data.voice_sessions.lock().unwrap();
                sessions.remove(&discord_user_id)
            };

            let Some(join_time) = join_time else {
                // Session started before bot was running; nothing to record.
                return;
            };

            let duration = Utc::now().signed_duration_since(join_time);
            let minutes = duration.num_minutes();

            if minutes < 1 {
                debug!(
                    discord_user_id,
                    minutes, "Voice session too short — skipped."
                );
                return;
            }

            debug!(
                discord_user_id,
                minutes, "Voice session ended — recording minutes."
            );

            increment_stat_by(
                &data.db,
                data,
                discord_user_id,
                guild_id.get() as i64,
                "voice_minutes",
                minutes as f64,
            )
            .await;
        }

        // User moved between channels — keep the existing session going.
        (true, true) => {}

        // Already not in voice; no-op.
        (false, false) => {}
    }
}

/// Record command usage (called from command hook)
pub async fn record_command_usage(pool: &PgPool, data: &Data, discord_user_id: i64, guild_id: i64) {
    increment_stat_by(pool, data, discord_user_id, guild_id, "commands_used", 1.0).await;
}

/// Increment a Discord stat by `by` units and immediately apply XP + event XP.
async fn increment_stat_by(
    pool: &PgPool,
    data: &Data,
    discord_user_id: i64,
    guild_id: i64,
    stat_name: &str,
    by: f64,
) {
    if by <= 0.0 {
        return;
    }

    let now = Utc::now();

    // ----------------------------------------------------
    // Lookup user
    // ----------------------------------------------------
    let user = match queries::get_user_by_discord_id(pool, discord_user_id, guild_id).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            warn!(
                discord_user_id,
                guild_id, "user not registered, skipping stat increment"
            );
            return;
        }
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

    let new_value = current + by;

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

    if delta.difference <= 0.0 {
        return;
    }

    // ----------------------------------------------------
    // Load guild XP config so we use per-guild multipliers
    // ----------------------------------------------------
    let guild_config = if let Some(cached) = data.guild_configs.get(&guild_id) {
        cached.clone()
    } else {
        let fetched = match queries::get_guild(pool, guild_id).await {
            Ok(Some(g)) => serde_json::from_value(g.config_json).unwrap_or_default(),
            Ok(None) => GuildConfig::default(),
            Err(e) => {
                error!(error = %e, "failed to fetch guild config");
                GuildConfig::default()
            }
        };
        data.guild_configs.insert(guild_id, fetched.clone());
        fetched
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
    // Award event XP for this delta (post-commit, pool-only)
    // ----------------------------------------------------
    let event_xp = match queries::award_event_xp_for_delta(
        pool,
        guild_id,
        user.id,
        stat_name,
        delta_id,
        delta.difference,
        &now,
    )
    .await
    {
        Ok(xp) => xp,
        Err(e) => {
            error!(error = %e, "failed to award event XP");
            0.0
        }
    };

    // ----------------------------------------------------
    // Apply regular XP to user (pool-only query — runs after transaction commit)
    // ----------------------------------------------------
    if total_xp > 0.0 {
        if let Err(e) = queries::increment_xp(pool, user.id, total_xp, &now).await {
            error!(error = %e, "failed incrementing xp");
            return;
        }
    }

    // If event XP was also earned we need an additional increment (events
    // contribute to the user's global total_xp per spec).
    if event_xp > 0.0 {
        if let Err(e) = queries::increment_xp(pool, user.id, event_xp, &now).await {
            error!(error = %e, "failed incrementing event xp");
        }
    }

    // ----------------------------------------------------
    // Fetch updated XP total and recalculate level (only if any XP changed)
    // ----------------------------------------------------
    if total_xp > 0.0 || event_xp > 0.0 {
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
        event_xp_awarded = event_xp,
        "Discord stat processed"
    );
}
