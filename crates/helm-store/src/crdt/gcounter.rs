//! G-Counter — Grow-only counter CRDT.
//!
//! Each node maintains its own counter. The global value is the sum.
//! Merge takes the max per node. Guarantees monotonic growth.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};

/// A merge-able CRDT trait.
pub trait Crdt: Clone {
    /// Merge another replica's state into this one.
    fn merge(&mut self, other: &Self);
}

/// Grow-only counter.
///
/// Each node increments its own slot. Total = sum of all slots.
/// Merge = element-wise max (idempotent, commutative, associative).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GCounter {
    counts: HashMap<String, u64>,
}

impl GCounter {
    pub fn new() -> Self {
        Self {
            counts: HashMap::new(),
        }
    }

    /// Increment this node's counter by 1.
    pub fn increment(&mut self, node_id: &str) {
        *self.counts.entry(node_id.to_string()).or_insert(0) += 1;
    }

    /// Increment by a specific amount.
    pub fn increment_by(&mut self, node_id: &str, amount: u64) {
        *self.counts.entry(node_id.to_string()).or_insert(0) += amount;
    }

    /// Get this node's local count.
    pub fn local_count(&self, node_id: &str) -> u64 {
        self.counts.get(node_id).copied().unwrap_or(0)
    }

    /// Get the total count across all nodes.
    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Number of nodes that have contributed.
    pub fn node_count(&self) -> usize {
        self.counts.len()
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("GCounter serialization cannot fail")
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

impl Default for GCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl Crdt for GCounter {
    fn merge(&mut self, other: &Self) {
        for (node, &count) in &other.counts {
            let entry = self.counts.entry(node.clone()).or_insert(0);
            *entry = (*entry).max(count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_increment() {
        let mut c = GCounter::new();
        c.increment("node-1");
        c.increment("node-1");
        c.increment("node-2");
        assert_eq!(c.value(), 3);
        assert_eq!(c.local_count("node-1"), 2);
        assert_eq!(c.local_count("node-2"), 1);
    }

    #[test]
    fn merge_takes_max() {
        let mut a = GCounter::new();
        a.increment_by("node-1", 5);
        a.increment_by("node-2", 3);

        let mut b = GCounter::new();
        b.increment_by("node-1", 3);
        b.increment_by("node-2", 7);
        b.increment_by("node-3", 2);

        a.merge(&b);
        assert_eq!(a.local_count("node-1"), 5); // max(5, 3)
        assert_eq!(a.local_count("node-2"), 7); // max(3, 7)
        assert_eq!(a.local_count("node-3"), 2); // new from b
        assert_eq!(a.value(), 14);
    }

    #[test]
    fn merge_is_idempotent() {
        let mut a = GCounter::new();
        a.increment_by("n1", 10);

        let b = a.clone();
        a.merge(&b);
        assert_eq!(a.value(), 10);
    }

    #[test]
    fn merge_is_commutative() {
        let mut a = GCounter::new();
        a.increment_by("n1", 5);

        let mut b = GCounter::new();
        b.increment_by("n2", 3);

        let mut ab = a.clone();
        ab.merge(&b);

        let mut ba = b.clone();
        ba.merge(&a);

        assert_eq!(ab.value(), ba.value());
    }

    #[test]
    fn serialization_roundtrip() {
        let mut c = GCounter::new();
        c.increment_by("x", 42);
        c.increment_by("y", 7);

        let bytes = c.to_bytes();
        let decoded = GCounter::from_bytes(&bytes).unwrap();
        assert_eq!(c, decoded);
    }

    #[test]
    fn empty_counter() {
        let c = GCounter::new();
        assert_eq!(c.value(), 0);
        assert_eq!(c.node_count(), 0);
    }
}
