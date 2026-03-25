CREATE TABLE event_participants (
    event_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    disqualified BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (event_id, user_id),

    FOREIGN KEY (event_id) REFERENCES events(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_event_participants_lookup
ON event_participants (event_id, user_id, disqualified);

ALTER TABLE users
ADD COLUMN event_ban_until TIMESTAMPTZ,
ADD COLUMN event_ban_reason TEXT;