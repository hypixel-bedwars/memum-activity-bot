/// `/force_register` command.
///
/// Allows an admin to forcibly register a Discord user to a Minecraft account,
/// bypassing Hypixel Discord verification. Use only if the normal registration
/// process is failing for legitimate users.
use tracing::{debug, error, info};

use poise::serenity_prelude::{self as serenity, CreateEmbed};

use crate::commands::logger::logger::{LogType, logger};
use crate::commands::registration::register::fetch_and_cache_head_texture;
use crate::config::GuildConfig;
use crate::database::queries;
use crate::shared::types::{Context, Error};

/// Forcibly register a user, bypassing Hypixel Discord verification.
#[poise::command(
    slash_command,
    guild_only,
    check = "crate::utils::permissions::admin_check"
)]
pub async fn force_register(
    ctx: Context<'_>,
    #[description = "Discord user to register"] user: serenity::User,
    #[description = "Minecraft username"] minecraft_username: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;

    let guild_id_i64 = guild_id.get() as i64;
    let discord_user_id = user.id.get() as i64;

    queries::upsert_guild(&ctx.data().db, guild_id_i64).await?;

    let guild_row = queries::get_guild(&ctx.data().db, guild_id_i64).await?;
    let guild_config: GuildConfig = guild_row
        .as_ref()
        .map(|g| serde_json::from_value(g.config_json.clone()).unwrap_or_default())
        .unwrap_or_default();

    let profile = ctx
        .data()
        .hypixel
        .resolve_username(&minecraft_username)
        .await
        .map_err(|e| format!("Could not resolve Minecraft username: {e}"))?;

    let player_data = ctx
        .data()
        .hypixel
        .fetch_player(&profile.id)
        .await
        .map_err(|e| format!("Could not fetch Hypixel player data: {e}"))?;

    let role_id = match guild_config.registered_role_id {
        Some(id) => id,
        None => {
            let embed = CreateEmbed::default()
                .title("Registration Failed")
                .color(0xFF4444)
                .description("Registration is not configured on this server. An administrator must set a registered role first.");
            ctx.send(poise::CreateReply::default().embed(embed)).await?;
            return Ok(());
        }
    };

    if let Some(existing_user) =
        queries::get_user_by_discord_id(&ctx.data().db, discord_user_id, guild_id_i64).await?
    {
        let embed = CreateEmbed::default()
            .title("Already Registered")
            .color(0xFFAA00)
            .description(format!(
                "User is already registered as **{}** (UUID `{}`). If you want to change the linked Minecraft account, please unregister first with `/unregister`.",
                existing_user.minecraft_uuid, existing_user.minecraft_uuid
            ));
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        logger(
            ctx.serenity_context(),
            ctx.data(),
            guild_id,
            LogType::Warn,
            format!(
                "{} attempted to force register <@{}> but they are already registered as `{}`",
                ctx.author().name,
                user.id,
                existing_user.minecraft_uuid
            ),
        )
        .await?;
        return Ok(());
    }

    let role = serenity::RoleId::new(role_id);
    let member = guild_id
        .member(&ctx.serenity_context().http, user.id)
        .await?;

    if let Err(e) = member.add_role(&ctx.serenity_context().http, role).await {
        error!(
            guild_id = guild_id_i64,
            discord_user_id,
            role_id,
            error = %e,
            "Failed to assign registered role"
        );

        let embed = CreateEmbed::default()
            .title("Registration Failed")
            .color(0xFF4444)
            .description("I couldn't assign the registered role. Please ensure I have **Manage Roles** permission and my role is above the registered role.");
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        logger(
            ctx.serenity_context(),
            ctx.data(),
            guild_id,
            LogType::Error,
            format!(
                "Failed to assign registered role during force register for <@{}> by {}",
                user.id,
                ctx.author().name
            ),
        )
        .await?;
        logger(
            ctx.serenity_context(),
            ctx.data(),
            guild_id,
            LogType::Error,
            format!(
                "Failed to assign registered role during force register for <@{}> by {}",
                user.id,
                ctx.author().name
            ),
        )
        .await?;
        return Ok(());
    }

    let now = chrono::Utc::now();

    let db_user = queries::register_user(
        &ctx.data().db,
        discord_user_id,
        profile.id,
        &profile.name,
        guild_id_i64,
        now,
    )
    .await?;

    queries::update_user_hypixel_rank(
        &ctx.data().db,
        db_user.id,
        player_data.rank.as_db_str(),
        player_data.rank_plus_color.as_deref(),
    )
    .await?;

    // Insert stat snapshots as in normal registration
    let bw = &player_data.bedwars;
    for (stat_name, value) in &bw.stats {
        queries::insert_hypixel_snapshot(&ctx.data().db, db_user.id, stat_name, *value, now)
            .await?;
    }
    for stat_name in &["messages_sent", "reactions_added", "commands_used", "total_messages_raw"] {
        queries::insert_discord_snapshot(&ctx.data().db, db_user.id, stat_name, 0.0, now).await?;
    }

    let _ = fetch_and_cache_head_texture(&ctx.data().db, db_user.id, &profile.id).await;

    info!(
        discord_user_id,
        minecraft_uuid = %profile.id,
        minecraft_name = %profile.name,
        "User forcibly registered by admin"
    );

    logger(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        LogType::Warn,
        format!(
            "{} forcibly registered <@{}> as **{}** (`{}`)",
            ctx.author().name,
            user.id,
            profile.name,
            profile.id
        ),
    )
    .await?;

    let embed = CreateEmbed::default()
        .title("Force Registration Successful")
        .color(0x00BFFF)
        .description(format!(
            "User <@{}> has been forcibly registered as **{}** (UUID `{}`).",
            user.id, profile.name, profile.id
        ));
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}
#[poise::command(
    slash_command,
    guild_only,
    check = "crate::utils::permissions::admin_check"
)]
pub async fn force_unregister(
    ctx: Context<'_>,
    #[description = "Discord user to unregister"] user: serenity::User,
) -> Result<(), Error> {
    let user_id = user.id.get() as i64;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let guild_id_i64 = guild_id.get() as i64;

    let data = ctx.data();

    let guild_row = queries::get_guild(&data.db, guild_id_i64)
        .await?
        .ok_or("Guild configuration not found. Ask an admin to configure the bot.")?;

    let guild_config: GuildConfig =
        serde_json::from_value(guild_row.config_json.clone()).unwrap_or_default();

    queries::unregister_user(&data.db, user_id, guild_id_i64).await?;

    if let Some(role_id) = guild_config.registered_role_id {
        let role = serenity::RoleId::new(role_id);

        let role_exists = ctx
            .guild()
            .map(|g| g.roles.contains_key(&role))
            .unwrap_or(false);

        // check role exists in cached guild
        if !role_exists {
            debug!(
                role_id,
                guild_id = guild_id_i64,
                "Registered role not found in guild, skipping role removal"
            );
            let embed = CreateEmbed::default()
                .title("Unregistered")
                .color(0xFFAA00)
                .description(
                    "You have been unregistered, but I couldn't find the registered role in \
                     the server. Please ask an administrator to update the configuration and \
                     remove the role manually if desired.",
                );
            ctx.send(poise::CreateReply::default().embed(embed)).await?;
            return Ok(());
        }

        // remove role from the member
        let member = guild_id.member(ctx.http(), ctx.author().id).await?;

        if member.roles.contains(&role) {
            member.remove_role(ctx.http(), role).await?;
        }
    }

    let embed = CreateEmbed::default()
        .title("Unregistered")
        .color(0x00BFFF)
        .description(format!(
            "You have been successfully unregistered {}",
            user.id
        ));
    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    logger(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        LogType::Warn,
        format!("{} forcibly unregistered <@{}>", ctx.author().name, user.id),
    )
    .await?;

    Ok(())
}
