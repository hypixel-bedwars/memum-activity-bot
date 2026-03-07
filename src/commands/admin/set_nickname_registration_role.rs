/// `/set-nickname-registration-role` and `/clear-nickname-registration-role` commands — admin only.
///
/// Configure which guild role enables nickname-based auto-registration via the
/// Register button. When a role is set, members who possess it may press the
/// Register button and have their Minecraft username extracted automatically
/// from their nickname (format: `[NNN emoji] MinecraftUsername`). Members
/// without the role, or any user when no role is configured, must use
/// `/register <minecraft_username>` instead.
use poise::serenity_prelude::{self as serenity, CreateEmbed};
use tracing::{debug, info};

use crate::config::GuildConfig;
use crate::database::queries;
use crate::shared::types::{Context, Error};

/// Set the role that allows members to register via nickname parsing. Admin only.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "set-nickname-registration-role"
)]
pub async fn set_nickname_registration_role(
    ctx: Context<'_>,
    #[description = "Verified role."]
    role: serenity::Role,
) -> Result<(), Error> {
    if !ctx
        .data()
        .config
        .admin_user_ids
        .contains(&ctx.author().id.get())
    {
        let embed = CreateEmbed::default()
            .title("Permission Denied")
            .color(0xFF4444)
            .description("You do not have permission to use this command.");
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    debug!(
        "Invoked /set-nickname-registration-role with role {} (ID {})",
        role.name, role.id
    );

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let guild_id_i64 = guild_id.get() as i64;
    let data = ctx.data();

    queries::upsert_guild(&data.db, guild_id_i64).await?;

    let guild_row = queries::get_guild(&data.db, guild_id_i64).await?;
    let mut guild_config: GuildConfig = guild_row
        .as_ref()
        .and_then(|g| serde_json::from_value(g.config_json.clone()).ok())
        .unwrap_or_default();

    guild_config.nickname_registration_role_id = Some(role.id.get());

    debug!(
        "Setting nickname registration role for guild {} to {} (ID {})",
        guild_id, role.name, role.id
    );

    let config_json = serde_json::to_value(&guild_config)?;
    queries::update_guild_config(&data.db, guild_id_i64, config_json).await?;

    let embed = CreateEmbed::default()
        .title("Nickname Registration Role Set")
        .color(0x00BFFF)
        .description(format!(
            "Nickname registration role set to **{}**.\n\n\
            Members with this role may now press the **Register** button to register \
            automatically via their nickname.\n\n\
            Nickname format: `[NNN emoji] MinecraftUsername`\n\
            Example: `[313 💫] VA80`",
            role.name
        ));
    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    info!(
        "Updated nickname registration role for guild {} to {} (ID {})",
        guild_id, role.name, role.id
    );

    Ok(())
}

/// Clear the nickname registration role, requiring all users to use /register. Admin only.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "clear-nickname-registration-role"
)]
pub async fn clear_nickname_registration_role(ctx: Context<'_>) -> Result<(), Error> {
    if !ctx
        .data()
        .config
        .admin_user_ids
        .contains(&ctx.author().id.get())
    {
        let embed = CreateEmbed::default()
            .title("Permission Denied")
            .color(0xFF4444)
            .description("You do not have permission to use this command.");
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    debug!("Invoked /clear-nickname-registration-role");

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let guild_id_i64 = guild_id.get() as i64;
    let data = ctx.data();

    queries::upsert_guild(&data.db, guild_id_i64).await?;

    let guild_row = queries::get_guild(&data.db, guild_id_i64).await?;
    let mut guild_config: GuildConfig = guild_row
        .as_ref()
        .and_then(|g| serde_json::from_value(g.config_json.clone()).ok())
        .unwrap_or_default();

    guild_config.nickname_registration_role_id = None;

    let config_json = serde_json::to_value(&guild_config)?;
    queries::update_guild_config(&data.db, guild_id_i64, config_json).await?;

    let embed = CreateEmbed::default()
        .title("Nickname Registration Role Cleared")
        .color(0x00BFFF)
        .description(
            "Nickname registration has been disabled.\n\n\
            All users must now use `/register <minecraft_username>` to register.",
        );
    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    info!("Cleared nickname registration role for guild {}", guild_id);

    Ok(())
}
