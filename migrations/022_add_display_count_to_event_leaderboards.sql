-- Migration 022: Add display_count to persistent_event_leaderboards
--
-- Adds configurable display limit for persistent event leaderboards.
-- Default is 20 players (2 pages at 10 per page).

ALTER TABLE persistent_event_leaderboards
ADD COLUMN display_count INTEGER NOT NULL DEFAULT 20;
