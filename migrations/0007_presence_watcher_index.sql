-- Supports the batched presence watcher lookup used by /sync.
CREATE INDEX IF NOT EXISTS idx_room_members_presence_watchers
    ON room_members (room_id, membership, user_id);