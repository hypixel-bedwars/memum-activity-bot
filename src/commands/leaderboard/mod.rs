/// Leaderboard commands.
///
/// - `leaderboard`        — user-facing paginated leaderboard image
/// - `leaderboard_create` — admin: create a persistent auto-updating leaderboard
/// - `leaderboard_remove` — admin: remove the persistent leaderboard
/// - `helpers`            — shared image generation logic
pub mod helpers;
pub mod leaderboard;
pub mod leaderboard_create;
pub mod leaderboard_remove;
