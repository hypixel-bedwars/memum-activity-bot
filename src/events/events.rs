// The only reason i added back the use of nicknames is because
// It is a better way for a user to register if they are already verified in the server
// So i do beleive having the nickanme registration is a good UX improvment

use poise::serenity_prelude as serenity;
use tracing::{debug, warn};

use crate::commands::events::events as event_cmd;
use crate::commands::leaderboard::helpers as lb_helpers;
use crate::commands::leaderboard::leaderboard as lb;
use crate::commands::registration::register::perform_registration;
use crate::config::GuildConfig;
use crate::database::queries;
use crate::shared::types::{Data, Error};

pub async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    data: &Data,
) -> Result<(), Error> {
    if let serenity::FullEvent::InteractionCreate { interaction } = event {
        if let serenity::Interaction::Component(component) = interaction {
            if component.data.custom_id == "register_button" {
                handle_register_button(ctx, component, data).await?;
            } else if component.data.custom_id.starts_with("lb_page_") {
                if let Err(e) = lb::handle_pagination(ctx, component, data).await {
                    tracing::error!(error = %e, "Leaderboard pagination handler failed");
                }
            } else if component.data.custom_id.starts_with("event_lb_") {
                if let Err(e) = handle_event_lb_pagination(ctx, component, data).await {
                    tracing::error!(error = %e, "Event leaderboard pagination handler failed");
                }
            }
        }
    }

    Ok(())
}

/// Handle pagination button clicks for event leaderboards.
///
/// Custom ID format: `event_lb_{event_id}_page_{page}`
async fn handle_event_lb_pagination(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    data: &Data,
) -> Result<(), Error> {
    // Parse the custom ID: "event_lb_{event_id}_page_{page}"
    let custom_id = &component.data.custom_id;
    // Strip "event_lb_" prefix
    let rest = match custom_id.strip_prefix("event_lb_") {
        Some(r) => r,
        None => return Ok(()),
    };
    // rest = "{event_id}_page_{page}"
    let page_marker = "_page_";
    let page_pos = match rest.find(page_marker) {
        Some(p) => p,
        None => {
            warn!(custom_id, "Event LB pagination: malformed custom_id");
            return Ok(());
        }
    };

    let event_id_str = &rest[..page_pos];
    let page_str = &rest[page_pos + page_marker.len()..];

    let event_id: i64 = match event_id_str.parse() {
        Ok(v) => v,
        Err(_) => {
            warn!(custom_id, "Event LB pagination: failed to parse event_id");
            return Ok(());
        }
    };
    let page: u32 = match page_str.parse() {
        Ok(v) => v,
        Err(_) => {
            warn!(custom_id, "Event LB pagination: failed to parse page");
            return Ok(());
        }
    };

    // Defer the update so Discord doesn't show a loading spinner.
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Defer(
                serenity::CreateInteractionResponseMessage::new(),
            ),
        )
        .await?;

    // Load event details.
    let event = match queries::get_event_by_id(&data.db, event_id).await? {
        Some(e) => e,
        None => {
            warn!(event_id, "Event LB pagination: event not found");
            return Ok(());
        }
    };

    let (png_bytes, total_pages) = lb_helpers::generate_event_leaderboard_page(
        &data.db,
        event.id,
        &event.name,
        &event.status,
        event.start_date.timestamp(),
        page,
    )
    .await?;

    let attachment = serenity::CreateAttachment::bytes(png_bytes, "event_leaderboard.png");
    let components = event_cmd::event_lb_pagination_buttons(event_id, page, total_pages);

    component
        .edit_response(
            ctx,
            serenity::EditInteractionResponse::new()
                .new_attachment(attachment)
                .components(components),
        )
        .await?;

    Ok(())
}

/// Extract a Minecraft username from a guild nickname.
///
/// Nicknames must follow the format `[NNN emoji] MinecraftUsername`, for
/// example `[313 💫] VA80` or `[204 ✨] CosmicFuji`. The function returns the
/// substring after the first `"] "` sequence, trimmed of whitespace. Returns
/// `None` if the format is not matched or the extracted value is empty.
fn extract_username_from_nickname(nickname: &str) -> Option<&str> {
    let bracket_end = nickname.find("] ")?;
    let username = nickname[bracket_end + 2..].trim();
    if username.is_empty() {
        None
    } else {
        Some(username)
    }
}

/// Represents which source provides the Minecraft username for registration.
enum RegistrationPath {
    /// Username was already stored in the database for this user.
    WithUsername(String),
    /// Nickname auto-registration is enabled; contains the required role ID.
    FromNickname(u64),
}

async fn handle_register_button(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    data: &Data,
) -> Result<(), Error> {
    let guild_id = match component.guild_id {
        Some(id) => id,
        None => {
            respond_ephemeral(
                ctx,
                component,
                "This button can only be used inside a server.",
            )
            .await?;
            return Ok(());
        }
    };

    let discord_id = component.user.id.get() as i64;
    let guild_id_i64 = guild_id.get() as i64;

    let db_user = match queries::get_user_by_discord_id(&data.db, discord_id, guild_id_i64).await {
        Ok(u) => u,
        Err(e) => {
            warn!(error = %e, "Failed to query DB for user during button press");
            respond_ephemeral(
                ctx,
                component,
                "A database error occurred. Please try again.",
            )
            .await?;
            return Ok(());
        }
    };

    let path = if let Some(user) = db_user {
        // User is already in the database with a stored username.
        if let Some(username) = user.minecraft_username {
            RegistrationPath::WithUsername(username)
        } else {
            // Edge case: row exists but username column is null (pre-migration row).
            // Fall through to nickname registration.
            debug!(
                discord_id,
                "User row found but minecraft_username is null; attempting nickname path"
            );
            match resolve_nickname_path(ctx, component, data, guild_id_i64).await? {
                Some(p) => p,
                None => return Ok(()), // already responded ephemerally
            }
        }
    } else {
        // User is not registered at all.
        match resolve_nickname_path(ctx, component, data, guild_id_i64).await? {
            Some(p) => p,
            None => return Ok(()), // already responded ephemerally
        }
    };

    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Defer(
                serenity::CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await?;

    let minecraft_username: String = match path {
        RegistrationPath::WithUsername(u) => u,

        RegistrationPath::FromNickname(role_id) => {
            // Fetch the full guild member object (HTTP, so done after defer).
            let member = match guild_id.member(&ctx.http, component.user.id).await {
                Ok(m) => m,
                Err(e) => {
                    warn!(error = %e, "Failed to fetch guild member for nickname registration");
                    send_followup(
                        ctx,
                        component,
                        "Could not retrieve your server profile. Please try again.",
                    )
                    .await?;
                    return Ok(());
                }
            };

            // Check role membership.
            let required_role = serenity::RoleId::new(role_id);
            if !member.roles.contains(&required_role) {
                debug!(
                    discord_id,
                    role_id, "User pressed Register button but lacks nickname-registration role"
                );
                send_followup(
                    ctx,
                    component,
                    "You don't have the required role to register automatically.\n\n\
                    Please run `/register <minecraft_username>` to register manually.",
                )
                .await?;
                return Ok(());
            }

            // Read nickname.
            let nick = match member.nick.as_deref() {
                Some(n) => n,
                None => {
                    send_followup(
                        ctx,
                        component,
                        "You don't have a nickname set on this server.\n\n\
                        Your nickname must follow the format: `[NNN emoji] MinecraftUsername`\n\
                        Example: `[313 💫] VA80`",
                    )
                    .await?;
                    return Ok(());
                }
            };

            // Parse Minecraft username out of nickname.
            match extract_username_from_nickname(nick) {
                Some(u) => u.to_string(),
                None => {
                    send_followup(
                        ctx,
                        component,
                        "Your nickname doesn't match the required format.\n\n\
                        Expected: `[NNN emoji] MinecraftUsername`\n\
                        Example: `[313 💫] VA80`\n\n\
                        Please update your nickname or run `/register <minecraft_username>` manually.",
                    )
                    .await?;
                    return Ok(());
                }
            }
        }
    };

    let result = perform_registration(
        ctx,
        data,
        guild_id,
        component.user.id,
        &component.user.tag(),
        &minecraft_username,
    )
    .await;

    let reply_text = match result {
        Ok((text, Some((db_user_id, uuid)))) => {
            let _ = crate::commands::registration::register::fetch_and_cache_head_texture(
                &data.db, db_user_id, &uuid,
            )
            .await;
            text
        }
        Ok((text, None)) => text,
        Err(e) => {
            warn!(
                user = component.user.id.get(),
                error = %e,
                "perform_registration returned an unexpected error"
            );
            format!("An unexpected error occurred during registration: {e}")
        }
    };

    send_followup(ctx, component, &reply_text).await?;

    Ok(())
}

/// Load guild config and return the appropriate `RegistrationPath` for a user
/// who is not (or not fully) registered in the database.
///
/// Returns `None` if an ephemeral error response has already been sent and the
/// caller should return immediately. Returns `Some(path)` otherwise.
async fn resolve_nickname_path(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    data: &Data,
    guild_id_i64: i64,
) -> Result<Option<RegistrationPath>, Error> {
    let guild_row = match queries::get_guild(&data.db, guild_id_i64).await {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "Failed to load guild config during button press");
            respond_ephemeral(
                ctx,
                component,
                "A database error occurred. Please try again.",
            )
            .await?;
            return Ok(None);
        }
    };

    let guild_config: GuildConfig = guild_row
        .as_ref()
        .and_then(|g| serde_json::from_value(g.config_json.clone()).ok())
        .unwrap_or_default();

    match guild_config.nickname_registration_role_id {
        Some(role_id) => Ok(Some(RegistrationPath::FromNickname(role_id))),
        None => {
            // Nickname registration not configured; direct user to /register.
            respond_ephemeral(
                ctx,
                component,
                "You are not registered yet.\n\n\
                Please run `/register <minecraft_username>` to link your Minecraft account.",
            )
            .await?;
            Ok(None)
        }
    }
}

/// Send an ephemeral direct response (only valid before any ack/defer).
async fn respond_ephemeral(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    content: &str,
) -> Result<(), Error> {
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Message(
                serenity::CreateInteractionResponseMessage::new()
                    .content(content)
                    .ephemeral(true),
            ),
        )
        .await?;
    Ok(())
}

/// Send an ephemeral followup message (only valid after a defer).
async fn send_followup(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    content: &str,
) -> Result<(), Error> {
    component
        .create_followup(
            ctx,
            serenity::CreateInteractionResponseFollowup::new()
                .content(content)
                .ephemeral(true),
        )
        .await?;
    Ok(())
}
