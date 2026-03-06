-- Migration 004: add persistent leaderboard configuration.
-- 
-- Persistent leaderboard configuration per guild.
-- Stores which channel/message the bot should auto-update.

CREATE TABLE IF NOT EXISTS persistent_leaderboards (
    guild_id          BIGINT PRIMARY KEY REFERENCES guilds(guild_id) ON DELETE CASCADE,
    channel_id        BIGINT NOT NULL,
    message_ids       JSONB  NOT NULL DEFAULT '[]'::jsonb,
    status_message_id BIGINT NOT NULL DEFAULT 0,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_updated      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);