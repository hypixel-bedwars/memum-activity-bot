/// `/register` command.
///
/// Links a Discord user to their Minecraft account by resolving the username
/// to a UUID via the Mojang API, storing the mapping in the database, and
/// assigning the guild's configured registered role.
use time::OffsetDateTime;
use tracing::{debug, info};

use crate::config::GuildConfig;
use crate::database::queries;
use crate::shared::types::{Context, Error};
use poise::serenity_prelude::RoleId;

/// Register your Minecraft account to start tracking stats and earning XP.
#[poise::command(slash_command, guild_only)]
pub async fn register(
    ctx: Context<'_>,
    #[description = "Your Minecraft username"] minecraft_username: String,
) -> Result<(), Error> {
    // Defer the reply so the user sees a "thinking..." indicator while we
    // make external API calls.
    ctx.defer().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let guild_id_i64 = guild_id.get() as i64;
    let discord_user_id = ctx.author().id.get() as i64;
    let data = ctx.data();

    queries::upsert_guild(&data.db, guild_id_i64).await?;

    let guild_row = queries::get_guild(&data.db, guild_id_i64).await?;
    let guild_config: GuildConfig = guild_row
        .as_ref()
        .map(|g| serde_json::from_str(&g.config_json).unwrap_or_default())
        .unwrap_or_default();

    let profile = data
        .hypixel
        .resolve_username(&minecraft_username)
        .await
        .map_err(|e| format!("Could not resolve Minecraft username: {e}"))?;

    let player_data = data
        .hypixel
        .fetch_player(&profile.id)
        .await
        .map_err(|e| format!("Could not fetch Hypixel player data: {e}"))?;

    let author_tag = ctx.author().tag();

    match player_data.social_links.get("DISCORD") {
        Some(linked) => {
            if linked != &author_tag {
                ctx.say(format!(
                    "Ownership verification failed.\n\n\
                     Hypixel account **{}** is linked to Discord `{}` but you are `{}`.\n\
                     Please update your Hypixel social link to match your Discord.",
                    profile.name, linked, author_tag
                ))
                .await?;
                return Ok(());
            }
        }
        None => {
            ctx.say(
                "Ownership verification failed.\n\n\
                 Your Hypixel account must have a **Discord social link** set.\n\
                 Please link your Discord in Hypixel:\n\
                 `/socials discord <your discord>`",
            )
            .await?;
            return Ok(());
        }
    }

    let role_id = match guild_config.registered_role_id {
        Some(id) => id,
        None => {
            ctx.say(
                "Registration is not configured on this server. \
                An administrator must set a registered role first.",
            )
            .await?;
            info!(
                "Guild {} attempted to register but has no registered role configured.",
                guild_id_i64
            );
            return Ok(());
        }
    };

    // Check if the user is already registered in this guild. If they are
    // already registered then send a message that will
    // tell the user that they are already registered
    if let Some(existing_user) =
        queries::get_user_by_discord_id(&data.db, discord_user_id, guild_id_i64).await?
    {
        ctx.say(format!(
			"You are already registered as **{}** (UUID `{}`). If you want to change your linked Minecraft account, please unregister first with `/unregister`.",
			existing_user.minecraft_uuid, existing_user.minecraft_uuid
		)).await?;
        debug!(
            "User {} attempted to register but is already registered as {} in guild {}.",
            discord_user_id, existing_user.minecraft_uuid, guild_id_i64
        );
        return Ok(());
    }

    // Assign the registered role to the user.
    let role = RoleId::new(role_id);
    let member = guild_id.member(ctx.http(), ctx.author().id).await?;

    if let Err(e) = member.add_role(ctx.http(), role).await {
        ctx.say(
            "I couldn't assign the registered role. \
            Please ensure I have **Manage Roles** permission and my role is above the registered role."
        )
        .await?;

        info!(
            "Failed to assign registered role to user {} in guild {}: {}",
            discord_user_id, guild_id_i64, e
        );

        return Ok(());
    }

    let now = OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());

    let db_user = queries::register_user(&data.db, discord_user_id, &profile.id, guild_id_i64, &now).await?;

    // fetch the base stats for the user
    // and store the initial values in the database so we have a baseline for future comparisons.
    
    let bw = &player_data.bedwars;

    for (stat_name, value) in &bw.stats {
        queries::insert_hypixel_snapshot(
            &data.db,
            db_user.id,
            stat_name,
            *value,
            &now,
        )
        .await?;
    }

    ctx.say(format!(
		 "You have been registered as **{}** (UUID `{}`). You can now start earning XP and tracking your stats!",
		 profile.name, profile.id
	 )).await?;

    info!(
        discord_user_id,
        minecraft_uuid = %profile.id,
        minecraft_name = %profile.name,
        "User registered."
    );

    // Ensure the user has an initial XP row (start at 0 XP, level 1).
    queries::upsert_xp(&data.db, db_user.id, 0.0, &now).await?;

    Ok(())
}
