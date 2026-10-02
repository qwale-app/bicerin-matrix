-- Outbound push-notification delivery queue (Push Gateway API payloads).

CREATE TABLE IF NOT EXISTS pending_pushes (
    id              TEXT PRIMARY KEY,
    user_id         TEXT NOT NULL,
    pushkey         TEXT NOT NULL,
    app_id          TEXT NOT NULL,
    url             TEXT NOT NULL,
    payload         JSONB NOT NULL,
    attempts        INT NOT NULL DEFAULT 0,
    next_retry_at   TIMESTAMPTZ,
    delivered_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_pending_pushes_pending ON pending_pushes (delivered_at, next_retry_at);
