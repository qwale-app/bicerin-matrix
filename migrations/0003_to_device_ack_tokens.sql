-- Track the exact /sync response that delivered each to-device message.
-- Sync tokens are opaque; acknowledging a token must not delete messages
-- which were still beyond the per-response delivery limit.
ALTER TABLE to_device_messages
    ADD COLUMN IF NOT EXISTS delivered_sync_token TEXT;
CREATE INDEX IF NOT EXISTS idx_to_device_ack
    ON to_device_messages(user_id, device_id, delivered_sync_token);
