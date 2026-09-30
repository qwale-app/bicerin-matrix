use bicerin_error::{BicerinError, BicerinResult};
use serde_json::Value;

pub fn validate_event_type(event_type: &str) -> BicerinResult<()> {
    if event_type.is_empty() {
        return Err(BicerinError::BadRequest("event_type cannot be empty".to_string()));
    }
    if event_type.len() > 255 {
        return Err(BicerinError::BadRequest("event_type too long".to_string()));
    }
    Ok(())
}

pub fn validate_event_content(content: &Value) -> BicerinResult<()> {
    if !content.is_object() {
        return Err(BicerinError::BadRequest("event content must be a JSON object".to_string()));
    }
    Ok(())
}

pub fn validate_room_id(room_id: &str) -> BicerinResult<()> {
    if !room_id.starts_with('!') || !room_id.contains(':') {
        return Err(BicerinError::BadRequest("invalid room_id format".to_string()));
    }
    Ok(())
}

pub fn validate_user_id(user_id: &str) -> BicerinResult<()> {
    if !user_id.starts_with('@') || !user_id.contains(':') {
        return Err(BicerinError::BadRequest("invalid user_id format".to_string()));
    }
    Ok(())
}
