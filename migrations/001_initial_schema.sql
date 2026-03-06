-- Initial schema for memum-activity-bot.
--
-- Design notes:
--   - guild_id and discord_user_id are stored as BIGINT (i64) since Discord
--     snowflakes fit in a signed 64-bit BIGINT.
--   - Snapshot tables use an EAV (entity-attribute-value) pattern so that new
--     stats can be tracked without schema changes.
--   - Timestamps are stored as TIMESTAMPTZ in ISO 8601 format for human readability
--     and broad compatibility.

-- =========================================================================
-- Guilds
-- =========================================================================
CREATE TABLE IF NOT EXISTS guilds (
    guild_id           BIGINT PRIMARY KEY,
    registered_role_id BIGINT,
    config_json        JSONB NOT NULL DEFAULT '{}'::jsonb
);

-- =========================================================================
-- Registered users
-- =========================================================================
CREATE TABLE IF NOT EXISTS users (
    id              BIGSERIAL PRIMARY KEY,
    discord_user_id BIGINT NOT NULL,
    minecraft_uuid  UUID NOT NULL,
    guild_id        BIGINT NOT NULL REFERENCES guilds(guild_id),
    registered_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE(discord_user_id, guild_id)
);

CREATE INDEX IF NOT EXISTS idx_users_guild
ON users(guild_id);

-- =========================================================================
-- Hypixel stats snapshots (EAV)
-- =========================================================================
CREATE TABLE IF NOT EXISTS hypixel_stats_snapshot (
    id         BIGSERIAL PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    stat_name  TEXT NOT NULL,
    stat_value DOUBLE PRECISION NOT NULL DEFAULT 0,
    timestamp  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_hypixel_user_stat_ts
ON hypixel_stats_snapshot(user_id, stat_name, timestamp DESC);

-- =========================================================================
-- Discord activity stats snapshots (EAV)
-- =========================================================================
CREATE TABLE IF NOT EXISTS discord_stats_snapshot (
    id         BIGSERIAL PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    stat_name  TEXT NOT NULL,
    stat_value DOUBLE PRECISION NOT NULL DEFAULT 0,
    timestamp  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_discord_user_stat_ts
ON discord_stats_snapshot(user_id, stat_name, timestamp DESC);

-- XP table (replaces points)
CREATE TABLE IF NOT EXISTS xp (
    user_id      BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    total_xp     DOUBLE PRECISION NOT NULL DEFAULT 0,
    level        INTEGER NOT NULL DEFAULT 1,
    last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Speeds up leaderboard queries ordered by XP
CREATE INDEX IF NOT EXISTS idx_xp_total_xp_desc
ON xp (total_xp DESC);