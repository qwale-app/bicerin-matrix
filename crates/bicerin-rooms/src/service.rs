pub struct RoomService {
    pub store: bicerin_storage::Store,
}

impl RoomService {
    pub fn new(store: bicerin_storage::Store) -> Self {
        Self { store }
    }
}
