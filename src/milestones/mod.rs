/// Milestone event hook.
///
/// This module provides the `handle_milestone_reached` function, which is
/// called every time a user crosses a milestone level threshold for the first
/// time during an XP sweep.
///
/// # Extension point
///
/// The function body is intentionally empty. In the future this is where
/// milestone rewards should be implemented, for example:
/// - Assigning a Discord role to the user
/// - Sending a congratulatory announcement to a configured channel
/// - Granting server permissions
/// - Issuing in-game or bot rewards
///
/// The milestone system works correctly without any logic here; this hook
/// simply does nothing until those features are added.

/// Called when a user first reaches a milestone level.
///
/// # Parameters
/// - `discord_user_id`: The Discord snowflake ID of the user who reached
///   the milestone.
/// - `milestone_level`: The level threshold of the milestone that was crossed.
///
/// # Future use
/// Implement role assignments, announcements, or reward logic here.
pub async fn handle_milestone_reached(discord_user_id: u64, milestone_level: i32) {
    // Suppress unused-variable warnings until logic is added.
    let _ = (discord_user_id, milestone_level);

    // TODO: add milestone reward logic here.
    // e.g.
    //   - assign a Discord role based on milestone_level
    //   - send a message to an announcement channel
    //   - update a rewards table in the database
}
