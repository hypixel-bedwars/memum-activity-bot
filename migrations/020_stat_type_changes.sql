BEGIN;

ALTER TABLE hypixel_stats_snapshot ADD COLUMN stat_value_int8 INT8;
ALTER TABLE discord_stats_snapshot ADD COLUMN stat_value_int8 INT8;
ALTER TABLE sweep_cursor ADD COLUMN stat_value_int8 INT8;
ALTER TABLE stat_deltas ADD COLUMN old_value_int8 INT8;
ALTER TABLE stat_deltas ADD COLUMN new_value_int8 INT8;
ALTER TABLE stat_deltas ADD COLUMN delta_int8 INT8;
ALTER TABLE daily_snapshots ADD COLUMN stat_value_int8 INT8;

UPDATE hypixel_stats_snapshot SET stat_value_int8 = FLOOR(stat_value)::BIGINT;
UPDATE discord_stats_snapshot SET stat_value_int8 = FLOOR(stat_value)::BIGINT;
UPDATE sweep_cursor SET stat_value_int8 = FLOOR(stat_value)::BIGINT;

UPDATE stat_deltas
SET old_value_int8 = FLOOR(old_value)::BIGINT,
    new_value_int8 = FLOOR(new_value)::BIGINT,
    delta_int8 = FLOOR(delta)::BIGINT;

UPDATE daily_snapshots SET stat_value_int8 = FLOOR(stat_value)::BIGINT;

ALTER TABLE hypixel_stats_snapshot DROP COLUMN stat_value;
ALTER TABLE discord_stats_snapshot DROP COLUMN stat_value;
ALTER TABLE sweep_cursor DROP COLUMN stat_value;
ALTER TABLE stat_deltas DROP COLUMN old_value;
ALTER TABLE stat_deltas DROP COLUMN new_value;
ALTER TABLE stat_deltas DROP COLUMN delta;
ALTER TABLE daily_snapshots DROP COLUMN stat_value;

ALTER TABLE hypixel_stats_snapshot RENAME COLUMN stat_value_int8 TO stat_value;
ALTER TABLE discord_stats_snapshot RENAME COLUMN stat_value_int8 TO stat_value;
ALTER TABLE sweep_cursor RENAME COLUMN stat_value_int8 TO stat_value;
ALTER TABLE stat_deltas RENAME COLUMN old_value_int8 TO old_value;
ALTER TABLE stat_deltas RENAME COLUMN new_value_int8 TO new_value;
ALTER TABLE stat_deltas RENAME COLUMN delta_int8 TO delta;
ALTER TABLE daily_snapshots RENAME COLUMN stat_value_int8 TO stat_value;

COMMIT;