-- Migration 003: add per-source sweep cursor state.
--
-- The sweeper cursor tracks the last processed value/timestamp for each
-- (user, source, stat_name) tuple. This allows independent sweep loops
-- (e.g. Discord and Hypixel) to process deltas without sharing a single
-- global XP timestamp.

CREATE TABLE IF NOT EXISTS sweep_cursor (
    user_id            INTEGER NOT NULL REFERENCES users(id),
    source             TEXT    NOT NULL,
    stat_name          TEXT    NOT NULL,
    stat_value         REAL    NOT NULL DEFAULT 0,
    last_snapshot_ts   TEXT    NOT NULL,
    updated_at         TEXT    NOT NULL,
    PRIMARY KEY(user_id, source, stat_name)
);

CREATE INDEX IF NOT EXISTS idx_sweep_cursor_user_source
    ON sweep_cursor(user_id, source);
