-- Create table for persistent event status messages
CREATE TABLE event_status_messages (
    id          BIGSERIAL PRIMARY KEY,
    event_id    BIGINT NOT NULL UNIQUE,
    channel_id  BIGINT NOT NULL,
    message_id  BIGINT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
