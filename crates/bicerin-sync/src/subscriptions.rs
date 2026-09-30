use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct RoomUpdate {
    pub room_id: String,
    pub stream_id: i64,
}

pub struct SyncBus {
    sender: broadcast::Sender<RoomUpdate>,
}

impl SyncBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn notify(&self, room_id: String, stream_id: i64) {
        let _ = self.sender.send(RoomUpdate { room_id, stream_id });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RoomUpdate> {
        self.sender.subscribe()
    }
}
