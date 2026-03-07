// Althrough this is not a skeleton project anymore
// I would prefer to keep some of the unused codearound for now
// most of the dead code is just me nor reading them yet
// but as i work on the project with very bug fix i will eventually read most of them
// and thats why i am letting this be for now.
#![allow(dead_code)]

//! Entry point for the memum-activity-bot.
//!
//! Responsibilities (in order):
//! 1. Load environment variables from `.env`.
//! 2. Initialize structured logging via `tracing`.
//! 3. Parse application configuration.
//! 4. Initialize the SQLite database and run migrations.
//! 5. Build the Poise framework (commands, event handler, sweeper).
//! 6. Start the Discord gateway client.

mod bot;
mod commands;
mod config;
mod database;
mod discord_stats;
mod events;
mod hypixel;
mod leaderboard_card;
mod leaderboard_updater;
mod level_card;
mod milestones;
mod permissions;
mod shared;
mod stats_definitions;
mod sweeper;
mod xp;

use poise::serenity_prelude as serenity;
use tracing::info;
use tracing_subscriber::EnvFilter;

use config::AppConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Load .env file (silently ignore if missing — variables may be set
    //    directly in the environment).
    let _ = dotenv::dotenv();

    // 2. Initialize tracing. The RUST_LOG env var controls filtering; default
    //    to `info` for the bot crate and `warn` for everything else.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("memum_activity_bot=info,warn")),
        )
        .init();

    info!("Starting memum-activity-bot...");

    // 3. Parse configuration from environment.
    let config = AppConfig::from_env();
    let token = config.discord_token.clone();

    // 4. Initialize the database.
    let db = database::init_db(&config.database_url).await?;
    info!("Database initialized.");

    // 5. Build the Poise framework.
    let framework = bot::build(config, db).await?;

    // 6. Build and start the Serenity client.
    let mut client = serenity::ClientBuilder::new(&token, bot::intents())
        .framework(framework)
        .await?;

    info!("Connecting to Discord...");
    client.start().await?;

    Ok(())
}
