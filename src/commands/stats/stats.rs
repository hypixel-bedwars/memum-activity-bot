/// `/stats` command.
///
/// Shows each configured stat as its **change since registration**
/// and also displays the **XP reward configured for each stat**.
use poise::serenity_prelude::{self as serenity, CreateEmbed, CreateEmbedFooter};
use tracing::info;

use crate::config::GuildConfig;
use crate::database::queries;
use crate::shared::types::{Context, Error};
use crate::utils::stats_definitions::{display_name_for_key, is_discord_stat};
use crate::sweeper;

#[poise::command(slash_command, guild_only)]
pub async fn stats(
    ctx: Context<'_>,
    #[description = "User to look up (defaults to you)"] user: Option<serenity::User>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;
    let guild_id_i64 = guild_id.get() as i64;

    let target = user.as_ref().unwrap_or_else(|| ctx.author());
    let data = ctx.data();

    // resolve registered user
    let db_user =
        queries::get_user_by_discord_id(&data.db, target.id.get() as i64, guild_id_i64).await?;

    let db_user = match db_user {
        Some(u) => u,
        None => {
            let embed = CreateEmbed::default()
                .title("Not Registered")
                .color(0xFF4444)
                .description(format!(
                    "**{}** is not registered. Use `/register` to link a Minecraft account.",
                    target.name
                ));
            ctx.send(poise::CreateReply::default().embed(embed)).await?;
            return Ok(());
        }
    };

    // on-demand Hypixel refresh
    sweeper::hypixel_sweeper::refresh_hypixel_user(
        &data.db,
        &data.hypixel,
        &db_user,
        &data.config,
    )
    .await;

    // load guild config
    let guild_row = queries::get_guild(&data.db, guild_id_i64).await?;
    let guild_config: GuildConfig = guild_row
        .as_ref()
        .map(|g| serde_json::from_value(g.config_json.clone()).unwrap_or_default())
        .unwrap_or_default();

    let active_keys: Vec<String> = {
        let mut keys: Vec<String> = guild_config.xp_config.keys().cloned().collect();
        keys.sort();
        keys
    };

    if active_keys.is_empty() {
        let embed = CreateEmbed::default()
            .title(format!("Stats — {}", target.name))
            .color(0xFF4444)
            .description(
                "No stats are currently configured. \
                 An admin can add stats with `/edit-stats add`.",
            );

        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    // resolve minecraft name
    let mc_name = match &db_user.minecraft_username {
        Some(name) => name.clone(),
        None => match data.hypixel.resolve_uuid(&db_user.minecraft_uuid).await {
            Ok(name) => name,
            Err(_) => db_user.minecraft_uuid.to_string(),
        },
    };

    let thumbnail_url = if let Some(tex) = &db_user.head_texture {
        tex.clone()
    } else {
        format!("https://minotar.net/avatar/{}/80", db_user.minecraft_uuid)
    };

    let mut embed = CreateEmbed::default()
        .title(format!("📊 Stats — {}", mc_name))
        .description(format!(
            "Statistics gained since **/register** for **{}**",
            mc_name
        ))
        .color(0x00BFFF)
        .thumbnail(thumbnail_url)
        .author(
            serenity::CreateEmbedAuthor::new(&target.name)
                .icon_url(target.avatar_url().unwrap_or_default()),
        )
        .footer(CreateEmbedFooter::new(format!(
            "UUID: {} • {} tracked stats",
            db_user.minecraft_uuid,
            active_keys.len()
        )));

    // --------------------------
    // Stat delta fields
    // --------------------------

    for key in &active_keys {
        let (latest_val, initial_val) = if is_discord_stat(key) {
            let latest = queries::get_latest_discord_snapshot(&data.db, db_user.id, key)
                .await?
                .map(|s| s.stat_value)
                .unwrap_or(0.0);

            let initial = queries::get_first_discord_snapshot(&data.db, db_user.id, key)
                .await?
                .map(|s| s.stat_value)
                .unwrap_or(0.0);

            (latest, initial)
        } else {
            let latest = queries::get_latest_hypixel_snapshot(&data.db, db_user.id, key)
                .await?
                .map(|s| s.stat_value)
                .unwrap_or(0.0);

            let initial = queries::get_first_hypixel_snapshot(&data.db, db_user.id, key)
                .await?
                .map(|s| s.stat_value)
                .unwrap_or(0.0);

            (latest, initial)
        };

        let delta = (latest_val - initial_val).max(0.0);

        embed = embed.field(
            display_name_for_key(key),
            format!("+{:.0}", delta),
            true,
        );
    }

    // --------------------------
    // XP reward section
    // --------------------------

    let mut rows: Vec<(String, String, f64)> = guild_config
        .xp_config
        .iter()
        .map(|(k, v)| (display_name_for_key(k), k.clone(), *v))
        .collect();

    rows.sort_by(|a, b| a.0.cmp(&b.0));

    let xp_description = rows
        .iter()
        .map(|(display, key, xp)| {
            format!("**{}** — {:.0} XP (`{}`)", display, xp, key)
        })
        .collect::<Vec<_>>()
        .join("\n");

    embed = embed.field(
        "XP Rewards",
        xp_description,
        false,
    );

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    info!(
        "Displayed stats for user {} ({})",
        target.name,
        target.id
    );

    Ok(())
}