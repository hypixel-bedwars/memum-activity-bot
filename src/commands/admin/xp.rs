/// Admin XP management commands.
///
/// Subcommands:
/// - `/xp add @user amount`
/// - `/xp remove @user amount`
use poise::serenity_prelude::{self as serenity, CreateEmbed};
use tracing::info;

use crate::commands::logger::logger::{LogType, logger};
use crate::database::queries;
use crate::shared::types::{Context, Error};
use crate::xp::calculator;

/// Import the award_admin_event_xp function
use crate::database::queries::award_admin_event_xp;

/// Parent command
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    subcommands("add", "remove"),
    check = "crate::utils::permissions::admin_check"
)]
pub async fn xp(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Add XP to a user
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    check = "crate::utils::permissions::admin_check"
)]
pub async fn add(
    ctx: Context<'_>,
    #[description = "User to add XP to"] user: serenity::User,
    #[description = "Amount of XP to add"] amount: f64,
) -> Result<(), Error> {
    let data = ctx.data();
    let pool = &data.db;

    let guild_id = ctx.guild_id().ok_or("Command must be run in a guild")?;
    let db_user =
        queries::get_user_by_discord_id(pool, user.id.get() as i64, guild_id.get() as i64)
            .await?
            .ok_or("User is not registered")?;

    let now = chrono::Utc::now();

    // Update XP
    queries::increment_xp(pool, db_user.id, amount, &now).await?;

    // Award event XP if there are active events
    let event_xp = award_admin_event_xp(pool, guild_id.get() as i64, db_user.id, amount, &now)
        .await
        .unwrap_or(0.0);

    let xp_row = queries::get_xp(pool, db_user.id)
        .await?
        .ok_or("XP row missing")?;

    let new_level = calculator::calculate_level(
        xp_row.total_xp,
        data.config.base_level_xp,
        data.config.level_exponent,
    ) as i32;

    if new_level != xp_row.level {
        queries::update_level(pool, db_user.id, new_level, &now).await?;
    }

    let embed = CreateEmbed::default()
        .title("XP Added")
        .description(format!(
            "Added **{} XP** to <@{}>\nNew total XP: **{}**\nEvent XP awarded: **{}**",
            amount, user.id, xp_row.total_xp, event_xp
        ))
        .color(0x00FFAA);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    info!(
        admin = %ctx.author().name,
        target = %user.id,
        amount,
        "Admin added XP"
    );

    logger(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        LogType::Warn,
        format!(
            "{} added {} XP to <@{}> (new total: {})",
            ctx.author().name,
            amount,
            user.id,
            xp_row.total_xp
        ),
    )
    .await?;

    Ok(())
}

/// Remove XP from a user
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    check = "crate::utils::permissions::admin_check"
)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "User to remove XP from"] user: serenity::User,
    #[description = "Amount of XP to remove"] amount: f64,
) -> Result<(), Error> {
    let data = ctx.data();
    let pool = &data.db;

    let guild_id = ctx.guild_id().ok_or("Command must be run in a guild")?;
    let db_user =
        queries::get_user_by_discord_id(pool, user.id.get() as i64, guild_id.get() as i64)
            .await?
            .ok_or("User is not registered")?;

    let now = chrono::Utc::now();

    queries::increment_xp(pool, db_user.id, -amount, &now).await?;

    // Award event XP if there are active events (negative for remove)
    let event_xp = award_admin_event_xp(pool, guild_id.get() as i64, db_user.id, -amount, &now)
        .await
        .unwrap_or(0.0);

    let xp_row = queries::get_xp(pool, db_user.id)
        .await?
        .ok_or("XP row missing")?;

    let new_level = calculator::calculate_level(
        xp_row.total_xp,
        data.config.base_level_xp,
        data.config.level_exponent,
    ) as i32;

    if new_level != xp_row.level {
        queries::update_level(pool, db_user.id, new_level, &now).await?;
    }

    let embed = CreateEmbed::default()
        .title("XP Removed")
        .description(format!(
            "Removed **{} XP** from <@{}>\nNew total XP: **{}**\nEvent XP adjusted: **{}**",
            amount, user.id, xp_row.total_xp, event_xp
        ))
        .color(0xFF5555);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    info!(
        admin = %ctx.author().name,
        target = %user.id,
        amount,
        "Admin removed XP"
    );

    logger(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        LogType::Warn,
        format!(
            "{} removed {} XP from <@{}> (new total: {})",
            ctx.author().name,
            amount,
            user.id,
            xp_row.total_xp
        ),
    )
    .await?;

    Ok(())
}
