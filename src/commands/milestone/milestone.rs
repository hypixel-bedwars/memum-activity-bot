/// `/milestone` command group.
///
/// Admin subcommands: `add`, `edit`, `remove` — restricted to users listed in
/// `AppConfig.admin_user_ids`.
///
/// Player subcommand: `view` — available to all registered users.
use poise::serenity_prelude as serenity;
use tracing::info;

use crate::database::queries;
use crate::shared::types::{Context, Error};

/// Returns the existing milestone levels for the invoking guild as autocomplete
/// choices. The submitted value is the level as a decimal string so the handler
/// can parse it back to i64 without needing the internal row ID.
async fn autocomplete_existing_milestone<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> Vec<serenity::AutocompleteChoice> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id.get() as i64,
        None => return Vec::new(),
    };

    let milestones = match queries::get_milestones(&ctx.data().db, guild_id).await {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let partial_lower = partial.to_lowercase();

    milestones
        .into_iter()
        .filter(|m| {
            let display = format!("Level {}", m.level);
            display.to_lowercase().contains(&partial_lower)
                || m.level.to_string().contains(&partial_lower)
        })
        .take(25)
        .map(|m| {
            serenity::AutocompleteChoice::new(format!("Level {}", m.level), m.level.to_string())
        })
        .collect()
}

/// Milestone management and progress commands.
#[poise::command(
    slash_command,
    subcommands("add", "edit", "remove", "view"),
    subcommand_required
)]
pub async fn milestone(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Add a new milestone level. Admin only.
///
/// Creates a milestone at the given level threshold. Once added it appears on
/// the leaderboard with a live count of how many users have reached it.
#[poise::command(slash_command, ephemeral, check = "crate::permissions::admin_check")]
pub async fn add(
    ctx: Context<'_>,
    #[description = "The level threshold for this milestone (e.g. 25, 50, 100)"] level: i32,
) -> Result<(), Error> {
    if level < 1 {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("Milestone level must be 1 or greater."),
        )
        .await?;
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;

    let created = queries::add_milestone(&ctx.data().db, guild_id, level).await?;

    if created {
        info!(guild_id, level, "Milestone added.");
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content(format!("Milestone **Level {level}** added successfully.")),
        )
        .await?;
    } else {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content(format!(
                    "A milestone at **Level {level}** already exists for this server."
                )),
        )
        .await?;
    }

    Ok(())
}

/// Edit an existing milestone's level. Admin only.
///
/// Select the milestone you want to change via autocomplete, then supply the
/// new level value. The milestone must not conflict with another existing one.
#[poise::command(slash_command, ephemeral, check = "crate::permissions::admin_check")]
pub async fn edit(
    ctx: Context<'_>,
    #[description = "The current milestone level to edit"]
    #[autocomplete = "autocomplete_existing_milestone"]
    current_level: String,
    #[description = "The new level for this milestone"] new_level: i32,
) -> Result<(), Error> {
    let parsed_current: i32 =
        match current_level.trim().parse() {
            Ok(v) => v,
            Err(_) => {
                ctx.send(poise::CreateReply::default().ephemeral(true).content(
                    "Invalid milestone level. Please select one from the autocomplete list.",
                ))
                .await?;
                return Ok(());
            }
        };

    if new_level < 1 {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("Milestone level must be 1 or greater."),
        )
        .await?;
        return Ok(());
    }

    if parsed_current == new_level {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("The new level is the same as the current level. No changes made."),
        )
        .await?;
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;

    // Look up the milestone by its current level.
    let milestones = queries::get_milestones(&ctx.data().db, guild_id).await?;
    let target = milestones.iter().find(|m| m.level == parsed_current);

    let milestone = match target {
        Some(m) => m.clone(),
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .ephemeral(true)
                    .content(format!(
                        "No milestone found at **Level {parsed_current}** for this server."
                    )),
            )
            .await?;
            return Ok(());
        }
    };

    // Check the new level doesn't conflict with an existing milestone.
    if milestones.iter().any(|m| m.level == new_level) {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content(format!(
                    "A milestone at **Level {new_level}** already exists. \
                     Remove it first before moving this milestone there."
                )),
        )
        .await?;
        return Ok(());
    }

    let updated =
        queries::edit_milestone(&ctx.data().db, guild_id, milestone.id, new_level).await?;

    if updated {
        info!(
            guild_id,
            old_level = parsed_current,
            new_level,
            "Milestone edited."
        );
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content(format!(
                    "Milestone updated: **Level {parsed_current}** → **Level {new_level}**."
                )),
        )
        .await?;
    } else {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("Failed to update the milestone. Please try again."),
        )
        .await?;
    }

    Ok(())
}

/// Remove an existing milestone. Admin only.
///
/// Select the milestone to delete via autocomplete. This action cannot be
/// undone.
#[poise::command(slash_command, ephemeral, check = "crate::permissions::admin_check")]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "The milestone level to remove"]
    #[autocomplete = "autocomplete_existing_milestone"]
    level: String,
) -> Result<(), Error> {
    let parsed_level: i32 =
        match level.trim().parse() {
            Ok(v) => v,
            Err(_) => {
                ctx.send(poise::CreateReply::default().ephemeral(true).content(
                    "Invalid milestone level. Please select one from the autocomplete list.",
                ))
                .await?;
                return Ok(());
            }
        };

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;

    // Look up the milestone by level to get its ID.
    let milestones = queries::get_milestones(&ctx.data().db, guild_id).await?;
    let target = milestones.iter().find(|m| m.level == parsed_level);

    let milestone = match target {
        Some(m) => m.clone(),
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .ephemeral(true)
                    .content(format!(
                        "No milestone found at **Level {parsed_level}** for this server."
                    )),
            )
            .await?;
            return Ok(());
        }
    };

    let removed = queries::remove_milestone(&ctx.data().db, guild_id, milestone.id).await?;

    if removed {
        info!(guild_id, level = parsed_level, "Milestone removed.");
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content(format!("Milestone **Level {parsed_level}** removed.")),
        )
        .await?;
    } else {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("Failed to remove the milestone. Please try again."),
        )
        .await?;
    }

    Ok(())
}

/// Show your progress toward the next milestone.
///
/// Displays the next upcoming milestone level, your current level, and how
/// far along you are. If you have surpassed all milestones, a congratulatory
/// message is shown instead.
#[poise::command(slash_command, ephemeral)]
pub async fn view(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get() as i64;

    let discord_user_id = ctx.author().id.get() as i64;

    // Look up the calling user's DB row.
    let user = queries::get_user_by_discord_id(&ctx.data().db, discord_user_id, guild_id).await?;

    let Some(user) = user else {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("You are not registered. Use `/register` to get started."),
        )
        .await?;
        return Ok(());
    };

    // Fetch XP/level data.
    let xp_row = queries::get_xp(&ctx.data().db, user.id).await?;
    let current_level = xp_row.as_ref().map(|x| x.level).unwrap_or(1);

    // Fetch all milestones for this guild ordered by level ascending.
    let milestones = queries::get_milestones(&ctx.data().db, guild_id).await?;

    if milestones.is_empty() {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("No milestones have been configured for this server yet."),
        )
        .await?;
        return Ok(());
    }

    // Find the next milestone above the player's current level.
    let next_milestone = milestones.iter().find(|m| m.level > current_level);

    let message = match next_milestone {
        Some(m) => {
            let target = m.level;
            let progress_pct = ((current_level as f64 / target as f64) * 100.0).min(100.0) as u64;
            format!(
                "**Next Milestone:** Level {target}\n\
                 **Your Level:** {current_level}\n\
                 **Progress:** {current_level} / {target} ({progress_pct}%)"
            )
        }
        None => {
            // The player has surpassed every milestone.
            let highest = milestones.last().unwrap().level; // safe: milestones is non-empty
            format!(
                "You have reached all milestones!\n\
                 **Highest Milestone:** Level {highest}\n\
                 **Your Level:** {current_level}"
            )
        }
    };

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            serenity::CreateEmbed::new()
                .title("Milestone Progress")
                .description(message)
                .color(0x00bfff),
        ),
    )
    .await?;

    Ok(())
}
