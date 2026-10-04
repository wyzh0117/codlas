//! Identifiers, clocks and content hashing shared by every layer.

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn new_id() -> String {
    Uuid::new_v4().simple().to_string()
}

pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// Deterministic hash over whitespace-normalised content.
/// Used for dedup level L2 (V1_DEVELOPMENT.md §52) and evidence identity.
pub fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalize_for_hash(text).as_bytes());
    hex::encode(hasher.finalize())
}

pub fn normalize_for_hash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_whitespace_independent() {
        assert_eq!(content_hash("a  b\tc"), content_hash("a b c"));
        assert_ne!(content_hash("a b"), content_hash("a b c"));
    }

    #[test]
    fn ids_are_unique() {
        assert_ne!(new_id(), new_id());
    }
}
