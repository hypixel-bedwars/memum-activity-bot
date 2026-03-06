-- Migration 005: Add head texture fields to users table.
--
-- This migration adds two new columns to the users table:
-- 1. head_texture: A TEXT field to store the URL or identifier of the user's head texture.
-- 2. head_texture_updated_at: A TIMESTAMPTZ field to store the timestamp of when the head texture was last updated.

ALTER TABLE users ADD COLUMN head_texture TEXT;
ALTER TABLE users ADD COLUMN head_texture_updated_at TIMESTAMPTZ;