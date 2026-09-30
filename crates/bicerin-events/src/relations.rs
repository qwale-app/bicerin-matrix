use bicerin_storage::events::{EventRecord, EventRelationRecord};

pub async fn index_relations(store: &bicerin_storage::Store, event: &EventRecord) {
    let relates_to = match event.content.get("m.relates_to") {
        Some(r) => r,
        None => return,
    };

    let rel_type = match relates_to.get("rel_type").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return,
    };

    let parent_event_id = match relates_to.get("event_id").and_then(|v| v.as_str()) {
        Some(e) => e.to_string(),
        None => return,
    };

    let rel = EventRelationRecord {
        room_id: event.room_id.clone(),
        parent_event_id,
        child_event_id: event.event_id.clone(),
        rel_type,
    };

    if let Err(e) = bicerin_storage::events::insert_event_relation(store, &rel).await {
        tracing::warn!(error = %e, "failed to index event relation");
    }
}
