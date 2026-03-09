-- Migration 008: Add Hypixel refresh tracking columns to users table.
--
-- last_hypixel_refresh  — timestamp of the most recent successful Hypixel API
--                         fetch for this user. NULL means never fetched.
--                         Updated by sweep_hypixel_user() after every successful
--                         API call (both background and command-triggered).
--
-- last_command_activity — timestamp of the most recent stat command (/level,
--                         /stats) invoked by this user. NULL means no stat
--                         command has been used since this migration ran.
--                         Used by the sweeper to priority-sort active users
--                         to the front of each sweep cycle.

ALTER TABLE users
    ADD COLUMN last_hypixel_refresh  TIMESTAMPTZ,
    ADD COLUMN last_command_activity TIMESTAMPTZ;
    
CREATE INDEX idx_users_last_hypixel_refresh
ON users (last_hypixel_refresh);