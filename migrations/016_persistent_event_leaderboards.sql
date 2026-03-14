-- Migration 016: persistent event leaderboards table
--
-- Stores the Discord message IDs for each event's persistent leaderboard so
-- the background updater can edit them on a schedule.

CREATE TABLE persistent_event_leaderboards (
    id                BIGSERIAL PRIMARY KEY,
    event_id          BIGINT NOT NULL UNIQUE,
    guild_id          BIGINT NOT NULL,
    channel_id        BIGINT NOT NULL,
    message_ids       JSONB NOT NULL DEFAULT '[]',
    status_message_id BIGINT NOT NULL DEFAULT 0,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_updated      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
