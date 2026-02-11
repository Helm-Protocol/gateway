//! Merkle DAG — content-addressed storage with hash-linked nodes.
//!
//! Every node is identified by the SHA-256 hash of its serialized content.
//! Used for:
//! - Efficient sync (compare root hashes to detect differences)
//! - CRDT state history (each mutation creates a new DAG node)
//! - Tamper detection (any modification breaks the hash chain)

use std::fmt;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};

use crate::kv::KvStore;

/// 32-byte SHA-256 hash used as node identifier.
pub type Hash = [u8; 32];

/// Format a hash as a hex string (first 8 chars for display).
pub fn hash_short(h: &Hash) -> String {
    h.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// Format a hash as a full hex string.
pub fn hash_hex(h: &Hash) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// A node in the Merkle DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNode {
    /// Data payload (application-defined).
    pub data: Vec<u8>,
    /// Links to parent nodes (their hashes).
    pub parents: Vec<Hash>,
    /// Timestamp (unix millis) when the node was created.
    pub timestamp_ms: u64,
}

impl DagNode {
    /// Compute the SHA-256 hash of this node.
    pub fn hash(&self) -> Hash {
        let serialized = serde_json::to_vec(self).expect("DagNode serialization cannot fail");
        let mut hasher = Sha256::new();
        hasher.update(&serialized);
        hasher.finalize().into()
    }
}

/// Merkle DAG backed by a KvStore.
///
/// Nodes are stored as: key = `dag:<hash_hex>`, value = serialized DagNode.
/// The root pointer is stored as: key = `dag:root`.
pub struct MerkleDag<'a> {
    store: &'a dyn KvStore,
    root: Option<Hash>,
}

impl<'a> MerkleDag<'a> {
    /// Create a new MerkleDag backed by the given store.
    pub fn new(store: &'a dyn KvStore) -> Result<Self> {
        // Try to load existing root
        let root = store.get(b"dag:root")?
            .and_then(|bytes| {
                if bytes.len() == 32 {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(&bytes);
                    Some(h)
                } else {
                    None
                }
            });

        Ok(Self { store, root })
    }

    /// Get the current root hash.
    pub fn root(&self) -> Option<&Hash> {
        self.root.as_ref()
    }

    /// Add a new node to the DAG. Returns the node's hash.
    ///
    /// The new node's parents default to the current root (if any).
    pub fn add(&mut self, data: &[u8], timestamp_ms: u64) -> Result<Hash> {
        let parents = self.root.map(|r| vec![r]).unwrap_or_default();
        self.add_with_parents(data, parents, timestamp_ms)
    }

    /// Add a node with explicit parent links.
    pub fn add_with_parents(
        &mut self,
        data: &[u8],
        parents: Vec<Hash>,
        timestamp_ms: u64,
    ) -> Result<Hash> {
        let node = DagNode {
            data: data.to_vec(),
            parents,
            timestamp_ms,
        };

        let hash = node.hash();
        let key = dag_key(&hash);
        let value = serde_json::to_vec(&node)?;

        self.store.put(&key, &value)?;
        self.root = Some(hash);
        self.store.put(b"dag:root", &hash)?;

        Ok(hash)
    }

    /// Get a node by its hash.
    pub fn get_node(&self, hash: &Hash) -> Result<Option<DagNode>> {
        let key = dag_key(hash);
        match self.store.get(&key)? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Check if a node exists.
    pub fn contains(&self, hash: &Hash) -> Result<bool> {
        let key = dag_key(hash);
        self.store.contains(&key)
    }

    /// Walk backwards from a node to collect its ancestry chain.
    /// Returns nodes in order from the given node back to the root.
    pub fn ancestors(&self, from: &Hash, max_depth: usize) -> Result<Vec<(Hash, DagNode)>> {
        let mut result = Vec::new();
        let mut queue = vec![*from];
        let mut visited = std::collections::HashSet::new();

        while let Some(current) = queue.pop() {
            if visited.contains(&current) || result.len() >= max_depth {
                break;
            }
            visited.insert(current);

            if let Some(node) = self.get_node(&current)? {
                for parent in &node.parents {
                    queue.push(*parent);
                }
                result.push((current, node));
            }
        }

        Ok(result)
    }

    /// Count total nodes in the DAG.
    pub fn node_count(&self) -> Result<usize> {
        let entries = self.store.scan_prefix(b"dag:")?;
        // Subtract 1 for the "dag:root" pointer if it exists
        let total = entries.len();
        if self.root.is_some() && total > 0 {
            Ok(total - 1)
        } else {
            Ok(total)
        }
    }

    /// Get all node hashes stored in the DAG.
    pub fn all_hashes(&self) -> Result<Vec<Hash>> {
        let entries = self.store.scan_prefix(b"dag:")?;
        let mut hashes = Vec::new();
        for (key, _) in entries {
            if key == b"dag:root" {
                continue;
            }
            // key format: "dag:<64-char hex>"
            if key.len() == 4 + 64 {
                if let Ok(hash) = parse_hash_from_key(&key) {
                    hashes.push(hash);
                }
            }
        }
        Ok(hashes)
    }

    /// Verify the integrity of a node (recompute hash and compare).
    pub fn verify(&self, hash: &Hash) -> Result<bool> {
        match self.get_node(hash)? {
            Some(node) => Ok(node.hash() == *hash),
            None => Ok(false),
        }
    }
}

impl<'a> fmt::Debug for MerkleDag<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MerkleDag")
            .field("root", &self.root.map(|h| hash_short(&h)))
            .finish()
    }
}

/// Build the storage key for a DAG node.
fn dag_key(hash: &Hash) -> Vec<u8> {
    let mut key = b"dag:".to_vec();
    key.extend(hash_hex(hash).as_bytes());
    key
}

/// Parse a hash from a DAG storage key.
fn parse_hash_from_key(key: &[u8]) -> Result<Hash> {
    let hex_str = std::str::from_utf8(&key[4..])?;
    let bytes = (0..hex_str.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()?;

    if bytes.len() != 32 {
        anyhow::bail!("invalid hash length");
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::memory::MemoryBackend;

    #[test]
    fn add_and_retrieve_node() {
        let store = MemoryBackend::new();
        let mut dag = MerkleDag::new(&store).unwrap();

        let hash = dag.add(b"hello world", 1000).unwrap();
        let node = dag.get_node(&hash).unwrap().unwrap();
        assert_eq!(node.data, b"hello world");
        assert_eq!(node.timestamp_ms, 1000);
        assert!(node.parents.is_empty());
    }

    #[test]
    fn chain_of_nodes() {
        let store = MemoryBackend::new();
        let mut dag = MerkleDag::new(&store).unwrap();

        let h1 = dag.add(b"first", 100).unwrap();
        let h2 = dag.add(b"second", 200).unwrap();
        let h3 = dag.add(b"third", 300).unwrap();

        // h3 should point to h2, h2 to h1
        let n3 = dag.get_node(&h3).unwrap().unwrap();
        assert_eq!(n3.parents, vec![h2]);

        let n2 = dag.get_node(&h2).unwrap().unwrap();
        assert_eq!(n2.parents, vec![h1]);

        let n1 = dag.get_node(&h1).unwrap().unwrap();
        assert!(n1.parents.is_empty());

        assert_eq!(dag.node_count().unwrap(), 3);
    }

    #[test]
    fn root_tracking() {
        let store = MemoryBackend::new();
        let mut dag = MerkleDag::new(&store).unwrap();

        assert!(dag.root().is_none());

        let h1 = dag.add(b"one", 1).unwrap();
        assert_eq!(dag.root(), Some(&h1));

        let h2 = dag.add(b"two", 2).unwrap();
        assert_eq!(dag.root(), Some(&h2));
    }

    #[test]
    fn integrity_verification() {
        let store = MemoryBackend::new();
        let mut dag = MerkleDag::new(&store).unwrap();

        let hash = dag.add(b"verify me", 500).unwrap();
        assert!(dag.verify(&hash).unwrap());

        // Non-existent hash
        let fake = [0u8; 32];
        assert!(!dag.verify(&fake).unwrap());
    }

    #[test]
    fn ancestor_walk() {
        let store = MemoryBackend::new();
        let mut dag = MerkleDag::new(&store).unwrap();

        dag.add(b"a", 1).unwrap();
        dag.add(b"b", 2).unwrap();
        let h3 = dag.add(b"c", 3).unwrap();

        let ancestors = dag.ancestors(&h3, 10).unwrap();
        assert_eq!(ancestors.len(), 3);
        assert_eq!(ancestors[0].1.data, b"c");
    }

    #[test]
    fn persistent_root() {
        let store = MemoryBackend::new();

        // First session
        {
            let mut dag = MerkleDag::new(&store).unwrap();
            dag.add(b"persisted", 1000).unwrap();
        }

        // Second session — should recover root
        {
            let dag = MerkleDag::new(&store).unwrap();
            assert!(dag.root().is_some());
            let node = dag.get_node(dag.root().unwrap()).unwrap().unwrap();
            assert_eq!(node.data, b"persisted");
        }
    }

    #[test]
    fn hash_determinism() {
        let node = DagNode {
            data: b"test".to_vec(),
            parents: vec![],
            timestamp_ms: 42,
        };
        let h1 = node.hash();
        let h2 = node.hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn all_hashes_list() {
        let store = MemoryBackend::new();
        let mut dag = MerkleDag::new(&store).unwrap();

        dag.add(b"x", 1).unwrap();
        dag.add(b"y", 2).unwrap();
        dag.add(b"z", 3).unwrap();

        let hashes = dag.all_hashes().unwrap();
        assert_eq!(hashes.len(), 3);
    }
}
