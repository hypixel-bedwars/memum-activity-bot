/// Command registration.
///
/// All slash commands are aggregated here into a single `Vec` that Poise uses
/// during framework setup. To add a new command, implement it in its own file
/// and add it to the vector returned by `all()`.
pub mod register;
pub mod stats;

use crate::shared::types::{Data, Error};

/// Returns all registered commands.
pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![register::register(), stats::stats()]
}
