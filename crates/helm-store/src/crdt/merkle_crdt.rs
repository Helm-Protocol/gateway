//! Merkle-CRDT — CRDT state backed by a Merkle DAG.
//!
//! Every mutation is recorded as a DAG node, providing:
//! - Immutable history of all state changes
//! - Efficient sync (compare root hashes)
//! - Tamper detection (hash chain integrity)
//!
//! The current CRDT state is rebuilt by replaying the DAG from root.

use anyhow::Result;
use serde::{Serialize, Deserialize};

use crate::kv::KvStore;
use crate::merkle::dag::{MerkleDag, Hash, hash_short};
use super::gcounter::GCounter;
use super::lww::LwwRegister;
use super::orset::OrSet;

/// Operation recorded in the Merkle DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrdtOp {
    /// Increment a G-Counter.
    GCounterIncrement { key: String, node_id: String, amount: u64 },
    /// Set a LWW-Register.
    LwwSet { key: String, value: Vec<u8>, timestamp_ms: u64 },
    /// Add to an OR-Set.
    OrSetAdd { key: String, element: Vec<u8> },
    /// Remove from an OR-Set.
    OrSetRemove { key: String, element: Vec<u8> },
}

/// A named CRDT value in the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrdtValue {
    Counter(GCounter),
    Register(LwwRegister),
    Set(OrSet),
}

/// Merkle-backed CRDT store.
///
/// Operations are appended to the DAG. Current state is maintained in memory
/// and can be rebuilt from the DAG at any time.
pub struct MerkleCrdt<'a> {
    dag: MerkleDag<'a>,
    /// Current CRDT state (key → CrdtValue)
    state: std::collections::HashMap<String, CrdtValue>,
}

impl<'a> MerkleCrdt<'a> {
    /// Create a new Merkle-CRDT backed by the given store.
    pub fn new(store: &'a dyn KvStore) -> Result<Self> {
        let dag = MerkleDag::new(store)?;
        let mut crdt = Self {
            dag,
            state: std::collections::HashMap::new(),
        };

        // Rebuild state from existing DAG
        crdt.rebuild_state()?;
        Ok(crdt)
    }

    /// Increment a G-Counter.
    pub fn counter_increment(
        &mut self,
        key: &str,
        node_id: &str,
        amount: u64,
        timestamp_ms: u64,
    ) -> Result<Hash> {
        let op = CrdtOp::GCounterIncrement {
            key: key.to_string(),
            node_id: node_id.to_string(),
            amount,
        };
        self.apply_op(&op);
        let bytes = serde_json::to_vec(&op)?;
        self.dag.add(&bytes, timestamp_ms)
    }

    /// Get a counter value.
    pub fn counter_value(&self, key: &str) -> u64 {
        match self.state.get(key) {
            Some(CrdtValue::Counter(c)) => c.value(),
            _ => 0,
        }
    }

    /// Set a LWW-Register.
    pub fn register_set(
        &mut self,
        key: &str,
        value: &[u8],
        timestamp_ms: u64,
    ) -> Result<Hash> {
        let op = CrdtOp::LwwSet {
            key: key.to_string(),
            value: value.to_vec(),
            timestamp_ms,
        };
        self.apply_op(&op);
        let bytes = serde_json::to_vec(&op)?;
        self.dag.add(&bytes, timestamp_ms)
    }

    /// Get a register value.
    pub fn register_value(&self, key: &str) -> Option<&[u8]> {
        match self.state.get(key) {
            Some(CrdtValue::Register(r)) => Some(r.value()),
            _ => None,
        }
    }

    /// Add to an OR-Set.
    pub fn set_add(
        &mut self,
        key: &str,
        element: &[u8],
        timestamp_ms: u64,
    ) -> Result<Hash> {
        let op = CrdtOp::OrSetAdd {
            key: key.to_string(),
            element: element.to_vec(),
        };
        self.apply_op(&op);
        let bytes = serde_json::to_vec(&op)?;
        self.dag.add(&bytes, timestamp_ms)
    }

    /// Remove from an OR-Set.
    pub fn set_remove(
        &mut self,
        key: &str,
        element: &[u8],
        timestamp_ms: u64,
    ) -> Result<Hash> {
        let op = CrdtOp::OrSetRemove {
            key: key.to_string(),
            element: element.to_vec(),
        };
        self.apply_op(&op);
        let bytes = serde_json::to_vec(&op)?;
        self.dag.add(&bytes, timestamp_ms)
    }

    /// Check if an OR-Set contains an element.
    pub fn set_contains(&self, key: &str, element: &[u8]) -> bool {
        match self.state.get(key) {
            Some(CrdtValue::Set(s)) => s.contains(element),
            _ => false,
        }
    }

    /// Get all elements of an OR-Set.
    pub fn set_elements(&self, key: &str) -> Vec<Vec<u8>> {
        match self.state.get(key) {
            Some(CrdtValue::Set(s)) => s.elements(),
            _ => Vec::new(),
        }
    }

    /// Get the Merkle root hash.
    pub fn root_hash(&self) -> Option<&Hash> {
        self.dag.root()
    }

    /// Number of operations recorded.
    pub fn op_count(&self) -> Result<usize> {
        self.dag.node_count()
    }

    /// Number of CRDT keys.
    pub fn key_count(&self) -> usize {
        self.state.len()
    }

    /// Get root hash as short hex string.
    pub fn root_short(&self) -> String {
        self.dag.root()
            .map(hash_short)
            .unwrap_or_else(|| "none".to_string())
    }

    /// Apply an operation to in-memory state (no DAG write).
    fn apply_op(&mut self, op: &CrdtOp) {
        match op {
            CrdtOp::GCounterIncrement { key, node_id, amount } => {
                let counter = self.state
                    .entry(key.clone())
                    .or_insert_with(|| CrdtValue::Counter(GCounter::new()));
                if let CrdtValue::Counter(c) = counter {
                    c.increment_by(node_id, *amount);
                }
            }
            CrdtOp::LwwSet { key, value, timestamp_ms } => {
                let register = self.state
                    .entry(key.clone())
                    .or_insert_with(|| CrdtValue::Register(LwwRegister::new(b"", 0)));
                if let CrdtValue::Register(r) = register {
                    r.set(value, *timestamp_ms);
                }
            }
            CrdtOp::OrSetAdd { key, element } => {
                let set = self.state
                    .entry(key.clone())
                    .or_insert_with(|| CrdtValue::Set(OrSet::new("local")));
                if let CrdtValue::Set(s) = set {
                    s.add(element);
                }
            }
            CrdtOp::OrSetRemove { key, element } => {
                if let Some(CrdtValue::Set(s)) = self.state.get_mut(key) {
                    s.remove(element);
                }
            }
        }
    }

    /// Rebuild state from DAG by replaying all operations.
    fn rebuild_state(&mut self) -> Result<()> {
        let root = match self.dag.root() {
            Some(h) => *h,
            None => return Ok(()),
        };

        let ancestors = self.dag.ancestors(&root, 10000)?;
        // Replay in reverse order (oldest first)
        for (_hash, node) in ancestors.into_iter().rev() {
            if let Ok(op) = serde_json::from_slice::<CrdtOp>(&node.data) {
                self.apply_op(&op);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::memory::MemoryBackend;

    #[test]
    fn counter_operations() {
        let store = MemoryBackend::new();
        let mut crdt = MerkleCrdt::new(&store).unwrap();

        crdt.counter_increment("hits", "node-1", 5, 100).unwrap();
        crdt.counter_increment("hits", "node-2", 3, 200).unwrap();

        assert_eq!(crdt.counter_value("hits"), 8);
        assert_eq!(crdt.op_count().unwrap(), 2);
    }

    #[test]
    fn register_operations() {
        let store = MemoryBackend::new();
        let mut crdt = MerkleCrdt::new(&store).unwrap();

        crdt.register_set("config", b"v1", 100).unwrap();
        assert_eq!(crdt.register_value("config"), Some(b"v1".as_slice()));

        crdt.register_set("config", b"v2", 200).unwrap();
        assert_eq!(crdt.register_value("config"), Some(b"v2".as_slice()));
    }

    #[test]
    fn register_lww_semantics() {
        let store = MemoryBackend::new();
        let mut crdt = MerkleCrdt::new(&store).unwrap();

        crdt.register_set("key", b"newer", 200).unwrap();
        crdt.register_set("key", b"older", 100).unwrap(); // Should not overwrite

        assert_eq!(crdt.register_value("key"), Some(b"newer".as_slice()));
    }

    #[test]
    fn set_operations() {
        let store = MemoryBackend::new();
        let mut crdt = MerkleCrdt::new(&store).unwrap();

        crdt.set_add("peers", b"alice", 100).unwrap();
        crdt.set_add("peers", b"bob", 200).unwrap();

        assert!(crdt.set_contains("peers", b"alice"));
        assert!(crdt.set_contains("peers", b"bob"));

        crdt.set_remove("peers", b"alice", 300).unwrap();
        assert!(!crdt.set_contains("peers", b"alice"));
        assert!(crdt.set_contains("peers", b"bob"));
    }

    #[test]
    fn merkle_root_changes() {
        let store = MemoryBackend::new();
        let mut crdt = MerkleCrdt::new(&store).unwrap();

        assert!(crdt.root_hash().is_none());

        crdt.counter_increment("x", "n1", 1, 100).unwrap();
        let root1 = *crdt.root_hash().unwrap();

        crdt.counter_increment("x", "n1", 1, 200).unwrap();
        let root2 = *crdt.root_hash().unwrap();

        assert_ne!(root1, root2);
    }

    #[test]
    fn mixed_crdt_types() {
        let store = MemoryBackend::new();
        let mut crdt = MerkleCrdt::new(&store).unwrap();

        crdt.counter_increment("count", "n1", 10, 100).unwrap();
        crdt.register_set("name", b"helm", 200).unwrap();
        crdt.set_add("tags", b"p2p", 300).unwrap();
        crdt.set_add("tags", b"rust", 400).unwrap();

        assert_eq!(crdt.counter_value("count"), 10);
        assert_eq!(crdt.register_value("name"), Some(b"helm".as_slice()));
        assert!(crdt.set_contains("tags", b"p2p"));
        assert!(crdt.set_contains("tags", b"rust"));
        assert_eq!(crdt.key_count(), 3);
    }

    #[test]
    fn state_persistence_and_rebuild() {
        let store = MemoryBackend::new();

        // First session: write state
        {
            let mut crdt = MerkleCrdt::new(&store).unwrap();
            crdt.counter_increment("views", "n1", 42, 100).unwrap();
            crdt.register_set("version", b"0.1.0", 200).unwrap();
        }

        // Second session: rebuild state from DAG
        {
            let crdt = MerkleCrdt::new(&store).unwrap();
            assert_eq!(crdt.counter_value("views"), 42);
            assert_eq!(crdt.register_value("version"), Some(b"0.1.0".as_slice()));
        }
    }
}
