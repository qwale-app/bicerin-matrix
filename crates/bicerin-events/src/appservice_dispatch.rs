use bicerin_storage::events::EventRecord;
use bicerin_storage::Store;

/// After an event is persisted, best-effort enqueues an application-service
/// transaction for every appservice interested in it: one whose bot/ghost
/// namespace owns the sender, whose room namespace matches the room, or which
/// already has a ghost joined to the room. Failures are logged, not
/// propagated — a slow/broken bridge must never block message sending (see
/// designplan.txt #76-78).
pub async fn dispatch(store: &Store, server_name: &str, event: &EventRecord) {
    let appservices = match bicerin_storage::appservice::list_appservices(store).await {
        Ok(list) if !list.is_empty() => list,
        Ok(_) => return,
        Err(e) => {
            tracing::warn!(error = %e, "failed to list appservices for event dispatch");
            return;
        }
    };

    let members =
        bicerin_storage::rooms::get_room_members_by_membership(store, &event.room_id, "join")
            .await
            .unwrap_or_default();

    for appservice in appservices {
        // Membership events (e.g. a real user inviting a not-yet-joined
        // ghost into a fresh DM) target the ghost via `state_key`, so check
        // that explicitly — the ghost won't show up in `members` (join-only)
        // until it accepts the invite.
        let targets_owned_user = event.event_type == "m.room.member"
            && event.state_key.as_deref().is_some_and(|target| {
                bicerin_storage::appservice::owns_user(&appservice, server_name, target)
            });

        let interested =
            bicerin_storage::appservice::owns_user(&appservice, server_name, &event.sender)
                || bicerin_storage::appservice::namespace_matches(
                    &appservice.namespaces,
                    "rooms",
                    &event.room_id,
                )
                || targets_owned_user
                || members.iter().any(|m| {
                    bicerin_storage::appservice::owns_user(&appservice, server_name, &m.user_id)
                });

        if !interested {
            continue;
        }

        let payload = serde_json::json!({ "events": [event_to_as_json(event)] });
        let record = bicerin_storage::appservice::AppserviceTransactionRecord {
            transaction_id: uuid::Uuid::new_v4().to_string(),
            appservice_id: appservice.id.clone(),
            first_stream_id: event.stream_id,
            last_stream_id: event.stream_id,
            payload,
            attempts: 0,
            next_retry_at: None,
            delivered_at: None,
            created_at: chrono::Utc::now(),
        };

        if let Err(e) =
            bicerin_storage::appservice::create_appservice_transaction(store, &record).await
        {
            tracing::warn!(error = %e, appservice_id = %appservice.id, "failed to enqueue appservice transaction");
        }
    }
}

fn event_to_as_json(event: &EventRecord) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "event_id": event.event_id,
        "room_id": event.room_id,
        "sender": event.sender,
        "type": event.event_type,
        "origin_server_ts": event.origin_server_ts,
        "content": event.content,
    });
    if let Some(state_key) = &event.state_key {
        obj["state_key"] = serde_json::Value::String(state_key.clone());
    }
    obj
}
