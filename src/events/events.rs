use poise::serenity_prelude as serenity;
use tracing::warn;

use crate::commands::leaderboard::leaderboard as lb;
use crate::commands::registration::register::perform_registration;
use crate::shared::types::{Data, Error};

/// Extract the Minecraft username from a server nickname.
///
/// Expects the format `[NNN emoji] Username` — e.g. `[313 💫] VA80`.
/// Returns `Some(username)` if the pattern is matched, `None` otherwise.
fn extract_username_from_nickname(nickname: &str) -> Option<&str> {
    // Find the closing bracket followed by a space.
    let bracket_end = nickname.find("] ")?;
    let username = nickname[bracket_end + 2..].trim();
    if username.is_empty() {
        None
    } else {
        Some(username)
    }
}

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
            }
        }
    }

    Ok(())
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

    // Fetch the guild member to read their server nickname.
    let member = match guild_id.member(&ctx.http, component.user.id).await {
        Ok(m) => m,
        Err(e) => {
            warn!(
                "Failed to fetch member {} in guild {}: {}",
                component.user.id, guild_id, e
            );
            respond_ephemeral(
                ctx,
                component,
                "Could not retrieve your server profile. Please try again.",
            )
            .await?;
            return Ok(());
        }
    };

    // The server nickname is preferred; fall back to the global username if absent,
    // but we require the structured format so we reject anything that doesn't match.
    let nickname = match member.nick.as_deref() {
        Some(n) => n,
        None => {
            respond_ephemeral(
                ctx,
                component,
                "You don't have a server nickname set.\n\n\
                Please ask an admin to set your nickname to the format:\n\
                `[NNN emoji] YourMinecraftUsername`\n\
                *(e.g. `[313 💫] VA80`)*",
            )
            .await?;
            return Ok(());
        }
    };

    let minecraft_username = match extract_username_from_nickname(nickname) {
        Some(u) => u.to_string(),
        None => {
            respond_ephemeral(
                ctx,
                component,
                &format!(
                    "Your server nickname **`{nickname}`** doesn't match the required format.\n\n\
                    It must look like: `[NNN emoji] YourMinecraftUsername`\n\
                    *(e.g. `[313 💫] VA80`, `[204 ✨] CosmicFuji`)*\n\n\
                    Please ask an admin to update your nickname, then try again."
                ),
            )
            .await?;
            return Ok(());
        }
    };

    // Acknowledge the interaction immediately; Discord requires a response within 3 seconds.
    // We use a deferred ephemeral reply so we can follow up after the async API calls complete.
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Defer(
                serenity::CreateInteractionResponseMessage::new().ephemeral(true),
            ),
        )
        .await?;

    let msg = perform_registration(
        ctx,
        data,
        guild_id,
        component.user.id,
        &component.user.tag(),
        &minecraft_username,
    )
    .await;

    let reply_text = match msg {
        Ok(text) => text,
        Err(e) => {
            warn!(
                user = component.user.id.get(),
                error = %e,
                "perform_registration returned an unexpected error"
            );
            format!("An unexpected error occurred during registration: {e}")
        }
    };

    component
        .create_followup(
            ctx,
            serenity::CreateInteractionResponseFollowup::new()
                .content(reply_text)
                .ephemeral(true),
        )
        .await?;

    Ok(())
}

/// Send a one-shot ephemeral response to a component interaction.
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
