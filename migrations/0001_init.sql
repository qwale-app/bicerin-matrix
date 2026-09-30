-- Bicerin Matrix initial schema
-- Single-tenant, unfederated Matrix homeserver core tables.

CREATE SEQUENCE IF NOT EXISTS event_stream_seq START 1;

-- Identity ------------------------------------------------------------

CREATE TABLE IF NOT EXISTS users (
    user_id         TEXT PRIMARY KEY,
    localpart       TEXT NOT NULL,
    password_hash   TEXT,
    display_name    TEXT,
    avatar_url      TEXT,
    is_guest        BOOLEAN NOT NULL DEFAULT FALSE,
    is_deactivated  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS devices (
    device_id       TEXT NOT NULL,
    user_id         TEXT NOT NULL REFERENCES users(user_id),
    display_name    TEXT,
    last_seen_ip    TEXT,
    last_seen_ts    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, device_id)
);

CREATE TABLE IF NOT EXISTS access_tokens (
    token_hash      TEXT PRIMARY KEY,
    user_id         TEXT NOT NULL REFERENCES users(user_id),
    device_id       TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at      TIMESTAMPTZ,
    last_used_at    TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_access_tokens_user ON access_tokens (user_id, device_id);

-- Rooms -----------------------------------------------------------------

CREATE TABLE IF NOT EXISTS rooms (
    room_id         TEXT PRIMARY KEY,
    creator         TEXT NOT NULL,
    room_version    TEXT NOT NULL,
    is_encrypted    BOOLEAN NOT NULL DEFAULT FALSE,
    is_direct       BOOLEAN NOT NULL DEFAULT FALSE,
    name            TEXT,
    topic           TEXT,
    canonical_alias TEXT,
    creation_ts     BIGINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS room_members (
    room_id         TEXT NOT NULL REFERENCES rooms(room_id),
    user_id         TEXT NOT NULL,
    membership      TEXT NOT NULL,
    display_name    TEXT,
    avatar_url      TEXT,
    sender          TEXT NOT NULL,
    event_id        TEXT NOT NULL,
    stream_id       BIGINT NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (room_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_room_members_user ON room_members (user_id, membership);

CREATE TABLE IF NOT EXISTS room_state (
    room_id         TEXT NOT NULL REFERENCES rooms(room_id),
    event_type      TEXT NOT NULL,
    state_key       TEXT NOT NULL,
    event_id        TEXT NOT NULL,
    content         JSONB NOT NULL,
    sender          TEXT NOT NULL,
    stream_id       BIGINT NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (room_id, event_type, state_key)
);

-- Events ------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS events (
    event_id            TEXT PRIMARY KEY,
    room_id             TEXT NOT NULL REFERENCES rooms(room_id),
    sender              TEXT NOT NULL,
    stream_id           BIGINT NOT NULL,
    origin_server_ts    BIGINT NOT NULL,
    event_type          TEXT NOT NULL,
    state_key           TEXT,
    room_version        TEXT NOT NULL,
    content             JSONB NOT NULL,
    unsigned            JSONB,
    redacts             TEXT,
    depth               BIGINT NOT NULL DEFAULT 0,
    auth_events_json     JSONB NOT NULL DEFAULT '[]',
    prev_events_json     JSONB NOT NULL DEFAULT '[]',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_events_room_stream ON events (room_id, stream_id);
CREATE INDEX IF NOT EXISTS idx_events_room_type_stream ON events (room_id, event_type, stream_id);
CREATE INDEX IF NOT EXISTS idx_events_stream ON events (stream_id);
CREATE INDEX IF NOT EXISTS idx_events_sender_stream ON events (sender, stream_id);

CREATE TABLE IF NOT EXISTS event_relations (
    room_id             TEXT NOT NULL,
    parent_event_id     TEXT NOT NULL,
    child_event_id      TEXT NOT NULL,
    rel_type            TEXT NOT NULL,
    PRIMARY KEY (parent_event_id, child_event_id, rel_type)
);
CREATE INDEX IF NOT EXISTS idx_event_relations_parent ON event_relations (parent_event_id);

-- Sync ----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS user_room_cursors (
    user_id         TEXT NOT NULL,
    room_id         TEXT NOT NULL,
    last_stream_id  BIGINT NOT NULL,
    PRIMARY KEY (user_id, room_id)
);

-- Application services --------------------------------------------------------

CREATE TABLE IF NOT EXISTS appservices (
    id                  TEXT PRIMARY KEY,
    url                 TEXT NOT NULL,
    as_token            TEXT NOT NULL UNIQUE,
    hs_token            TEXT NOT NULL,
    sender_localpart    TEXT NOT NULL,
    namespaces          JSONB NOT NULL DEFAULT '{}',
    rate_limited        BOOLEAN NOT NULL DEFAULT TRUE,
    protocols           JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS appservice_transactions (
    transaction_id      TEXT NOT NULL,
    appservice_id       TEXT NOT NULL REFERENCES appservices(id),
    first_stream_id     BIGINT NOT NULL,
    last_stream_id      BIGINT NOT NULL,
    payload             JSONB NOT NULL,
    attempts            INT NOT NULL DEFAULT 0,
    next_retry_at       TIMESTAMPTZ,
    delivered_at        TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (appservice_id, transaction_id)
);
CREATE INDEX IF NOT EXISTS idx_as_txn_pending ON appservice_transactions (appservice_id, delivered_at, next_retry_at);

-- Crypto ------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS device_keys (
    user_id     TEXT NOT NULL,
    device_id   TEXT NOT NULL,
    key_json    JSONB NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, device_id)
);

CREATE TABLE IF NOT EXISTS one_time_keys (
    user_id     TEXT NOT NULL,
    device_id   TEXT NOT NULL,
    key_id      TEXT NOT NULL,
    key_json    JSONB NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, device_id, key_id)
);

CREATE TABLE IF NOT EXISTS fallback_keys (
    user_id     TEXT NOT NULL,
    device_id   TEXT NOT NULL,
    algorithm   TEXT NOT NULL,
    key_json    JSONB NOT NULL,
    used        BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, device_id, algorithm)
);

-- Transaction idempotency ---------------------------------------------------------

CREATE TABLE IF NOT EXISTS transactions (
    user_id     TEXT NOT NULL,
    device_id   TEXT NOT NULL,
    txn_id      TEXT NOT NULL,
    endpoint    TEXT NOT NULL,
    result      JSONB NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, device_id, txn_id, endpoint)
);

-- Media ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS media (
    media_id        TEXT NOT NULL,
    server_name     TEXT NOT NULL,
    uploader        TEXT,
    mime_type       TEXT NOT NULL,
    size_bytes      BIGINT NOT NULL,
    sha256          TEXT NOT NULL,
    storage_key     TEXT NOT NULL,
    upload_name     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (server_name, media_id)
);
