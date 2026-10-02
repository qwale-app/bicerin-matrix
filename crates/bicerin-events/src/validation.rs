use bicerin_error::{BicerinError, BicerinResult};
use serde_json::Value;

pub fn validate_event_type(event_type: &str) -> BicerinResult<()> {
    if event_type.is_empty() {
        return Err(BicerinError::BadRequest(
            "event_type cannot be empty".to_string(),
        ));
    }
    if event_type.len() > 255 {
        return Err(BicerinError::BadRequest("event_type too long".to_string()));
    }
    Ok(())
}

pub fn validate_event_content(content: &Value) -> BicerinResult<()> {
    if !content.is_object() {
        return Err(BicerinError::BadRequest(
            "event content must be a JSON object".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_room_id(room_id: &str) -> BicerinResult<()> {
    if !room_id.starts_with('!') || !room_id.contains(':') {
        return Err(BicerinError::BadRequest(
            "invalid room_id format".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_user_id(user_id: &str) -> BicerinResult<()> {
    if !user_id.starts_with('@') || !user_id.contains(':') {
        return Err(BicerinError::BadRequest(
            "invalid user_id format".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_state_key_sender(sender: &str, state_key: &str) -> BicerinResult<()> {
    if state_key.starts_with('@') && state_key != sender {
        return Err(BicerinError::Forbidden);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        validate_event_content, validate_event_type, validate_room_id,
        validate_state_key_sender, validate_user_id,
    };
    use bicerin_error::BicerinError;
    use serde_json::json;

    #[test]
    fn event_types_must_be_nonempty_and_within_the_matrix_limit() {
        assert!(validate_event_type("m.room.message").is_ok());
        assert!(matches!(
            validate_event_type(""),
            Err(BicerinError::BadRequest(_))
        ));
        assert!(matches!(
            validate_event_type(&"x".repeat(256)),
            Err(BicerinError::BadRequest(_))
        ));
    }

    #[test]
    fn event_content_must_be_a_json_object() {
        assert!(validate_event_content(&json!({"body": "hello"})).is_ok());
        assert!(matches!(
            validate_event_content(&json!(["not", "an", "object"])),
            Err(BicerinError::BadRequest(_))
        ));
    }

    #[test]
    fn room_and_user_ids_require_their_matrix_sigils_and_server_separator() {
        assert!(validate_room_id("!room:example.org").is_ok());
        assert!(validate_user_id("@alice:example.org").is_ok());
        assert!(validate_room_id("room:example.org").is_err());
        assert!(validate_user_id("alice:example.org").is_err());
        assert!(validate_room_id("!room").is_err());
        assert!(validate_user_id("@alice").is_err());
    }

    #[test]
    fn user_addressed_state_keys_can_only_name_the_sender() {
        assert!(validate_state_key_sender("@alice:example.org", "@alice:example.org").is_ok());
        assert!(validate_state_key_sender("@alice:example.org", "group").is_ok());
        assert!(matches!(
            validate_state_key_sender("@alice:example.org", "@bob:example.org"),
            Err(BicerinError::Forbidden)
        ));
    }
}
