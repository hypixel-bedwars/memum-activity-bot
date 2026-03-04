/// Bot framework setup.
///
/// Configures and builds the Poise framework, registers commands, sets up
/// Discord gateway intents, wires the event handler for Discord stats
/// tracking, and starts the background stat sweeper.
use std::sync::Arc;

use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;
use tracing::info;

use crate::commands;
use crate::config::AppConfig;
use crate::discord_stats::tracker;
use crate::hypixel::client::HypixelClient;
use crate::shared::types::{Data, Error};
use crate::sweeper;

/// Build and return the Poise framework, ready to be started.
///
/// This function:
/// 1. Creates the `HypixelClient`.
/// 2. Configures Poise with all commands, the event handler, and the
///    pre-command hook.
/// 3. In the `setup` callback, starts the stat sweeper background task.
pub async fn build(
    config: AppConfig,
    db: SqlitePool,
) -> Result<poise::Framework<Data, Error>, Error> {
    let hypixel = Arc::new(HypixelClient::new(config.hypixel_api_key.clone()));

    // Clone values that need to move into closures.
    let sweep_db = db.clone();
    let sweep_hypixel = hypixel.clone();
    let sweep_interval = config.sweep_interval_seconds;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            // Register all slash commands.
            commands: commands::all(),

            // Event handler: forward events to the Discord stats tracker.
            event_handler: |_ctx, event, _framework, data| {
                Box::pin(async move { tracker::handle_event(event, data).await })
            },

            // Pre-command hook: track command usage as a Discord stat.
            pre_command: |ctx| {
                Box::pin(async move {
                    if let Some(guild_id) = ctx.guild_id() {
                        let data = ctx.data();
                        tracker::record_command_usage(
                            &data.db,
                            ctx.author().id.get() as i64,
                            guild_id.get() as i64,
                        )
                        .await;
                    }
                })
            },

            ..Default::default()
        })
        .setup(move |ctx, _ready, framework| {
            Box::pin(async move {
                info!("Bot is connected and ready!");

                // Register slash commands globally.
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                info!("Slash commands registered globally.");

                // Start the background stat sweeper.
                sweeper::start_sweeper(sweep_db, sweep_hypixel, sweep_interval);

                Ok(Data {
                    db,
                    hypixel,
                    config,
                })
            })
        })
        .build();

    Ok(framework)
}

/// Return the gateway intents required by the bot.
///
/// We need:
/// - `GUILDS` — for guild/role lookups.
/// - `GUILD_MESSAGES` — to track message activity.
/// - `GUILD_MESSAGE_REACTIONS` — to track reaction activity.
/// - `MESSAGE_CONTENT` — required to read message content (privileged intent).
pub fn intents() -> serenity::GatewayIntents {
    serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::GUILD_MESSAGE_REACTIONS
        | serenity::GatewayIntents::MESSAGE_CONTENT
}
