use poise::serenity_prelude::{self as serenity, CreateEmbed};
use tracing::{error, info};

use crate::database::queries;
use crate::shared::types::{Context, Error};

/// Audit registered users in this guild.
///
/// Optionally pass `fix = true` to automatically mark users active/inactive
/// based on whether they are currently present in the guild.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "audit-users",
    check = "crate::utils::permissions::admin_check"
)]
pub async fn audit_users(
    ctx: Context<'_>,
    #[description = "Automatically fix mismatches (mark active/inactive)"] fix: Option<bool>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let guild_i64 = guild_id.get() as i64;
    let http = ctx.http();

    let data = ctx.data();

    // Fetch all users registered in this guild from the DB.
    let db_users = queries::get_all_users_in_guild(&data.db, guild_i64).await?;

    let mut present_and_active = 0usize;
    let mut present_but_inactive = 0usize;
    let mut absent_but_active = 0usize;
    let mut absent_and_inactive = 0usize;

    let mut to_reactivate: Vec<i64> = Vec::new();
    let mut to_deactivate: Vec<i64> = Vec::new();

    // Iterate users and check membership
    for u in db_users {
        let discord_id_u64 = u.discord_user_id as u64;
        let user_present = match serenity::GuildId::new(guild_id.get())
            .member(http, serenity::UserId::new(discord_id_u64))
            .await
        {
            Ok(_) => true,
            Err(_) => false,
        };

        match (user_present, u.active) {
            (true, true) => {
                present_and_active += 1;
            }
            (true, false) => {
                // present but marked inactive in DB
                present_but_inactive += 1;
                to_reactivate.push(u.discord_user_id);
            }
            (false, true) => {
                // absent but still active in DB
                absent_but_active += 1;
                to_deactivate.push(u.discord_user_id);
            }
            (false, false) => {
                absent_and_inactive += 1;
            }
        }
    }

    // Optionally fix mismatches
    let do_fix = fix.unwrap_or(false);
    let mut reactivated = 0usize;
    let mut deactivated = 0usize;

    if do_fix {
        let now = chrono::Utc::now();
        for discord_id in &to_reactivate {
            let res = queries::mark_user_active(&data.db, *discord_id, guild_i64).await;
            match res {
                Ok(_) => reactivated += 1,
                Err(e) => error!(
                    discord_user_id = *discord_id,
                    error = %e,
                    "Failed to mark user active during audit"
                ),
            }
        }
        for discord_id in &to_deactivate {
            let res = queries::mark_user_inactive(&data.db, *discord_id, guild_i64, &now).await;
            match res {
                Ok(_) => deactivated += 1,
                Err(e) => error!(
                    discord_user_id = *discord_id,
                    error = %e,
                    "Failed to mark user inactive during audit"
                ),
            }
        }
    }

    // Build a concise report for the admin (include a few example IDs)
    let mut description = String::new();
    description.push_str(&format!(
        "Total registered users scanned: {}\n\n",
        present_and_active + present_but_inactive + absent_but_active + absent_and_inactive
    ));
    description.push_str(&format!("Present & Active: {}\n", present_and_active));
    description.push_str(&format!(
        "Present but marked inactive: {}{}\n",
        present_but_inactive,
        if do_fix {
            format!(" ({} reactivated)", reactivated)
        } else {
            "".to_string()
        }
    ));
    description.push_str(&format!(
        "Absent but still active: {}{}\n",
        absent_but_active,
        if do_fix {
            format!(" ({} deactivated)", deactivated)
        } else {
            "".to_string()
        }
    ));
    description.push_str(&format!("Absent & inactive: {}\n\n", absent_and_inactive));

    // Add short example lists (up to 10) for quick inspection
    let examples_show = 10usize;
    if !to_reactivate.is_empty() {
        let sample: Vec<String> = to_reactivate
            .iter()
            .take(examples_show)
            .map(|id| format!("<@{}>", id))
            .collect();
        description.push_str(&format!(
            "Example users to reactivate ({}): {}\n",
            to_reactivate.len(),
            sample.join(", ")
        ));
    }
    if !to_deactivate.is_empty() {
        let sample: Vec<String> = to_deactivate
            .iter()
            .take(examples_show)
            .map(|id| format!("<@{}>", id))
            .collect();
        description.push_str(&format!(
            "Example users to deactivate ({}): {}\n",
            to_deactivate.len(),
            sample.join(", ")
        ));
    }

    let mut embed = CreateEmbed::default()
        .title("User Audit Report")
        .description(description)
        .color(0x00BFFF);

    if do_fix {
        embed = embed.field(
            "Action",
            "Applied fixes (reactivate/deactivate) where needed",
            false,
        );
    } else {
        embed = embed.field(
            "Action",
            "Dry run: no changes made. Re-run with `fix=true` to apply fixes",
            false,
        );
    }

    ctx.send(poise::CreateReply::default().ephemeral(true).embed(embed))
        .await?;

    info!(
        guild_id = guild_i64,
        total_scanned =
            present_and_active + present_but_inactive + absent_but_active + absent_and_inactive,
        "User audit completed"
    );

    Ok(())
}
