CREATE TABLE event_stats (
    id          BIGSERIAL PRIMARY KEY,
    event_id    BIGINT           NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    stat_name   TEXT             NOT NULL,
    xp_per_unit DOUBLE PRECISION NOT NULL DEFAULT 0 CHECK (xp_per_unit >= 0),
    created_at  TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    UNIQUE(event_id, stat_name)
);
CREATE INDEX idx_event_stats_event_id ON event_stats(event_id);
