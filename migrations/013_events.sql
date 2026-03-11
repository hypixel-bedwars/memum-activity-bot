CREATE TABLE events (
    id                  BIGSERIAL PRIMARY KEY,
    guild_id            BIGINT      NOT NULL REFERENCES guilds(guild_id) ON DELETE CASCADE,
    name                TEXT        NOT NULL,
    description         TEXT,
    start_date          TIMESTAMPTZ NOT NULL,
    end_date            TIMESTAMPTZ NOT NULL,
    start_snapshot_date DATE,
    end_snapshot_date   DATE,
    status              TEXT        NOT NULL DEFAULT 'pending'
                            CHECK (status IN ('pending', 'active', 'ended')),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(guild_id, name),
    CHECK (end_date > start_date)
);
CREATE INDEX idx_events_guild_status ON events(guild_id, status);
CREATE INDEX idx_events_start_date   ON events(start_date);
CREATE INDEX idx_events_end_date     ON events(end_date);
