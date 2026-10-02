//! Server signing-key identity for federation/tooling interop (designplan.txt `#87`).
//!
//! Bicerin is single-tenant/unfederated today, but still generates and
//! persists a standard Matrix Ed25519 server signing key (Synapse-compatible
//! on-disk format: `ed25519 <key_id_suffix> <base64_unpadded_seed>`) so that
//! `GET /_matrix/key/v2/server` works for any tooling that expects it, and so
//! a future federation milestone has a stable identity to build on.

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use rand::RngCore;
use std::path::Path;

pub struct ServerSigningKey {
    pub key_id: String,
    signing_key: SigningKey,
}

impl ServerSigningKey {
    pub fn load_or_generate(path: &str) -> std::io::Result<Self> {
        if let Ok(contents) = std::fs::read_to_string(path) {
            if let Some(key) = Self::parse(&contents) {
                return Ok(key);
            }
        }

        let mut seed = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        let signing_key = SigningKey::from_bytes(&seed);
        let suffix = generate_key_id_suffix();
        let contents = format!("ed25519 {} {}\n", suffix, STANDARD_NO_PAD.encode(seed));

        if let Some(parent) = Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, contents)?;

        Ok(Self {
            key_id: format!("ed25519:{}", suffix),
            signing_key,
        })
    }

    fn parse(contents: &str) -> Option<Self> {
        let line = contents.lines().find(|line| line.starts_with("ed25519 "))?;
        let mut parts = line.split_whitespace();
        parts.next()?;
        let suffix = parts.next()?;
        let seed_b64 = parts.next()?;
        let seed_bytes = STANDARD_NO_PAD.decode(seed_b64).ok()?;
        let seed: [u8; 32] = seed_bytes.try_into().ok()?;
        Some(Self {
            key_id: format!("ed25519:{}", suffix),
            signing_key: SigningKey::from_bytes(&seed),
        })
    }

    pub fn verify_key_base64(&self) -> String {
        STANDARD_NO_PAD.encode(self.signing_key.verifying_key().to_bytes())
    }

    pub fn sign_base64(&self, payload: &[u8]) -> String {
        STANDARD_NO_PAD.encode(self.signing_key.sign(payload).to_bytes())
    }
}

fn generate_key_id_suffix() -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..8)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

pub async fn get_server_key(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
) -> axum::Json<serde_json::Value> {
    let valid_until_ts = chrono::Utc::now().timestamp_millis() + 24 * 60 * 60 * 1000;
    let mut body = serde_json::json!({
        "server_name": state.server_name,
        "verify_keys": {
            state.signing_key.key_id.clone(): { "key": state.signing_key.verify_key_base64() }
        },
        "old_verify_keys": {},
        "valid_until_ts": valid_until_ts,
    });
    let payload = serde_json::to_vec(&body).unwrap_or_default();
    let signature = state.signing_key.sign_base64(&payload);
    body["signatures"] = serde_json::json!({
        state.server_name.clone(): { state.signing_key.key_id.clone(): signature }
    });
    axum::Json(body)
}
