-- Initial schema for memum-activity-bot.
--
-- Design notes:
--   - guild_id and discord_user_id are stored as INTEGER (i64) since Discord
--     snowflakes fit in a signed 64-bit integer.
--   - Snapshot tables use an EAV (entity-attribute-value) pattern so that new
--     stats can be tracked without schema changes.
--   - Timestamps are stored as TEXT in ISO 8601 format for human readability
--     and broad compatibility.

-- =========================================================================
-- Guilds
-- =========================================================================
CREATE TABLE IF NOT EXISTS guilds (
    guild_id            INTEGER PRIMARY KEY,  -- Discord guild snowflake
    registered_role_id  INTEGER,              -- role assigned on /register
    config_json         TEXT NOT NULL DEFAULT '{}'
);

-- =========================================================================
-- Registered users
-- =========================================================================
CREATE TABLE IF NOT EXISTS users (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    discord_user_id  INTEGER NOT NULL,
    minecraft_uuid   TEXT    NOT NULL,
    guild_id         INTEGER NOT NULL REFERENCES guilds(guild_id),
    registered_at    TEXT    NOT NULL,
    UNIQUE(discord_user_id, guild_id)
);

CREATE INDEX IF NOT EXISTS idx_users_guild ON users(guild_id);

-- =========================================================================
-- Hypixel stats snapshots (EAV)
-- =========================================================================
CREATE TABLE IF NOT EXISTS hypixel_stats_snapshot (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    stat_name  TEXT    NOT NULL,
    stat_value REAL    NOT NULL DEFAULT 0,
    timestamp  TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_hypixel_snap_user_stat_ts
    ON hypixel_stats_snapshot(user_id, stat_name, timestamp);

-- =========================================================================
-- Discord activity stats snapshots (EAV)
-- =========================================================================
CREATE TABLE IF NOT EXISTS discord_stats_snapshot (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    stat_name  TEXT    NOT NULL,
    stat_value REAL    NOT NULL DEFAULT 0,
    timestamp  TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_discord_snap_user_stat_ts
    ON discord_stats_snapshot(user_id, stat_name, timestamp);

-- =========================================================================
-- Accumulated points
-- =========================================================================
CREATE TABLE IF NOT EXISTS points (
    user_id      INTEGER PRIMARY KEY REFERENCES users(id),
    total_points REAL    NOT NULL DEFAULT 0,
    last_updated TEXT    NOT NULL
);
