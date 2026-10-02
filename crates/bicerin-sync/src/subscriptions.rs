use tokio::sync::broadcast;
use std::{collections::HashMap, sync::RwLock, time::{Duration, Instant}};

#[derive(Debug, Clone)]
pub struct RoomUpdate {
    pub room_id: String,
    pub stream_id: i64,
    pub user_id: Option<String>,
}

pub struct SyncBus {
    sender: broadcast::Sender<RoomUpdate>,
    typing: RwLock<HashMap<String, TypingRoom>>,
}

#[derive(Default)]
struct TypingRoom {
    version: i64,
    users: HashMap<String, (Instant, i64)>,
}

impl SyncBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender, typing: RwLock::new(HashMap::new()) }
    }

    pub fn notify(&self, room_id: String, stream_id: i64) {
        let _ = self.sender.send(RoomUpdate {
            room_id,
            stream_id,
            user_id: None,
        });
    }

    pub fn notify_user(&self, user_id: String, stream_id: i64) {
        let _ = self.sender.send(RoomUpdate {
            room_id: String::new(),
            stream_id,
            user_id: Some(user_id),
        });
    }

    pub fn notify_all(&self, stream_id: i64) {
        let _ = self.sender.send(RoomUpdate {
            room_id: String::new(),
            stream_id,
            user_id: None,
        });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RoomUpdate> {
        self.sender.subscribe()
    }

    pub fn set_typing(
        &self,
        room_id: &str,
        user_id: &str,
        stream_id: i64,
        typing: bool,
        timeout: Duration,
    ) {
        let mut rooms = self.typing.write().expect("typing state lock poisoned");
        let room = rooms.entry(room_id.to_string()).or_default();
        room.version = stream_id;
        if typing {
            room.users.insert(user_id.to_string(), (Instant::now() + timeout, stream_id));
        } else {
            room.users.remove(user_id);
        }
    }

    pub fn stop_typing_if_version(
        &self,
        room_id: &str,
        user_id: &str,
        expected_version: i64,
        stream_id: i64,
    ) -> bool {
        let mut rooms = self.typing.write().expect("typing state lock poisoned");
        let Some(room) = rooms.get_mut(room_id) else { return false; };
        if !room.users.get(user_id).is_some_and(|(_, version)| *version == expected_version) {
            return false;
        }
        room.users.remove(user_id);
        room.version = stream_id;
        true
    }

    pub fn typing_snapshot(
        &self,
        room_id: &str,
        since_stream_id: i64,
        initial_sync: bool,
    ) -> Option<Vec<String>> {
        let rooms = self.typing.read().expect("typing state lock poisoned");
        let room = rooms.get(room_id)?;
        let active = room.users.iter()
            .filter(|(_, (expires, _))| *expires > Instant::now())
            .map(|(user_id, _)| user_id.clone())
            .collect::<Vec<_>>();
        if room.version > since_stream_id || (initial_sync && !active.is_empty()) {
            Some(active)
        } else {
            None
        }
    }

    pub fn has_typing_updates(&self, room_ids: &[String], since_stream_id: i64) -> bool {
        let rooms = self.typing.read().expect("typing state lock poisoned");
        room_ids.iter().any(|room_id| {
            rooms.get(room_id).is_some_and(|room| room.version > since_stream_id)
        })
    }
}
