pub mod appservice;
pub mod client_data;
pub mod cross_signing;
pub mod crypto;
pub mod db;
pub mod events;
pub mod filters;
pub mod media;
pub mod mongo_schema;
pub mod presence;
pub mod push;
pub mod room_keys;
pub mod rooms;
pub mod store;
pub mod sync;
pub mod transactions;
pub mod users;

pub use store::{MongoBackend, Store};

#[cfg(test)]
mod tests {
    use super::events::EventRecord;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    #[test]
    fn event_records_round_trip_through_mongodb_bson_documents() {
        let created_at = Utc
            .timestamp_millis_opt(1_700_000_000_123)
            .single()
            .unwrap();
        let event = EventRecord {
            event_id: "$event:example.org".to_string(),
            room_id: "!room:example.org".to_string(),
            sender: "@alice:example.org".to_string(),
            stream_id: 17,
            origin_server_ts: 1_700_000_000_000,
            event_type: "m.room.message".to_string(),
            state_key: None,
            room_version: "10".to_string(),
            content: json!({"msgtype": "m.text", "body": "hello"}),
            unsigned: Some(json!({"age": 25})),
            redacts: None,
            depth: 4,
            auth_events: vec!["$auth:example.org".to_string()],
            prev_events: vec!["$prev:example.org".to_string()],
            created_at,
        };

        let document = bson::to_document(&event).expect("serialize event to BSON");
        let restored: EventRecord =
            bson::from_document(document).expect("deserialize event from BSON");

        assert_eq!(restored.event_id, event.event_id);
        assert_eq!(restored.room_id, event.room_id);
        assert_eq!(restored.stream_id, event.stream_id);
        assert_eq!(restored.content, event.content);
        assert_eq!(restored.unsigned, event.unsigned);
        assert_eq!(restored.auth_events, event.auth_events);
        assert_eq!(restored.prev_events, event.prev_events);
        assert_eq!(
            restored.created_at.timestamp_millis(),
            event.created_at.timestamp_millis()
        );
    }
}
