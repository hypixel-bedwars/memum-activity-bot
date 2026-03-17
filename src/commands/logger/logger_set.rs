use crate::database::queries;
use crate::shared::types::{Context, Error};
use poise::serenity_prelude::{self as serenity};

/// Manage stat XP configuration for this server. Admin only.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    rename = "logger-set",
    check = "crate::utils::permissions::admin_check"
)]
pub async fn logger_set(
    ctx: Context<'_>,
    #[description = "Select a channel to send logging"] channel: serenity::Channel,
) -> Result<(), Error> {
    // Ensure this is being invoked in a guild
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let guild_id_i64 = guild_id.get() as i64;

    // Ensure there's a guild row present
    let data = ctx.data();
    queries::upsert_guild(&data.db, guild_id_i64).await?;

    // Persist the configured logging channel
    let channel_id = channel.id().get() as i64;
    queries::set_guild_log_channel(&data.db, guild_id_i64, Some(channel_id)).await?;

    ctx.send(
        poise::CreateReply::default()
            .content(format!("✅ Logger channel set to <#{}>", channel_id)),
    )
    .await?;

    Ok(())
}
