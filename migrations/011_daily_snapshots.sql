-- Migration: 011_daily_snapshots.sql
-- Create a daily_snapshots table that stores an immutable, idempotent
-- daily baseline of user stats (one row per user_id / stat_name / snapshot_date).

CREATE TABLE IF NOT EXISTS daily_snapshots (
    user_id BIGINT NOT NULL,
    stat_name TEXT NOT NULL,
    stat_value DOUBLE PRECISION NOT NULL CHECK (stat_value >= 0),
    snapshot_date DATE NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- enforces uniqueness (one snapshot per user/stat/day)
    PRIMARY KEY (user_id, stat_name, snapshot_date),

    -- Foreign key to users table ensures snapshots reference an existing user.
    -- Use RESTRICT to avoid accidental cascade deletes and to preserve history;
    CONSTRAINT fk_daily_snapshots_user
      FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE RESTRICT
);

-- Useful for leaderboards for the day
CREATE INDEX idx_daily_snapshots_date
ON daily_snapshots (snapshot_date);

-- Helpful index for common queries
CREATE INDEX IF NOT EXISTS idx_daily_snapshots_user_date
    ON daily_snapshots (user_id, snapshot_date);
