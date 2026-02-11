//! OR-Set (Observed-Remove Set) — add-wins set CRDT.
//!
//! Each add generates a unique (replica_id, seq) tag.
//! Remove only removes observed tags.
//! Concurrent add + remove → element remains (add wins).

use std::collections::BTreeSet;
use serde::{Serialize, Deserialize};

use super::gcounter::Crdt;

/// Unique tag: (replica_id, sequence_number).
/// Guarantees global uniqueness across replicas.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Tag(pub String, pub u64);

/// Entry in the set: an element with its active tags.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    element: Vec<u8>,
    tags: BTreeSet<Tag>,
}

/// OR-Set: Observed-Remove Set.
///
/// Elements can be added and removed concurrently.
/// Add-wins semantics: if add and remove happen concurrently,
/// the element stays (because the add creates a new unobserved tag).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrSet {
    /// Replica ID for this instance.
    replica_id: String,
    /// Entries: element bytes (hex-encoded) → active tags
    entries: Vec<Entry>,
    /// Monotonic sequence counter for this replica.
    seq: u64,
    /// Tombstoned tags.
    tombstones: BTreeSet<Tag>,
}

impl OrSet {
    /// Create a new OR-Set for the given replica.
    pub fn new(replica_id: &str) -> Self {
        Self {
            replica_id: replica_id.to_string(),
            entries: Vec::new(),
            seq: 0,
            tombstones: BTreeSet::new(),
        }
    }

    /// Add an element. Returns the tag assigned.
    pub fn add(&mut self, element: &[u8]) -> Tag {
        self.seq += 1;
        let tag = Tag(self.replica_id.clone(), self.seq);

        if let Some(entry) = self.entries.iter_mut().find(|e| e.element == element) {
            entry.tags.insert(tag.clone());
        } else {
            let mut tags = BTreeSet::new();
            tags.insert(tag.clone());
            self.entries.push(Entry {
                element: element.to_vec(),
                tags,
            });
        }

        tag
    }

    /// Remove an element (all currently observed tags).
    /// Returns the number of tags removed.
    pub fn remove(&mut self, element: &[u8]) -> usize {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.element == element) {
            let count = entry.tags.len();
            let old_tags = std::mem::take(&mut entry.tags);
            for tag in old_tags {
                self.tombstones.insert(tag);
            }
            count
        } else {
            0
        }
    }

    /// Check if an element is in the set.
    pub fn contains(&self, element: &[u8]) -> bool {
        self.entries
            .iter()
            .any(|e| e.element == element && !e.tags.is_empty())
    }

    /// Get all elements currently in the set.
    pub fn elements(&self) -> Vec<Vec<u8>> {
        self.entries
            .iter()
            .filter(|e| !e.tags.is_empty())
            .map(|e| e.element.clone())
            .collect()
    }

    /// Number of distinct elements in the set.
    pub fn len(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| !e.tags.is_empty())
            .count()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("OrSet serialization cannot fail")
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

impl Default for OrSet {
    fn default() -> Self {
        Self::new("default")
    }
}

impl PartialEq for OrSet {
    fn eq(&self, other: &Self) -> bool {
        // Compare by active elements (ignore internal structure differences)
        let mut a = self.elements();
        a.sort();
        let mut b = other.elements();
        b.sort();
        a == b
    }
}

impl Crdt for OrSet {
    fn merge(&mut self, other: &Self) {
        // Combined tombstones from both sides
        let all_tombstones: BTreeSet<Tag> = self.tombstones
            .union(&other.tombstones)
            .cloned()
            .collect();

        // Add all tags from other that aren't tombstoned
        for other_entry in &other.entries {
            let entry = if let Some(e) = self.entries.iter_mut().find(|e| e.element == other_entry.element) {
                e
            } else {
                self.entries.push(Entry {
                    element: other_entry.element.clone(),
                    tags: BTreeSet::new(),
                });
                self.entries.last_mut().unwrap()
            };

            for tag in &other_entry.tags {
                if !all_tombstones.contains(tag) {
                    entry.tags.insert(tag.clone());
                }
            }
        }

        // Remove tombstoned tags from our entries
        for entry in &mut self.entries {
            entry.tags.retain(|tag| !all_tombstones.contains(tag));
        }

        // Clean up empty entries
        self.entries.retain(|e| !e.tags.is_empty());

        // Merge tombstones
        self.tombstones = all_tombstones;

        // Update seq counter
        self.seq = self.seq.max(other.seq);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_contains() {
        let mut set = OrSet::new("r1");
        set.add(b"alice");
        set.add(b"bob");

        assert!(set.contains(b"alice"));
        assert!(set.contains(b"bob"));
        assert!(!set.contains(b"charlie"));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn remove_element() {
        let mut set = OrSet::new("r1");
        set.add(b"item");
        assert!(set.contains(b"item"));

        set.remove(b"item");
        assert!(!set.contains(b"item"));
        assert!(set.is_empty());
    }

    #[test]
    fn add_after_remove() {
        let mut set = OrSet::new("r1");
        set.add(b"x");
        set.remove(b"x");
        set.add(b"x");
        assert!(set.contains(b"x"));
    }

    #[test]
    fn merge_union() {
        let mut a = OrSet::new("r1");
        a.add(b"alice");

        let mut b = OrSet::new("r2");
        b.add(b"bob");

        a.merge(&b);
        assert!(a.contains(b"alice"));
        assert!(a.contains(b"bob"));
    }

    #[test]
    fn concurrent_add_remove_add_wins() {
        // Node A adds "item" with tag ("r1", 1)
        let mut a = OrSet::new("r1");
        a.add(b"item");

        // Node B independently adds "item" with tag ("r2", 1)
        let mut b = OrSet::new("r2");
        b.add(b"item");

        // Node A removes "item" — tombstones ("r1", 1)
        a.remove(b"item");

        // Merge: B's tag ("r2", 1) survives because A's tombstone is ("r1", 1)
        a.merge(&b);
        assert!(a.contains(b"item")); // add wins
    }

    #[test]
    fn merge_is_idempotent() {
        let mut a = OrSet::new("r1");
        a.add(b"x");
        a.add(b"y");

        let b = a.clone();
        a.merge(&b);
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn merge_is_commutative() {
        let mut a = OrSet::new("r1");
        a.add(b"a");

        let mut b = OrSet::new("r2");
        b.add(b"b");

        let mut ab = a.clone();
        ab.merge(&b);

        let mut ba = b.clone();
        ba.merge(&a);

        let mut ab_elems = ab.elements();
        ab_elems.sort();
        let mut ba_elems = ba.elements();
        ba_elems.sort();

        assert_eq!(ab_elems, ba_elems);
    }

    #[test]
    fn serialization_roundtrip() {
        let mut set = OrSet::new("r1");
        set.add(b"one");
        set.add(b"two");

        let bytes = set.to_bytes();
        let decoded = OrSet::from_bytes(&bytes).unwrap();
        assert!(decoded.contains(b"one"));
        assert!(decoded.contains(b"two"));
    }

    #[test]
    fn elements_listing() {
        let mut set = OrSet::new("r1");
        set.add(b"c");
        set.add(b"a");
        set.add(b"b");

        let mut elems = set.elements();
        elems.sort();
        assert_eq!(elems, vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
    }
}
