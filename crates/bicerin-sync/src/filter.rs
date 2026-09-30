#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct SyncFilter {
    pub room: Option<RoomFilter>,
    pub event_fields: Option<Vec<String>>,
    pub event_format: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct RoomFilter {
    pub timeline: Option<EventFilter>,
    pub state: Option<StateFilter>,
    pub ephemeral: Option<EventFilter>,
    pub not_rooms: Option<Vec<String>>,
    pub rooms: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct EventFilter {
    pub limit: Option<i64>,
    pub not_types: Option<Vec<String>>,
    pub types: Option<Vec<String>>,
    pub not_senders: Option<Vec<String>>,
    pub senders: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct StateFilter {
    pub limit: Option<i64>,
    pub not_types: Option<Vec<String>>,
    pub types: Option<Vec<String>>,
    pub lazy_load_members: Option<bool>,
}
