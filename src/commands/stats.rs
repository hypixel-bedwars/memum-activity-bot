/// `/stats` command.
///
/// Displays a user's current Bedwars stats and accumulated points in a Discord
/// embed. If no user is specified, shows the invoking user's stats.
use poise::serenity_prelude::{self as serenity, CreateEmbed, CreateEmbedFooter};

use crate::database::queries;
use crate::shared::types::{Context, Error};

/// View Bedwars stats and points for yourself or another user.
#[poise::command(slash_command, guild_only)]
pub async fn stats(
    ctx: Context<'_>,
    #[description = "User to look up (defaults to you)"] user: Option<serenity::User>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let target = user.as_ref().unwrap_or_else(|| ctx.author());
    let data = ctx.data();

    // ------------------------------------------------------------------
    // 1. Look up the user in the database.
    // ------------------------------------------------------------------
    let db_user =
        queries::get_user_by_discord_id(&data.db, target.id.get() as i64, guild_id.get() as i64)
            .await?;

    let db_user = match db_user {
        Some(u) => u,
        None => {
            ctx.say(format!(
                "{} is not registered. Use `/register` to link a Minecraft account.",
                target.name
            ))
            .await?;
            return Ok(());
        }
    };

    // ------------------------------------------------------------------
    // 2. Fetch live stats from Hypixel (cached if recent).
    // ------------------------------------------------------------------
    let bw_stats = data
        .hypixel
        .fetch_bedwars_stats(&db_user.minecraft_uuid)
        .await;

    // ------------------------------------------------------------------
    // 3. Load points from the database.
    // ------------------------------------------------------------------
    let points = queries::get_points(&data.db, db_user.id).await?;
    let total_points = points.map(|p| p.total_points).unwrap_or(0.0);

    // ------------------------------------------------------------------
    // 4. Build and send the embed.
    // ------------------------------------------------------------------
    let embed = match bw_stats {
        Ok(stats) => CreateEmbed::default()
            .title(format!("Bedwars Stats — {}", target.name))
            .color(0x00BFFF)
            .field("Wins", format!("{}", stats.wins() as u64), true)
            .field("Kills", format!("{}", stats.kills() as u64), true)
            .field(
                "Beds Broken",
                format!("{}", stats.beds_broken() as u64),
                true,
            )
            .field("Points", format!("{:.0}", total_points), false)
            .footer(CreateEmbedFooter::new(format!(
                "UUID: {}",
                db_user.minecraft_uuid
            ))),
        Err(e) => CreateEmbed::default()
            .title(format!("Stats — {}", target.name))
            .color(0xFF4444)
            .description(format!(
                "Could not fetch Hypixel stats: {e}\n\n**Points:** {:.0}",
                total_points
            ))
            .footer(CreateEmbedFooter::new(format!(
                "UUID: {}",
                db_user.minecraft_uuid
            ))),
    };

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}
