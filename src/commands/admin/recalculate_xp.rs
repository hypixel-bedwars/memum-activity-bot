/// Admin command to recalculate user XP from source data (xp_events table).
///
/// This command fixes XP corruption caused by event XP being incorrectly added
/// to the global total_xp field. It recalculates correct XP from xp_events
/// (the source of truth for regular XP) and updates the xp table accordingly.
///
/// Event XP should only exist in the event_xp table for event-specific
/// leaderboards, not in the global xp.total_xp field.
use chrono::Utc;
use poise::serenity_prelude::{self as serenity, CreateEmbed};
use tracing::{debug, info};

use crate::commands::logger::logger::{LogType, logger};
use crate::database::queries;
use crate::shared::types::{Context, Error};
use crate::xp::calculator;

#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn recalculate_xp(
    ctx: Context<'_>,
    #[description = "User to recalculate (leave empty to process all users in the guild)"]
    user: Option<serenity::User>,
    #[description = "Preview changes without applying them"] dry_run: Option<bool>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server.")?;
    let guild_id_i64 = guild_id.get() as i64;

    let is_dry_run = dry_run.unwrap_or(false);
    let mode = if is_dry_run { "DRY RUN" } else { "LIVE" };

    let pool = &ctx.data().db;
    let config = &ctx.data().config;

    // Determine which users to process
    let target_users = if let Some(user) = user {
        // Single user mode
        let db_user = queries::get_user_by_discord_id(pool, user.id.get() as i64, guild_id_i64)
            .await?
            .ok_or_else(|| format!("User {} is not registered in this guild.", user.tag()))?;
        vec![db_user]
    } else {
        // All users mode
        queries::get_all_users_in_guild(pool, guild_id_i64).await?
    };

    if target_users.is_empty() {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("No users found to process."),
        )
        .await?;
        return Ok(());
    }

    info!(
        mode,
        user_count = target_users.len(),
        "Starting XP recalculation."
    );

    let mut processed = 0;
    let mut corrected = 0;
    let mut total_correction: f64 = 0.0;
    let mut corrections: Vec<String> = Vec::new();

    for db_user in &target_users {
        processed += 1;

        // Calculate correct regular XP from xp_events table (source of truth)
        let correct_regular_xp: f64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(xp_earned), 0) FROM xp_events WHERE user_id = $1",
        )
        .bind(db_user.id)
        .fetch_one(pool)
        .await?;

        // Calculate total event XP that was incorrectly added
        let total_event_xp: f64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(xp_earned), 0) FROM event_xp WHERE user_id = $1",
        )
        .bind(db_user.id)
        .fetch_one(pool)
        .await?;

        // Get current XP from xp table
        let current_xp_row = queries::get_xp(pool, db_user.id).await?;
        let (current_total_xp, current_level) = current_xp_row
            .as_ref()
            .map(|x| (x.total_xp, x.level))
            .unwrap_or((0.0, 1));

        // Calculate correction needed
        let correction = correct_regular_xp - current_total_xp;

        // Only process if there's a discrepancy
        if correction.abs() > 0.01 {
            corrected += 1;
            total_correction += correction;

            // Calculate new level
            let new_level = calculator::calculate_level(
                correct_regular_xp,
                config.base_level_xp,
                config.level_exponent,
            ) as i32;

            let user_mention = format!("<@{}>", db_user.discord_user_id);
            let correction_msg = format!(
                "**{}**: XP: {:.1} → {:.1} (change: {:.1}) | Level: {} → {} | Event XP (removed): {:.1}",
                user_mention,
                current_total_xp,
                correct_regular_xp,
                correction,
                current_level,
                new_level,
                total_event_xp
            );

            corrections.push(correction_msg);

            debug!(
                user_id = db_user.id,
                discord_user_id = db_user.discord_user_id,
                current_xp = current_total_xp,
                correct_xp = correct_regular_xp,
                correction,
                current_level,
                new_level,
                event_xp_removed = total_event_xp,
                "XP correction calculated."
            );

            // Apply correction if not dry run
            if !is_dry_run {
                let now = Utc::now();

                // Update XP and level
                sqlx::query(
                    "UPDATE xp SET total_xp = $1, level = $2, last_updated = $3 WHERE user_id = $4",
                )
                .bind(correct_regular_xp)
                .bind(new_level)
                .bind(&now)
                .bind(db_user.id)
                .execute(pool)
                .await?;

                info!(
                    user_id = db_user.id,
                    discord_user_id = db_user.discord_user_id,
                    old_xp = current_total_xp,
                    new_xp = correct_regular_xp,
                    correction,
                    old_level = current_level,
                    new_level,
                    "XP recalculated and updated."
                );

                // Log to guild log channel
                if let Err(e) = logger(
                    ctx.serenity_context(),
                    ctx.data(),
                    guild_id,
                    LogType::Info,
                    format!(
                        "XP recalculated for <@{}>: {:.1} → {:.1} XP ({}), Level {} → {}",
                        db_user.discord_user_id,
                        current_total_xp,
                        correct_regular_xp,
                        if correction > 0.0 {
                            format!("+{:.1}", correction)
                        } else {
                            format!("{:.1}", correction)
                        },
                        current_level,
                        new_level
                    ),
                )
                .await
                {
                    debug!(error = ?e, "Failed to log XP recalculation to guild log channel");
                }
            }
        }
    }

    // Build response embed
    let mut embed = CreateEmbed::new()
        .title(format!("XP Recalculation {}", mode))
        .color(if is_dry_run { 0xFFA500 } else { 0x00FF00 })
        .field("Users Processed", processed.to_string(), true)
        .field("Users Corrected", corrected.to_string(), true)
        .field(
            "Total XP Change",
            format!(
                "{} XP",
                if total_correction > 0.0 {
                    format!("+{:.1}", total_correction)
                } else {
                    format!("{:.1}", total_correction)
                }
            ),
            true,
        );

    if is_dry_run {
        embed = embed.description("**Preview Mode** - No changes were applied. Run without `dry_run:true` to apply corrections.");
    } else {
        embed = embed.description("✅ XP corrections have been applied successfully.");
    }

    // Add corrections details (limited to first 10 to avoid message length issues)
    if !corrections.is_empty() {
        let preview_count = corrections.len().min(10);
        let corrections_text = corrections[..preview_count].join("\n");

        embed = embed.field(
            format!(
                "Corrections (showing {} of {})",
                preview_count,
                corrections.len()
            ),
            corrections_text,
            false,
        );

        if corrections.len() > 10 {
            embed = embed.field(
                "Note",
                format!(
                    "{} more corrections not shown. Check logs for full details.",
                    corrections.len() - 10
                ),
                false,
            );
        }
    } else {
        embed = embed.field(
            "Status",
            "No corrections needed - all XP values are correct!",
            false,
        );
    }

    ctx.send(poise::CreateReply::default().ephemeral(true).embed(embed))
        .await?;

    info!(
        mode,
        processed, corrected, total_correction, "XP recalculation completed."
    );

    Ok(())
}
