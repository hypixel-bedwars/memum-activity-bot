/// `/stats` command.
///
/// Displays a user's current Bedwars stats and accumulated XP.
use poise::serenity_prelude::{self as serenity, CreateEmbed, CreateEmbedFooter};

use crate::database::queries;
use crate::xp::calculator;
use crate::shared::types::{Context, Error};

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

    let player_data = match data.hypixel.fetch_player(&db_user.minecraft_uuid).await {
        Ok(p) => p,
        Err(e) => {
            let xp = queries::get_xp(&data.db, db_user.id).await?.map(|x| x.total_xp).unwrap_or(0.0);
            let embed = CreateEmbed::default()
                .title(format!("Stats — {}", target.name))
                .color(0xFF4444)
                .description(format!(
                    "Could not fetch Hypixel stats: {e}\n\n**XP:** {:.0}",
                    xp
                ))
                .footer(CreateEmbedFooter::new(format!(
                    "UUID: {}",
                    db_user.minecraft_uuid
                )));

            ctx.send(poise::CreateReply::default().embed(embed)).await?;
            return Ok(());
        }
    };

    let stats = &player_data.bedwars;

    let xp_row = queries::get_xp(&data.db, db_user.id).await?;
    let total_xp = xp_row.as_ref().map(|x| x.total_xp).unwrap_or(0.0);
    // Level calculation from XP using AppConfig values
    let base_xp = data.config.base_level_xp;
    let exponent = data.config.level_exponent;
    let current_level = calculator::calculate_level(total_xp, base_xp, exponent);

    let embed = CreateEmbed::default()
        .title(format!("Bedwars Stats — {}", target.name))
        .color(0x00BFFF)
        .field("Wins", stats.wins().to_string(), true)
        .field("Kills", stats.kills().to_string(), true)
        .field("Beds Broken", stats.beds_broken().to_string(), true)
        .field("XP", format!("{:.0}", total_xp), true)
        .field("Level", current_level.to_string(), true)
        .footer(CreateEmbedFooter::new(format!(
            "UUID: {}",
            db_user.minecraft_uuid
        )));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
