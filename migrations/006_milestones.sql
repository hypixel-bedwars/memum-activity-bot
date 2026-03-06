-- Milestone definitions per guild.
--
-- Each row represents a level threshold that is tracked as a milestone.
-- The UNIQUE constraint on (guild_id, level) prevents duplicate milestones
-- within the same guild.
CREATE TABLE IF NOT EXISTS milestones (
    id       BIGSERIAL PRIMARY KEY,
    guild_id BIGINT NOT NULL REFERENCES guilds(guild_id) ON DELETE CASCADE,
    level    INTEGER NOT NULL,
    UNIQUE (guild_id, level)
);
