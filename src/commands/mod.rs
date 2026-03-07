pub mod admin;
/// Command registration.
///
/// All slash commands are aggregated here into a single `Vec` that Poise uses
/// during framework setup. To add a new command, implement it in its own file
/// inside the appropriate subfolder and add it to the vector returned by `all()`.
///
/// Commands are organized into five submodules:
/// - `registration`  — user-facing account linking commands
/// - `stats`         — stat viewing commands
/// - `admin`         — server configuration commands (admin only)
/// - `leaderboard`   — leaderboard commands (user + admin)
/// - `milestone`     — milestone management and progress commands
pub mod leaderboard;
pub mod milestone;
pub mod registration;
pub mod stats;

use crate::shared::types::{Data, Error};

/// Returns all registered commands.
pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![
        registration::register::register(),
        registration::unregister::unregister(),
        registration::send_registration_message::send_registration_message(),
        stats::stats::stats(),
        stats::level::level(),
        admin::set_register_role::set_register_role(),
        admin::set_nickname_registration_role::set_nickname_registration_role(),
        admin::set_nickname_registration_role::clear_nickname_registration_role(),
        admin::edit_stats::edit_stats(),
        admin::xp::xp(),
        leaderboard::leaderboard::leaderboard(),
        leaderboard::leaderboard_create::leaderboard_create(),
        leaderboard::leaderboard_remove::leaderboard_remove(),
        milestone::milestone::milestone(),
    ]
}
