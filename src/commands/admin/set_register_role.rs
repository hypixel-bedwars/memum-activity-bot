/// `/set-register-role` command — admin only.
///
/// Sets the role that will be assigned to users when they register.
use poise::serenity_prelude::{self as serenity, CreateEmbed};
use tracing::{debug, info};

use crate::config::GuildConfig;
use crate::database::queries;
use crate::shared::types::{Context, Error};

/// Set the role assigned to users on registration. Admin only.
#[poise::command(slash_command, guild_only, ephemeral, rename = "set-register-role")]
pub async fn set_register_role(
    ctx: Context<'_>,
    #[description = "The role to assign to users when they register"] role: serenity::Role,
) -> Result<(), Error> {
    // Inline admin check — replies ephemerally and exits if not authorised.
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
        "Invoked /set-register-role with role {} (ID {})",
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
        .map(|g| serde_json::from_str(&g.config_json).unwrap_or_default())
        .unwrap_or_default();

    guild_config.registered_role_id = Some(role.id.get());
    info!(
        "Setting registration role for guild {} to {} (ID {})",
        guild_id, role.name, role.id
    );
    let config_json = serde_json::to_string(&guild_config)?;
    queries::update_guild_config(&data.db, guild_id_i64, &config_json).await?;

    let embed = CreateEmbed::default()
        .title("Registration Role Updated")
        .color(0x00BFFF)
        .description(format!(
            "Registration role set to **{}**. New users will be assigned this role when they register.",
            role.name
        ));
    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    debug!("Finished handling /set-register-role");

    Ok(())
}
