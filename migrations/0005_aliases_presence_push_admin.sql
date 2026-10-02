-- Room directory/aliases, presence, pusher registration, and push-rule overrides.

ALTER TABLE rooms ADD COLUMN IF NOT EXISTS visibility TEXT NOT NULL DEFAULT 'private';

CREATE TABLE IF NOT EXISTS room_aliases (
    alias       TEXT PRIMARY KEY,
    room_id     TEXT NOT NULL REFERENCES rooms(room_id),
    creator     TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_room_aliases_room ON room_aliases (room_id);

CREATE TABLE IF NOT EXISTS user_presence (
    user_id         TEXT PRIMARY KEY,
    presence        TEXT NOT NULL DEFAULT 'offline',
    status_msg      TEXT,
    last_active_ts  BIGINT NOT NULL,
    currently_active BOOLEAN NOT NULL DEFAULT FALSE,
    stream_id       BIGINT NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS pushers (
    user_id             TEXT NOT NULL,
    pushkey             TEXT NOT NULL,
    app_id              TEXT NOT NULL,
    kind                TEXT,
    app_display_name    TEXT NOT NULL DEFAULT '',
    device_display_name TEXT NOT NULL DEFAULT '',
    profile_tag         TEXT,
    lang                TEXT NOT NULL DEFAULT 'en',
    data                JSONB NOT NULL DEFAULT '{}',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, pushkey, app_id)
);

CREATE TABLE IF NOT EXISTS push_rule_overrides (
    user_id     TEXT NOT NULL,
    rule_id     TEXT NOT NULL,
    enabled     BOOLEAN NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, rule_id)
);
