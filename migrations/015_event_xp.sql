CREATE TABLE event_xp (
    id          BIGSERIAL PRIMARY KEY,
    event_id    BIGINT           NOT NULL REFERENCES events(id)       ON DELETE CASCADE,
    user_id     BIGINT           NOT NULL REFERENCES users(id)        ON DELETE CASCADE,
    stat_name   TEXT             NOT NULL,
    delta_id    BIGINT                    REFERENCES stat_deltas(id)  ON DELETE SET NULL,
    units       INTEGER          NOT NULL,
    xp_per_unit DOUBLE PRECISION NOT NULL,
    xp_earned   DOUBLE PRECISION NOT NULL,
    created_at  TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    UNIQUE(event_id, delta_id)
);
CREATE INDEX idx_event_xp_event_user ON event_xp(event_id, user_id);
CREATE INDEX idx_event_xp_user       ON event_xp(user_id);
