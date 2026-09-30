use rand::Rng;
use base64::Engine;

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
