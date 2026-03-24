/// Admin command to recalculate user XP from source data (xp_events + event_xp).
///
/// Global XP is the sum of regular XP (xp_events) plus event XP (event_xp).
/// This command recomputes that sum and updates xp.total_xp and level.
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

        // Calculate correct XP from source tables
        // Global XP = Regular XP (from xp_events) + Event XP (from event_xp)
        let correct_regular_xp: f64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(xp_earned), 0) FROM xp_events WHERE user_id = $1",
        )
        .bind(db_user.id)
        .fetch_one(pool)
        .await?;

        let correct_event_xp: f64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(xp_earned), 0) FROM event_xp WHERE user_id = $1",
        )
        .bind(db_user.id)
        .fetch_one(pool)
        .await?;

        let correct_global_xp = correct_regular_xp;

        // Get current XP from xp table
        let current_xp_row = queries::get_xp(pool, db_user.id).await?;
        let (current_total_xp, current_level) = current_xp_row
            .as_ref()
            .map(|x| (x.total_xp, x.level))
            .unwrap_or((0.0, 1));

        // Calculate correction needed
        let correction = correct_global_xp - current_total_xp;

        // Only process if there's a discrepancy
        if correction.abs() > 0.01 {
            corrected += 1;
            total_correction += correction;

            // Calculate new level
            let new_level = calculator::calculate_level(
                correct_global_xp,
                config.base_level_xp,
                config.level_exponent,
            ) as i32;

            let user_mention = format!("<@{}>", db_user.discord_user_id);
            let correction_msg = format!(
                "**{}**: XP: {:.1} → {:.1} (Δ{:.1}) | Lvl: {} → {} | (Regular: {:.1}, Event: {:.1})",
                user_mention,
                current_total_xp,
                correct_global_xp,
                correction,
                current_level,
                new_level,
                correct_regular_xp,
                correct_event_xp
            );

            corrections.push(correction_msg);

            debug!(
                user_id = db_user.id,
                discord_user_id = db_user.discord_user_id,
                current_xp = current_total_xp,
                correct_xp = correct_regular_xp,
                correction,
                old_xp = current_total_xp,
                new_xp = correct_global_xp,
                correction,
                regular_xp = correct_regular_xp,
                event_xp = correct_event_xp,
                current_level,
                new_level,
                "XP correction calculated."
            );

            // Apply correction if not dry run
            if !is_dry_run {
                let now = Utc::now();

                // Update XP and level
                sqlx::query(
                    "UPDATE xp SET total_xp = $1, level = $2, last_updated = $3 WHERE user_id = $4",
                )
                .bind(correct_global_xp)
                .bind(new_level)
                .bind(&now)
                .bind(db_user.id)
                .execute(pool)
                .await?;

                info!(
                    user_id = db_user.id,
                    discord_user_id = db_user.discord_user_id,
                    old_xp = current_total_xp,
                    new_xp = correct_global_xp,
                    correction,
                    regular_xp = correct_regular_xp,
                    event_xp = correct_event_xp,
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
                        correct_global_xp,
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

    // Add corrections details (limited to avoid Discord's 1024 char field limit)
    if !corrections.is_empty() {
        let mut corrections_text = String::new();
        let mut shown_count = 0;

        // Add corrections until we approach the 1024 character limit (leave buffer for safety)
        for correction in &corrections {
            if corrections_text.len() + correction.len() + 1 > 950 {
                break;
            }
            if shown_count > 0 {
                corrections_text.push('\n');
            }
            corrections_text.push_str(correction);
            shown_count += 1;
        }

        embed = embed.field(
            format!(
                "Corrections (showing {} of {})",
                shown_count,
                corrections.len()
            ),
            if corrections_text.is_empty() {
                "Too many corrections to display. Check logs for details.".to_string()
            } else {
                corrections_text
            },
            false,
        );

        if corrections.len() > shown_count {
            embed = embed.field(
                "Note",
                format!(
                    "{} more corrections not shown. Check logs for full details.",
                    corrections.len() - shown_count
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
