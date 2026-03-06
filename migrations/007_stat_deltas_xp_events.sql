-- Migration 007: Event-based XP audit tables
--
-- stat_deltas  — every stat change detected by a sweeper, stored permanently.
-- xp_events    — the exact XP awarded for each delta at the time it occurred,
--                including the multiplier that was active then. Historical XP
--                never changes even when admins later edit guild multipliers.

CREATE TABLE IF NOT EXISTS stat_deltas (
    id         BIGSERIAL PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    stat_name  TEXT NOT NULL,
    old_value  DOUBLE PRECISION NOT NULL,
    new_value  DOUBLE PRECISION NOT NULL,
    delta      DOUBLE PRECISION NOT NULL,
    source     TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS xp_events (
    id          BIGSERIAL PRIMARY KEY,
    user_id     BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    stat_name   TEXT NOT NULL,
    delta_id    BIGINT NOT NULL REFERENCES stat_deltas(id) ON DELETE CASCADE,
    units       INTEGER NOT NULL,
    xp_per_unit DOUBLE PRECISION NOT NULL,
    xp_earned   DOUBLE PRECISION NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_stat_deltas_user_id ON stat_deltas(user_id);
CREATE INDEX IF NOT EXISTS idx_xp_events_user_id   ON xp_events(user_id);
CREATE INDEX IF NOT EXISTS idx_xp_events_delta_id  ON xp_events(delta_id);
