use chrono::Utc;

use crate::database::queries;
use crate::shared::types::Data;
use poise::serenity_prelude::{self as serenity, CreateEmbed};

/// Severity level for a manual audit-trail Discord log.
///
/// These are used for intentional admin-action audit events (e.g. "Admin X ran
/// backfill", "Admin X set the register role"). They are separate from the
/// automatic tracing layer in `logging.rs` which captures all `error!`/`warn!`
/// macro calls.
///
/// `Clone` is derived so variants can be passed across loop iterations without
/// the manual match-arm workaround.
#[derive(Clone)]
pub enum LogType {
    Info,
    Warn,
    Debug,
    Error,
}

/// Post an audit-trail embed to the guild's configured log channel.
///
/// Fetches the log channel from the database on each call. Returns `Ok(())`
/// silently if no channel is configured for the guild.
pub async fn logger(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    log_type: LogType,
    msg: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let guild_id_i64 = guild_id.get() as i64;

    // Fetch configured log channel
    let Some(channel_id) = queries::get_guild_log_channel(&data.db, guild_id_i64).await? else {
        return Ok(()); // logging not configured
    };

    let channel_id = serenity::ChannelId::new(channel_id as u64);
    let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

    let embed = match log_type {
        LogType::Info => CreateEmbed::default()
            .title("ℹ️ Info")
            .description(&msg)
            .color(0x3498db_u32)
            .field("Time", &timestamp, false),

        LogType::Warn => CreateEmbed::default()
            .title("⚠️ Warning")
            .description(&msg)
            .color(0xf1c40f_u32)
            .field("Time", &timestamp, false),

        LogType::Debug => CreateEmbed::default()
            .title("🐛 Debug")
            .description(&msg)
            .color(0x95a5a6_u32)
            .field("Time", &timestamp, false),

        LogType::Error => CreateEmbed::default()
            .title("❌ Error")
            .description(&msg)
            .color(0xe74c3c_u32)
            .field("Time", &timestamp, false),
    };

    channel_id
        .send_message(ctx, serenity::CreateMessage::default().embed(embed))
        .await?;

    Ok(())
}

/// Post an audit-trail embed to a guild's log channel from a background task
/// context (where no Poise `Context` is available — only `&Http` + `&PgPool`).
///
/// Returns silently if no channel is configured for the guild.
pub async fn logger_system(
    http: &serenity::Http,
    pool: &sqlx::PgPool,
    guild_id: i64,
    log_type: LogType,
    msg: String,
) {
    let channel_id = match queries::get_guild_log_channel(pool, guild_id).await {
        Ok(Some(id)) => id,
        _ => return, // logging not configured
    };

    let channel = serenity::ChannelId::new(channel_id as u64);
    let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

    let embed = match log_type {
        LogType::Info => serenity::CreateEmbed::new()
            .title("ℹ️ Info")
            .description(&msg)
            .color(0x3498db_u32)
            .field("Time", &timestamp, false),

        LogType::Warn => serenity::CreateEmbed::new()
            .title("⚠️ Warning")
            .description(&msg)
            .color(0xf1c40f_u32)
            .field("Time", &timestamp, false),

        LogType::Error => serenity::CreateEmbed::new()
            .title("❌ Error")
            .description(&msg)
            .color(0xe74c3c_u32)
            .field("Time", &timestamp, false),

        LogType::Debug => serenity::CreateEmbed::new()
            .title("🐛 Debug")
            .description(&msg)
            .color(0x95a5a6_u32)
            .field("Time", &timestamp, false),
    };

    let _ = channel
        .send_message(http, serenity::CreateMessage::new().embed(embed))
        .await;
}
