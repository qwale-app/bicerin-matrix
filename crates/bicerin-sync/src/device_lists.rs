use bicerin_error::{BicerinError, BicerinResult};
use bicerin_storage::Store;
use std::collections::HashSet;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct DeviceListUpdates {
    pub changed: Vec<String>,
    pub left: Vec<String>,
}

/// Computes the device-list invalidations visible to a user over a sync
/// token interval. Matrix clients need a fresh `/keys/query` both when
/// another user changes keys and when an encrypted-room membership change
/// starts or ends sharing.
pub async fn get_device_list_updates(
    store: &Store,
    user_id: &str,
    from: i64,
    to: i64,
) -> BicerinResult<DeviceListUpdates> {
    let changes = bicerin_storage::cross_signing::get_device_changes(store, from, to)
        .await
        .map_err(internal)?;
    let joined_rooms = bicerin_storage::rooms::get_joined_rooms(store, user_id)
        .await
        .map_err(internal)?;
    let left_rooms = bicerin_storage::rooms::get_room_members_by_user_membership(store, user_id, "leave")
        .await
        .map_err(internal)?;

    let mut shared_users = HashSet::new();
    let mut changed_candidates = HashSet::new();
    let mut left_candidates = HashSet::new();

    for room_id in &joined_rooms {
        let room = bicerin_storage::rooms::get_room(store, room_id)
            .await
            .map_err(internal)?;
        if !room.is_encrypted {
            continue;
        }

        let members = bicerin_storage::rooms::get_room_members_by_membership(store, room_id, "join")
            .await
            .map_err(internal)?;
        for member in &members {
            if member.user_id != user_id {
                shared_users.insert(member.user_id.clone());
            }
        }

        // A caller who just joined an encrypted room needs the existing
        // members' keys even though those users did not themselves change.
        let own_membership = bicerin_storage::rooms::get_room_member(store, room_id, user_id)
            .await
            .map_err(internal)?;
        if own_membership.stream_id > from && own_membership.stream_id <= to {
            changed_candidates.extend(members.iter().filter(|member| member.user_id != user_id).map(|member| member.user_id.clone()));
        }

        let mut cursor = to.saturating_add(1);
        loop {
            let events = bicerin_storage::events::get_events_in_room(store, room_id, cursor, 500, "b")
                .await
                .map_err(internal)?;
            if events.is_empty() {
                break;
            }
            let earliest = events.last().map(|event| event.stream_id).unwrap_or(cursor);
            for event in events.iter().filter(|event| event.stream_id > from && event.stream_id <= to) {
                if event.event_type == "m.room.encryption" {
                    changed_candidates.extend(members.iter().filter(|member| member.user_id != user_id).map(|member| member.user_id.clone()));
                } else if event.event_type == "m.room.member" {
                    if let Some(target) = event.state_key.as_deref().filter(|target| *target != user_id) {
                        match event.content.get("membership").and_then(serde_json::Value::as_str) {
                            Some("join") => { changed_candidates.insert(target.to_string()); }
                            Some("leave" | "ban") => { left_candidates.insert(target.to_string()); }
                            _ => {}
                        }
                    }
                }
            }
            if events.len() < 500 || earliest <= from {
                break;
            }
            cursor = earliest;
        }
    }

    // When the caller leaves an encrypted room, all remaining members cease
    // sharing that room. They belong in `left` only if no other encrypted
    // joined room is still shared with them.
    for membership in left_rooms.into_iter().filter(|membership| membership.stream_id > from && membership.stream_id <= to) {
        let room = bicerin_storage::rooms::get_room(store, &membership.room_id)
            .await
            .map_err(internal)?;
        if room.is_encrypted {
            let members = bicerin_storage::rooms::get_room_members_by_membership(store, &membership.room_id, "join")
                .await
                .map_err(internal)?;
            left_candidates.extend(members.into_iter().filter(|member| member.user_id != user_id).map(|member| member.user_id));
        }
    }

    for change in changes {
        if change.change_type == "changed" && shared_users.contains(&change.user_id) {
            changed_candidates.insert(change.user_id);
        }
    }

    Ok(finalize_device_list_updates(changed_candidates, left_candidates, shared_users, user_id))
}

fn finalize_device_list_updates(
    changed: HashSet<String>,
    left: HashSet<String>,
    shared: HashSet<String>,
    user_id: &str,
) -> DeviceListUpdates {
    let mut changed = changed.into_iter().filter(|id| id != user_id && shared.contains(id)).collect::<Vec<_>>();
    let mut left = left.into_iter().filter(|id| id != user_id && !shared.contains(id)).collect::<Vec<_>>();
    changed.sort();
    left.sort();
    DeviceListUpdates { changed, left }
}

fn internal(error: impl std::fmt::Display) -> BicerinError {
    BicerinError::Internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{finalize_device_list_updates, DeviceListUpdates};
    use std::collections::HashSet;

    #[test]
    fn device_lists_are_deduplicated_sorted_and_only_report_current_share_state() {
        let updates = finalize_device_list_updates(
            HashSet::from(["@bob:example.org".into(), "@alice:example.org".into()]),
            HashSet::from(["@bob:example.org".into(), "@carol:example.org".into(), "@me:example.org".into()]),
            HashSet::from(["@bob:example.org".into()]),
            "@me:example.org",
        );
        assert_eq!(updates, DeviceListUpdates {
            changed: vec!["@bob:example.org".to_string()],
            left: vec!["@carol:example.org".to_string()],
        });
    }
}
