/// `/register` command.
///
/// Links a Discord user to their Minecraft account by resolving the username
/// to a UUID via the Mojang API, storing the mapping in the database, and
/// assigning the guild's configured registered role.
use time::OffsetDateTime;
use tracing::{debug, error, info};

use poise::serenity_prelude::{self as serenity, CreateEmbed};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::GuildConfig;
use crate::database::queries;
use crate::shared::types::{Context, Data, Error};
use base64::{Engine as _, engine::general_purpose};

/// Core registration logic, shared between `/register` and the Register button.
///
/// Resolves `minecraft_username` via Mojang, verifies ownership via Hypixel
/// social links, assigns the guild's registered role, inserts the user record,
/// and seeds baseline stat snapshots.
///
/// Returns a user-facing message string describing either success or the reason
/// for failure. The caller is responsible for sending that message as a reply.
pub async fn perform_registration(
    serenity_ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
    user_tag: &str,
    minecraft_username: &str,
) -> Result<(String, Option<(i64, Uuid)>), Error> {
    let guild_id_i64 = guild_id.get() as i64;
    let discord_user_id = user_id.get() as i64;

    debug!(
        guild_id = guild_id_i64,
        discord_user_id,
        user_tag = %user_tag,
        minecraft_username = %minecraft_username,
        "Starting registration"
    );

    queries::upsert_guild(&data.db, guild_id_i64).await?;

    let guild_row = queries::get_guild(&data.db, guild_id_i64).await?;
    let guild_config: GuildConfig = guild_row
        .as_ref()
        .map(|g| serde_json::from_value(g.config_json.clone()).unwrap_or_default())
        .unwrap_or_default();

    debug!(
        guild_id = guild_id_i64,
        config = ?guild_config,
        "Loaded guild configuration"
    );

    debug!(minecraft_username = %minecraft_username, "Resolving Minecraft username");

    let profile = data
        .hypixel
        .resolve_username(minecraft_username)
        .await
        .map_err(|e| format!("Could not resolve Minecraft username: {e}"))?;

    debug!(
        minecraft_name = %profile.name,
        minecraft_uuid = %profile.id,
        "Minecraft username resolved"
    );

    debug!(minecraft_uuid = %profile.id, "Fetching Hypixel player data");

    let player_data = data
        .hypixel
        .fetch_player(&profile.id)
        .await
        .map_err(|e| format!("Could not fetch Hypixel player data: {e}"))?;

    debug!(minecraft_uuid = %profile.id, "Hypixel player data fetched");

    match player_data.social_links.get("DISCORD") {
        Some(linked) => {
            debug!(
                minecraft_name = %profile.name,
                linked_discord = %linked,
                expected_discord = %user_tag,
                "Found Hypixel Discord social link"
            );

            if linked != user_tag {
                debug!(
                    minecraft_name = %profile.name,
                    linked_discord = %linked,
                    actual_discord = %user_tag,
                    "Ownership verification failed"
                );

                return Ok((
                    format!(
                        "Ownership verification failed.\n\n\
                    Hypixel account **{}** is linked to Discord `{}` but you are `{}`.\n\
                    Please update your Hypixel social link to match your Discord.",
                        profile.name, linked, user_tag
                    ),
                    None,
                ));
            }

            debug!(
                minecraft_name = %profile.name,
                discord = %linked,
                "Ownership verification succeeded"
            );
        }
        None => {
            debug!(
                minecraft_name = %profile.name,
                "Ownership verification failed: no Discord social link"
            );

            return Ok((
                "Ownership verification failed.\n\n\
                 Your Hypixel account must have a **Discord social link** set.\n\
                 Please link your Discord in Hypixel:\n\
                 `/socials discord <your discord>`"
                    .to_string(),
                None,
            ));
        }
    }

    let role_id = match guild_config.registered_role_id {
        Some(id) => id,
        None => {
            debug!(
                guild_id = guild_id_i64,
                "Registration attempted but no registered role configured"
            );

            return Ok((
                "Registration is not configured on this server. \
                An administrator must set a registered role first."
                    .to_string(),
                None,
            ));
        }
    };

    if let Some(existing_user) =
        queries::get_user_by_discord_id(&data.db, discord_user_id, guild_id_i64).await?
    {
        debug!(
            guild_id = guild_id_i64,
            discord_user_id,
            minecraft_uuid = %existing_user.minecraft_uuid,
            "User attempted duplicate registration"
        );

        return Ok((
            format!(
                "You are already registered as **{}** (UUID `{}`). \
                If you want to change your linked Minecraft account, please unregister first with `/unregister`.",
                existing_user.minecraft_uuid, existing_user.minecraft_uuid
            ),
            None,
        ));
    }

    let role = serenity::RoleId::new(role_id);
    let member = guild_id.member(&serenity_ctx.http, user_id).await?;

    if let Err(e) = member.add_role(&serenity_ctx.http, role).await {
        error!(
            guild_id = guild_id_i64,
            discord_user_id,
            role_id,
            error = %e,
            "Failed to assign registered role"
        );

        return Ok((
            "I couldn't assign the registered role. \
            Please ensure I have **Manage Roles** permission and my role is above the registered role."
                .to_string(),
            None,
        ));
    }

    debug!(
        guild_id = guild_id_i64,
        discord_user_id,
        role_id,
        "Registered role assigned"
    );

    let now = chrono::Utc::now();

    let db_user = queries::register_user(
        &data.db,
        discord_user_id,
        profile.id,
        &profile.name,
        guild_id_i64,
        now,
    )
    .await?;

    debug!(
        db_user_id = db_user.id,
        minecraft_uuid = %profile.id,
        "User inserted into database"
    );

    let bw = &player_data.bedwars;
    let time_now = chrono::Utc::now();
    for (stat_name, value) in &bw.stats {
        queries::insert_hypixel_snapshot(&data.db, db_user.id, stat_name, *value, time_now).await?;
    }

    debug!(
        db_user_id = db_user.id,
        stat_count = bw.stats.len(),
        "Inserted Hypixel stat snapshots"
    );

    for stat_name in &["messages_sent", "reactions_added", "commands_used"] {
        queries::insert_discord_snapshot(&data.db, db_user.id, stat_name, 0.0, now).await?;
    }

    debug!(
        db_user_id = db_user.id,
        "Inserted initial Discord stat snapshots"
    );

    // XP row is created on first sweep via INSERT ... ON CONFLICT DO UPDATE
    // inside apply_stat_deltas. No seed row needed at registration time.

    info!(
        discord_user_id,
        minecraft_uuid = %profile.id,
        minecraft_name = %profile.name,
        "User registered"
    );

    Ok((
        format!(
            "You have been registered as **{}** (UUID `{}`). \
            You can now start earning XP and tracking your stats!",
            profile.name, profile.id
        ),
        Some((db_user.id, profile.id)),
    ))
}

pub async fn fetch_and_cache_head_texture(
    pool: &PgPool,
    user_id: i64,
    uuid: &Uuid,
) -> Option<String> {
    // Construct the URL you want to fetch. Minotar is convenient:
    // let url = format!("https://minotar.net/helm/{}/64.png", uuid);
    // If you have a different API for textures, use that URL.

    let url = format!("https://minotar.net/avatar/{}/128", uuid);

    let resp = match reqwest::get(&url).await {
        Ok(r) => r,
        Err(_) => return None,
    };
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => return None,
    };
    // Convert to base64 and build a data URL
    let b64 = general_purpose::STANDARD.encode(&bytes);
    let data_url = format!("data:image/png;base64,{}", b64);

    // store in DB
    let updated_at = OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .ok();
    if let Some(ts) = updated_at {
        let _ = queries::set_user_head_texture(pool, user_id, &data_url, &ts).await;
    }

    Some(data_url)
}

/// Register your Minecraft account to start tracking stats and earning XP.
#[poise::command(slash_command, guild_only)]
pub async fn register(
    ctx: Context<'_>,
    #[description = "Your Minecraft username"] minecraft_username: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?;

    let (msg, user_data) = perform_registration(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        ctx.author().id,
        &ctx.author().tag(),
        &minecraft_username,
    )
    .await?;

    if let Some((db_user_id, uuid)) = user_data {
        let _ = fetch_and_cache_head_texture(&ctx.data().db, db_user_id, &uuid).await;
    }

    // Detect success by looking for the phrase we set in the success branch.
    let success = msg.contains("You have been registered");
    let embed = CreateEmbed::default()
        .title(if success {
            "Registration Successful"
        } else {
            "Registration Failed"
        })
        .color(if success { 0x00BFFF } else { 0xFF4444 })
        .description(msg);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}
