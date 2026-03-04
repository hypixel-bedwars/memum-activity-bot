/// `/register` command.
///
/// Links a Discord user to their Minecraft account by resolving the username
/// to a UUID via the Mojang API, storing the mapping in the database, and
/// assigning the guild's configured registered role.
use poise::serenity_prelude as serenity;
use time::OffsetDateTime;
use tracing::info;

use crate::config::GuildConfig;
use crate::database::queries;
use crate::shared::types::{Context, Error};

/// Register your Minecraft account to start tracking stats and earning points.
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

    // ------------------------------------------------------------------
    // 1. Ensure the guild exists in the database.
    // ------------------------------------------------------------------
    queries::upsert_guild(&data.db, guild_id_i64).await?;

    // ------------------------------------------------------------------
    // 2. Load guild config to find the registered role.
    // ------------------------------------------------------------------
    let guild_row = queries::get_guild(&data.db, guild_id_i64).await?;
    let guild_config: GuildConfig = guild_row
        .as_ref()
        .map(|g| serde_json::from_str(&g.config_json).unwrap_or_default())
        .unwrap_or_default();

    // ------------------------------------------------------------------
    // 3. Resolve the Minecraft username to a UUID.
    // ------------------------------------------------------------------
    let profile = data
        .hypixel
        .resolve_username(&minecraft_username)
        .await
        .map_err(|e| format!("Could not resolve Minecraft username: {e}"))?;

    // ------------------------------------------------------------------
    // 4. Store the user in the database.
    // ------------------------------------------------------------------
    let now = OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string());

    let db_user =
        queries::register_user(&data.db, discord_user_id, &profile.id, guild_id_i64, &now).await?;

    info!(
        discord_user_id,
        minecraft_uuid = %profile.id,
        minecraft_name = %profile.name,
        "User registered."
    );

    // ------------------------------------------------------------------
    // 5. Assign the registered role (if configured).
    // ------------------------------------------------------------------
    if let Some(role_id) = guild_config.registered_role_id {
        let role = serenity::RoleId::new(role_id);

        // Verify the role exists in the guild.
        let guild_roles = guild_id.roles(&ctx.http()).await?;
        if !guild_roles.contains_key(&role) {
            ctx.say(format!(
                "Registered as **{}** (UUID `{}`), but the configured role (ID {}) does not exist in this server. \
                 Please ask an admin to update the guild config.",
                profile.name, profile.id, role_id
            ))
            .await?;
            return Ok(());
        }

        // Add the role to the member.
        let member = guild_id.member(&ctx.http(), ctx.author().id).await?;
        member.add_role(&ctx.http(), role).await.map_err(|e| {
            format!(
                "Failed to assign role: {e}. Make sure the bot has the Manage Roles permission \
                 and its role is above the registered role."
            )
        })?;

        ctx.say(format!(
            "Successfully registered as **{}** (UUID `{}`) and assigned <@&{}>!",
            profile.name, profile.id, role_id
        ))
        .await?;
    } else {
        ctx.say(format!(
            "Successfully registered as **{}** (UUID `{}`)! No role assignment configured for this server.",
            profile.name, profile.id
        ))
        .await?;
    }

    // Ensure the user has an initial points row.
    queries::upsert_points(&data.db, db_user.id, 0.0, &now).await?;

    Ok(())
}
