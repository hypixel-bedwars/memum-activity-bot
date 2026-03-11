/// Admin command modules.
///
/// This module contains commands and utilities restricted to server administrators.
/// Each submodule implements a specific admin-related command or feature.
///
/// Submodules:
/// - `edit_events`: Commands for creating and managing guild events.
/// - `edit_stats`: Commands for editing user statistics.
/// - `set_nickname_registration_role`: Set the role required for nickname registration.
/// - `set_register_role`: Set the role required for registration.
/// - `xp`: Admin commands for managing user experience points.
pub mod edit_events;
pub mod edit_stats;
pub mod force_register;
pub mod set_nickname_registration_role;
pub mod set_register_role;
pub mod xp;
