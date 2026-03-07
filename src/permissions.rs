/// Reusable permission checks for Poise commands.
///
/// The `admin_check` function is designed to be used as a `check` attribute on
/// any command that should be restricted to admins. It checks whether the
/// invoking member holds any of the roles listed in `AppConfig.admin_role_ids`.
///
/// Usage:
/// ```
/// #[poise::command(slash_command, guild_only, check = "crate::permissions::admin_check")]
/// pub async fn my_admin_command(ctx: Context<'_>) -> Result<(), Error> { ... }
/// ```
use crate::shared::types::{Context, Error};
use tracing::warn;

/// Returns `true` if the invoking member holds at least one of the configured
/// admin roles, `false` otherwise.
///
/// By default Poise will silently block the command when this returns `Ok(false)`.
/// To improve UX and observability we proactively send an ephemeral denial
/// message to the invoking user and log a warning when a non-admin attempts
/// to run an admin command.
///
/// Note: We intentionally swallow send errors — the check must not fail with
/// an internal error just because we couldn't notify the user.
pub async fn admin_check(ctx: Context<'_>) -> Result<bool, Error> {
    // Try to fetch the guild member; if we can't, deny and notify.
    let member = match ctx.author_member().await {
        Some(m) => m,
        None => {
            let _ = ctx
                .send(
                    poise::CreateReply::default()
                        .ephemeral(true)
                        .content("You do not have permission to use this command."),
                )
                .await;
            warn!(
                "admin_check: failed to fetch guild member for user {}. Denying access.",
                ctx.author().name
            );
            return Ok(false);
        }
    };

    let admin_role_ids = &ctx.data().config.admin_role_ids;

    // If the invoking member has any configured admin role, allow.
    let is_admin = member
        .roles
        .iter()
        .any(|role_id| admin_role_ids.contains(&role_id.get()));

    if is_admin {
        return Ok(true);
    }

    // Deny: send an ephemeral message so the user sees a helpful reason and log it.
    let _ = ctx
        .send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("You do not have permission to use this command."),
        )
        .await;

    warn!(
        "admin_check: permission denied for user {} (id {}).",
        ctx.author().name,
        ctx.author().id,
    );

    Ok(false)
}
