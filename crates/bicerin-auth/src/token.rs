use base64::Engine;
use rand::Rng;

/// Generate a cryptographically random access token.
/// Returns a 32-byte random value, base64url-encoded.
pub fn generate_access_token() -> String {
    let bytes: Vec<u8> = (0..32).map(|_| rand::thread_rng().gen()).collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes)
}

/// Generate a random device ID if not provided by client.
pub fn generate_device_id() -> String {
    let bytes: Vec<u8> = (0..8).map(|_| rand::thread_rng().gen()).collect();
    hex::encode(&bytes).to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::{generate_access_token, generate_device_id};
    use base64::Engine as _;

    #[test]
    fn access_tokens_are_32_bytes_encoded_as_unpadded_base64url() {
        let token = generate_access_token();
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&token)
            .expect("valid base64url token");

        assert_eq!(decoded.len(), 32);
        assert!(!token.contains('='));
    }

    #[test]
    fn generated_device_ids_are_eight_bytes_of_uppercase_hex() {
        let device_id = generate_device_id();

        assert_eq!(device_id.len(), 16);
        assert!(device_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)));
    }
}
