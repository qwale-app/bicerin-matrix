use bicerin_error::{BicerinError, BicerinResult};

/// Turns a login/register `username` field into a fully-qualified Matrix user ID.
/// Accepts either a bare localpart or an already-qualified `@user:server` ID.
pub fn normalize_user_id(username: &str, server_name: &str) -> String {
    if username.starts_with('@') {
        username.to_string()
    } else {
        format!("@{}:{}", username, server_name)
    }
}

/// Validates a localpart against the (simplified) Matrix user ID grammar:
/// lowercase ascii letters, digits, and `._=-/`.
pub fn validate_localpart(localpart: &str) -> BicerinResult<()> {
    if localpart.is_empty() || localpart.len() > 255 {
        return Err(BicerinError::MatrixError {
            errcode: "M_INVALID_USERNAME".to_string(),
            error: "Localpart has invalid length".to_string(),
        });
    }
    let valid = localpart.bytes().all(|b| {
        b.is_ascii_lowercase()
            || b.is_ascii_digit()
            || matches!(b, b'.' | b'_' | b'=' | b'-' | b'/')
    });
    if !valid {
        return Err(BicerinError::MatrixError {
            errcode: "M_INVALID_USERNAME".to_string(),
            error: "Localpart contains invalid characters".to_string(),
        });
    }
    Ok(())
}

pub fn generate_localpart() -> String {
    format!("guest{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
}

#[cfg(test)]
mod tests {
    use super::{generate_localpart, normalize_user_id, validate_localpart};

    #[test]
    fn username_normalization_preserves_qualified_ids() {
        assert_eq!(
            normalize_user_id("alice", "example.org"),
            "@alice:example.org"
        );
        assert_eq!(
            normalize_user_id("@bob:remote.org", "example.org"),
            "@bob:remote.org"
        );
    }

    #[test]
    fn localpart_validation_accepts_matrix_characters_and_rejects_invalid_values() {
        assert!(validate_localpart("alice_1/test-name").is_ok());
        assert!(validate_localpart("").is_err());
        assert!(validate_localpart("Alice").is_err());
        assert!(validate_localpart("has space").is_err());
        assert!(validate_localpart(&"a".repeat(256)).is_err());
    }

    #[test]
    fn generated_guest_localparts_are_valid_and_have_expected_shape() {
        let localpart = generate_localpart();

        assert_eq!(localpart.len(), 17);
        assert!(localpart.starts_with("guest"));
        assert!(validate_localpart(&localpart).is_ok());
    }
}
