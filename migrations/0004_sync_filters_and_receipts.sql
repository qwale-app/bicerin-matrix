CREATE TABLE IF NOT EXISTS user_filters (
    user_id TEXT NOT NULL,
    filter_id TEXT NOT NULL,
    filter_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, filter_id)
);

CREATE TABLE IF NOT EXISTS room_receipts (
    room_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    receipt_type TEXT NOT NULL,
    thread_id TEXT NOT NULL DEFAULT '',
    event_id TEXT NOT NULL,
    event_stream_id BIGINT NOT NULL,
    stream_id BIGINT NOT NULL,
    timestamp BIGINT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (room_id, user_id, receipt_type, thread_id)
);
CREATE INDEX IF NOT EXISTS idx_room_receipts_stream ON room_receipts(room_id, stream_id);
