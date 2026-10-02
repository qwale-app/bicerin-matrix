//! Integration test exercising the MongoDB storage backend end-to-end
//! against a real (disposable) MongoDB instance.
//!
//! Not run by default (`#[ignore]`) since it needs a live MongoDB to connect
//! to. The CI workflow (`.github/workflows/rust.yml`) spins up a local,
//! ephemeral MongoDB service container for the job and runs
//! `cargo test --workspace -- --include-ignored` so this executes there.
//! To run locally: start MongoDB (e.g. `docker run --rm -p 27017:27017
//! mongo:7`), then `BICERIN_TEST_MONGO_URI=mongodb://127.0.0.1:27017 cargo
//! test -p bicerin-storage -- --ignored`.

use bicerin_storage::{rooms, users, Store};

fn mongo_uri() -> String {
    std::env::var("BICERIN_TEST_MONGO_URI")
        .expect("BICERIN_TEST_MONGO_URI must be set to run this test (see module docs)")
}

#[tokio::test]
#[ignore = "requires a running MongoDB instance; see module docs"]
async fn mongo_backend_round_trips_users_rooms_and_aliases() {
    // A unique per-run database name avoids collisions if the test runs
    // more than once against the same long-lived MongoDB instance.
    let database_name = format!("bicerin_ci_test_{}", uuid::Uuid::new_v4().simple());
    let store = Store::connect_mongo(&mongo_uri(), &database_name)
        .await
        .expect("connect to MongoDB");
    store.init_schema().await.expect("initialize schema/indexes");

    let user = users::UserRecord {
        user_id: "@alice:test.local".to_string(),
        localpart: "alice".to_string(),
        password_hash: Some("hash".to_string()),
        display_name: None,
        avatar_url: None,
        is_guest: false,
        is_deactivated: false,
        created_at: chrono::Utc::now(),
    };
    users::create_user(&store, &user).await.expect("create user");
    let fetched = users::get_user(&store, &user.user_id)
        .await
        .expect("fetch user");
    assert_eq!(fetched.user_id, user.user_id);
    assert_eq!(users::count_users(&store).await.expect("count users"), 1);

    let room = rooms::RoomRecord {
        room_id: "!room:test.local".to_string(),
        creator: user.user_id.clone(),
        room_version: "10".to_string(),
        is_encrypted: false,
        is_direct: false,
        name: Some("Test room".to_string()),
        topic: None,
        canonical_alias: None,
        visibility: "private".to_string(),
        creation_ts: chrono::Utc::now().timestamp_millis(),
        created_at: chrono::Utc::now(),
    };
    rooms::create_room(&store, &room).await.expect("create room");

    rooms::create_alias(&store, "#test:test.local", &room.room_id, &user.user_id)
        .await
        .expect("create alias");
    let resolved = rooms::get_room_id_for_alias(&store, "#test:test.local")
        .await
        .expect("resolve alias");
    assert_eq!(resolved, room.room_id);

    rooms::set_room_visibility(&store, &room.room_id, "public")
        .await
        .expect("set visibility");
    let public_rooms = rooms::list_public_rooms(&store, 10)
        .await
        .expect("list public rooms");
    assert!(public_rooms.iter().any(|r| r.room_id == room.room_id));
    assert_eq!(rooms::count_rooms(&store).await.expect("count rooms"), 1);
}
