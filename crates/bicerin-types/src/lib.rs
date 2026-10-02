pub mod ids {
    // Re-export Ruma's owned ID types for convenience. The borrowed `RoomId`/`EventId`/etc.
    // types are unsized (like `str`) and don't implement Clone/Serialize on their own, so we
    // use the owned variants everywhere we need to store an ID.
    pub use ruma::{DeviceId, EventId, MxcUri, RoomId, UserId};
    pub use ruma::{OwnedDeviceId, OwnedEventId, OwnedMxcUri, OwnedRoomId, OwnedUserId};
}

pub mod events {
    use serde::{Deserialize, Serialize};

    /// A persisted event ready to be delivered to sync subscribers.
    ///
    /// Bicerin stores/transmits events as plain JSON (see designplan.txt #94's
    /// "wire-compatible, semantically compatible, internally different" philosophy)
    /// rather than using Ruma's strongly-typed event enums, which keeps the hot
    /// path free of per-event-type (de)serialization.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EventEnvelope {
        pub stream_id: crate::stream::StreamPosition,
        pub event: serde_json::Value,
        pub room_id: crate::ids::OwnedRoomId,
    }
}

pub mod auth {
    use sha2::{Digest, Sha256};
    use std::fmt;

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct AccessTokenHash(pub String);

    impl fmt::Display for AccessTokenHash {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    pub fn hash_access_token(token: &str) -> AccessTokenHash {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let result = hasher.finalize();
        AccessTokenHash(hex::encode(result))
    }
}

pub mod stream {
    use std::fmt;
    use std::ops::{Add, Sub};

    #[derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        serde::Serialize,
        serde::Deserialize,
    )]
    pub struct StreamPosition(pub i64);

    impl fmt::Display for StreamPosition {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl Add<i64> for StreamPosition {
        type Output = Self;

        fn add(self, other: i64) -> Self {
            StreamPosition(self.0 + other)
        }
    }

    impl Sub<i64> for StreamPosition {
        type Output = Self;

        fn sub(self, other: i64) -> Self {
            StreamPosition(self.0 - other)
        }
    }

    impl Sub<StreamPosition> for StreamPosition {
        type Output = i64;

        fn sub(self, other: StreamPosition) -> i64 {
            self.0 - other.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{auth::hash_access_token, stream::StreamPosition};

    #[test]
    fn access_token_hash_uses_sha256_hex() {
        assert_eq!(
            hash_access_token("abc").to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn stream_positions_support_arithmetic_and_ordering() {
        let position = StreamPosition(10);

        assert_eq!((position + 3).0, 13);
        assert_eq!((position - 4).0, 6);
        assert_eq!(position - StreamPosition(7), 3);
        assert!(StreamPosition(7) < position);
    }
}
