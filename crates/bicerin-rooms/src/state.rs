use crate::service::RoomService;
use bicerin_error::BicerinResult;
use bicerin_storage::rooms::*;

impl RoomService {
    pub async fn get_state(&self, room_id: &str) -> BicerinResult<Vec<RoomStateRecord>> {
        bicerin_storage::rooms::get_full_room_state(&self.store, room_id)
            .await
            .map_err(|_| bicerin_error::BicerinError::NotFound)
    }

    pub async fn get_state_event(
        &self,
        room_id: &str,
        event_type: &str,
        state_key: &str,
    ) -> BicerinResult<RoomStateRecord> {
        bicerin_storage::rooms::get_room_state(&self.store, room_id, event_type, state_key)
            .await
            .map_err(|_| bicerin_error::BicerinError::NotFound)
    }
}
