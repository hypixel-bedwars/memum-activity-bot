/// Bot framework setup.
///
/// Configures and builds the Poise framework, registers commands, sets up
/// Discord gateway intents, wires the event handler for Discord stats
/// tracking, and starts the background stat sweeper and leaderboard updater.
use std::collections::HashMap;
use std::sync::Arc;

use poise::serenity_prelude as serenity;
use sqlx::PgPool;
use tracing::info;

use crate::commands;
use crate::commands::leaderboard::leaderboard as lb;
use crate::config::AppConfig;
use crate::database::models::MessageValidationState;
use crate::discord_stats::tracker;
use crate::events::events;
use crate::hypixel::client::HypixelClient;
use crate::shared::types::{Data, Error};
use crate::sweeper;
use crate::utils::leaderboard_updater;

/// Build and return the Poise framework, ready to be started.
///
/// This function:
/// 1. Creates the `HypixelClient`.
/// 2. Configures Poise with all commands, the event handler, and the
///    pre-command hook.
/// 3. In the `setup` callback, starts the stat sweeper background task.
pub async fn build(config: AppConfig, db: PgPool) -> Result<poise::Framework<Data, Error>, Error> {
    let hypixel = Arc::new(HypixelClient::new(config.hypixel_api_key.clone()));

    // Clone values that need to move into closures.
    let lb_db = db.clone();
    let lb_config = config.clone();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            // Register all slash commands.
            commands: commands::all(),

            // Event handler: forward events to the Discord stats tracker.
            event_handler: |ctx, event, _framework, data| {
                Box::pin(async move {
                    // Calls event handler for buttons and other custom interactions (if any).
                    if let Err(e) = events::event_handler(ctx, event, data).await {
                        tracing::error!(error = %e, "Custom events handler failed");
                    }

                    // Calls event handler for tracking Discord stats.
                    tracker::handle_event(event, data).await
                })
            },

            // Pre-command hook: track command usage as a Discord stat.
            pre_command: |ctx| {
                Box::pin(async move {
                    if let Some(guild_id) = ctx.guild_id() {
                        let data = ctx.data();
                        tracker::record_command_usage(
                            &data.db,
                            data,
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

                let leaderboard_cache = lb::new_cache(config.leaderboard_cache_seconds);

                // Create bot data
                let data = Data {
                    db: db.clone(),
                    hypixel: hypixel.clone(),
                    config: config.clone(),
                    leaderboard_cache,
                    message_validation: MessageValidationState::default(),
                    voice_sessions: Arc::new(std::sync::Mutex::new(HashMap::new())),
                    http: ctx.http.clone(),
                };

                // Create Arc for background tasks
                let data_arc = Arc::new(data.clone());

                // Hypixel sweeper
                let sweeper_data = Arc::clone(&data_arc);

                tokio::spawn(async move {
                    info!("Hypixel background sweeper started.");

                    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(
                        sweeper_data.config.hypixel_sweep_interval_seconds,
                    ));

                    loop {
                        ticker.tick().await;

                        if let Err(e) =
                            sweeper::hypixel_sweeper::run_hypixel_stale_sweep(&sweeper_data).await
                        {
                            tracing::error!(error = %e, "Hypixel stale sweep failed");
                        }
                    }
                });

                // Daily snapshot
                let snapshot_data = Arc::clone(&data_arc);
                tokio::spawn(sweeper::daily_snapshot::start_daily_snapshot_loop(
                    snapshot_data,
                ));

                // Leaderboard updater
                leaderboard_updater::start_leaderboard_updater(
                    lb_db,
                    Arc::clone(&ctx.http),
                    lb_config,
                );

                // Register slash commands to the configured guild only.
                // Guild-scoped registration is instant (no propagation delay)
                // and is controlled by the GUILD_ID environment variable.
                let guild_id = serenity::GuildId::new(config.guild_id);
                poise::builtins::register_in_guild(ctx, &framework.options().commands, guild_id)
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            error = %e,
                            guild_id = config.guild_id,
                            "Failed to register slash commands in guild"
                        );
                        e
                    })?;

                info!(
                    commands = framework.options().commands.len(),
                    guild_id = config.guild_id,
                    "Slash commands registered in guild."
                );

                // Return Data to Poise
                Ok(data)
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
/// - `GUILD_VOICE_STATES` — to track voice channel join/leave for voice_minutes.
/// - `MESSAGE_CONTENT` — required to read message content (privileged intent).
pub fn intents() -> serenity::GatewayIntents {
    serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::GUILD_MESSAGE_REACTIONS
        | serenity::GatewayIntents::GUILD_VOICE_STATES
        | serenity::GatewayIntents::MESSAGE_CONTENT
}
