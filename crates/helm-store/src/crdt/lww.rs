//! LWW-Register — Last-Writer-Wins Register CRDT.
//!
//! Stores a single value with a timestamp. On merge, the value with
//! the higher timestamp wins. Ties broken by lexicographic value comparison.

use serde::{Serialize, Deserialize};

use super::gcounter::Crdt;

/// Last-Writer-Wins Register.
///
/// Conflict resolution: highest timestamp wins.
/// If timestamps are equal, lexicographically greater value wins (deterministic).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LwwRegister {
    value: Vec<u8>,
    timestamp_ms: u64,
}

impl LwwRegister {
    /// Create a new register with initial value and timestamp.
    pub fn new(value: &[u8], timestamp_ms: u64) -> Self {
        Self {
            value: value.to_vec(),
            timestamp_ms,
        }
    }

    /// Set the value if the timestamp is newer (or equal + greater value).
    pub fn set(&mut self, value: &[u8], timestamp_ms: u64) -> bool {
        if timestamp_ms > self.timestamp_ms
            || (timestamp_ms == self.timestamp_ms && value > self.value.as_slice())
        {
            self.value = value.to_vec();
            self.timestamp_ms = timestamp_ms;
            true
        } else {
            false
        }
    }

    /// Get the current value.
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// Get the current timestamp.
    pub fn timestamp(&self) -> u64 {
        self.timestamp_ms
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("LwwRegister serialization cannot fail")
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

impl Crdt for LwwRegister {
    fn merge(&mut self, other: &Self) {
        self.set(&other.value, other.timestamp_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_newer_wins() {
        let mut r = LwwRegister::new(b"old", 100);
        assert!(r.set(b"new", 200));
        assert_eq!(r.value(), b"new");
        assert_eq!(r.timestamp(), 200);
    }

    #[test]
    fn set_older_rejected() {
        let mut r = LwwRegister::new(b"current", 200);
        assert!(!r.set(b"stale", 100));
        assert_eq!(r.value(), b"current");
    }

    #[test]
    fn merge_newer_wins() {
        let mut a = LwwRegister::new(b"a-value", 100);
        let b = LwwRegister::new(b"b-value", 200);

        a.merge(&b);
        assert_eq!(a.value(), b"b-value");
    }

    #[test]
    fn merge_older_ignored() {
        let mut a = LwwRegister::new(b"a-value", 200);
        let b = LwwRegister::new(b"b-value", 100);

        a.merge(&b);
        assert_eq!(a.value(), b"a-value");
    }

    #[test]
    fn tie_breaks_by_value() {
        let mut a = LwwRegister::new(b"aaa", 100);
        let b = LwwRegister::new(b"zzz", 100);

        a.merge(&b);
        assert_eq!(a.value(), b"zzz"); // zzz > aaa lexicographically
    }

    #[test]
    fn serialization_roundtrip() {
        let r = LwwRegister::new(b"test-value", 42000);
        let bytes = r.to_bytes();
        let decoded = LwwRegister::from_bytes(&bytes).unwrap();
        assert_eq!(r, decoded);
    }

    #[test]
    fn merge_is_commutative() {
        let a = LwwRegister::new(b"val-a", 100);
        let b = LwwRegister::new(b"val-b", 200);

        let mut ab = a.clone();
        ab.merge(&b);

        let mut ba = b.clone();
        ba.merge(&a);

        assert_eq!(ab.value(), ba.value());
    }

    #[test]
    fn merge_is_idempotent() {
        let a = LwwRegister::new(b"same", 100);
        let mut r = a.clone();
        r.merge(&a);
        r.merge(&a);
        assert_eq!(r, a);
    }
}
