#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct SyncFilter {
    pub room: Option<RoomFilter>,
    pub account_data: Option<EventFilter>,
    pub presence: Option<EventFilter>,
    pub to_device: Option<EventFilter>,
    pub event_fields: Option<Vec<String>>,
    pub event_format: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct RoomFilter {
    pub timeline: Option<EventFilter>,
    pub state: Option<StateFilter>,
    pub ephemeral: Option<EventFilter>,
    pub account_data: Option<EventFilter>,
    pub not_rooms: Option<Vec<String>>,
    pub rooms: Option<Vec<String>>,
    pub include_leave: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct EventFilter {
    pub limit: Option<i64>,
    pub not_types: Option<Vec<String>>,
    pub types: Option<Vec<String>>,
    pub not_senders: Option<Vec<String>>,
    pub senders: Option<Vec<String>>,
    pub contains_url: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct StateFilter {
    pub limit: Option<i64>,
    pub not_types: Option<Vec<String>>,
    pub types: Option<Vec<String>>,
    pub lazy_load_members: Option<bool>,
}
