-- Persistent leaderboard configuration per guild.
-- Stores which channel/message the bot should auto-update.
CREATE TABLE IF NOT EXISTS persistent_leaderboards (
    guild_id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    message_ids TEXT NOT NULL DEFAULT '[]',
    status_message_id INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    last_updated TEXT NOT NULL
);