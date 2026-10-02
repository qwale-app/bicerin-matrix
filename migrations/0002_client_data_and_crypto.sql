-- Account data, send-to-device messages, and cross-signing metadata.
ALTER TABLE fallback_keys ADD COLUMN IF NOT EXISTS key_id TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS account_data (
    user_id TEXT NOT NULL,
    room_id TEXT NOT NULL DEFAULT '',
    event_type TEXT NOT NULL,
    content JSONB NOT NULL,
    stream_id BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, room_id, event_type)
);
CREATE INDEX IF NOT EXISTS idx_account_data_user_stream ON account_data(user_id, stream_id);

CREATE TABLE IF NOT EXISTS to_device_messages (
    message_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    sender TEXT NOT NULL,
    event_type TEXT NOT NULL,
    content JSONB NOT NULL,
    stream_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_to_device_recipient ON to_device_messages(user_id, device_id, stream_id);

CREATE TABLE IF NOT EXISTS cross_signing_keys (
    user_id TEXT NOT NULL,
    key_type TEXT NOT NULL,
    key_json JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, key_type)
);

CREATE TABLE IF NOT EXISTS device_key_changes (
    user_id TEXT NOT NULL,
    stream_id BIGINT NOT NULL,
    change_type TEXT NOT NULL DEFAULT 'changed',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, stream_id, change_type)
);
CREATE INDEX IF NOT EXISTS idx_device_key_changes_stream ON device_key_changes(stream_id, user_id);

CREATE TABLE IF NOT EXISTS room_key_backup_versions (
    user_id TEXT NOT NULL,
    version TEXT NOT NULL,
    algorithm TEXT NOT NULL,
    auth_data JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, version)
);

CREATE TABLE IF NOT EXISTS room_key_backup_sessions (
    user_id TEXT NOT NULL,
    version TEXT NOT NULL,
    room_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    data JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, version, room_id, session_id)
);
CREATE INDEX IF NOT EXISTS idx_room_key_backup_room ON room_key_backup_sessions(user_id, version, room_id);
