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
//! 2. Initialize structured logging via `tracing` (console + file + Discord).
//! 3. Install a panic hook that converts panics into structured `error!` events.
//! 4. Parse application configuration.
//! 5. Initialize the Postgres database and run migrations.
//! 6. Build the Poise framework (commands, event handler, sweeper).
//! 7. Start the Discord gateway client.
//! 8. Spawn the async Discord log worker (broadcasts errors/warnings to guilds).

mod bot;
mod cards;
mod commands;
mod config;
mod database;
mod discord_stats;
mod events;
mod font;
mod hypixel;
mod logging;
mod milestones;
mod shared;
mod sweeper;
mod utils;
mod xp;

use poise::serenity_prelude as serenity;
use tracing::info;

use config::AppConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Load .env file (silently ignore if missing — variables may be set
    //    directly in the environment).
    let _ = dotenv::dotenv();

    // 2. Initialize the three-layer tracing subscriber:
    //      • Console — coloured stdout with file/line context
    //      • File    — plain-text daily-rotating logs/YYYY-MM-DD.log
    //      • Discord — ERROR + WARN forwarded to guild log channels
    //    The returned `LoggingSetup` MUST be kept alive: `_file_guard` inside
    //    it owns the background file-writer thread.
    let logging::LoggingSetup {
        discord_log_rx,
        _file_guard,
    } = logging::init_logging();

    // 3. Convert panics into structured `error!` events so they reach all
    //    three layers (console, file, Discord).
    logging::install_panic_hook();

    info!("Starting memum-activity-bot...");

    // 4. Parse configuration from environment.
    let config = AppConfig::from_env();
    let token = config.discord_token.clone();

    // 5. Initialize the database.
    let db = database::init_db(&config.database_url).await?;
    // Clone before `db` is moved into the framework — needed by the Discord
    // log worker that starts later, after the HTTP client is available.
    let db_for_log_worker = db.clone();
    info!("Database initialized.");

    // 6. Build the Poise framework.
    let framework = bot::build(config, db).await?;

    // 7. Build the Serenity client.
    let mut client = serenity::ClientBuilder::new(&token, bot::intents())
        .framework(framework)
        .await?;

    // 8. Spawn the Discord log worker now that we have both the database pool
    //    and the HTTP client. It drains the channel produced in step 2 and
    //    broadcasts each ERROR/WARN event as a Discord embed to every guild
    //    with a configured log channel.
    tokio::spawn(logging::discord_log_worker(
        discord_log_rx,
        db_for_log_worker,
        client.http.clone(),
    ));

    info!("Connecting to Discord...");
    client.start().await?;

    Ok(())
}
