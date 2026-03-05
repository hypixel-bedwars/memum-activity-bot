pub mod admin;
/// Command registration.
///
/// All slash commands are aggregated here into a single `Vec` that Poise uses
/// during framework setup. To add a new command, implement it in its own file
/// inside the appropriate subfolder and add it to the vector returned by `all()`.
///
/// Commands are organized into three submodules:
/// - `registration` — user-facing account linking commands
/// - `stats`        — stat viewing commands
/// - `admin`        — server configuration commands (admin only)
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
        admin::edit_stats::edit_stats(),
    ]
}
