//! KvStore trait — the storage abstraction for Helm.
//!
//! All backends (memory, sled, etc.) implement this trait.
//! CRDT state, Merkle DAG nodes, and sync metadata all go through here.

use anyhow::Result;

/// Key-value store abstraction.
///
/// All operations are synchronous (embedded stores are fast enough).
/// For network-backed stores, use async wrappers externally.
pub trait KvStore: Send + Sync {
    /// Get a value by key. Returns None if not found.
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Put a key-value pair. Overwrites if key exists.
    fn put(&self, key: &[u8], value: &[u8]) -> Result<()>;

    /// Delete a key. Returns true if the key existed.
    fn delete(&self, key: &[u8]) -> Result<bool>;

    /// Check if a key exists.
    fn contains(&self, key: &[u8]) -> Result<bool> {
        Ok(self.get(key)?.is_some())
    }

    /// List all keys.
    fn keys(&self) -> Result<Vec<Vec<u8>>>;

    /// Scan all key-value pairs with a given prefix.
    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;

    /// Flush pending writes to durable storage (no-op for memory backend).
    fn flush(&self) -> Result<()>;

    /// Number of entries in the store.
    fn len(&self) -> Result<usize> {
        Ok(self.keys()?.len())
    }

    /// Whether the store is empty.
    fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Standard test suite that every KvStore backend must pass.
    pub fn run_kv_store_tests(store: &dyn KvStore) {
        // Initially empty
        assert!(store.is_empty().unwrap());
        assert_eq!(store.len().unwrap(), 0);

        // Put and get
        store.put(b"key1", b"value1").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(store.len().unwrap(), 1);

        // Contains
        assert!(store.contains(b"key1").unwrap());
        assert!(!store.contains(b"key2").unwrap());

        // Overwrite
        store.put(b"key1", b"updated").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"updated".to_vec()));
        assert_eq!(store.len().unwrap(), 1);

        // Multiple keys
        store.put(b"key2", b"value2").unwrap();
        store.put(b"key3", b"value3").unwrap();
        assert_eq!(store.len().unwrap(), 3);

        // Delete
        assert!(store.delete(b"key2").unwrap());
        assert!(!store.contains(b"key2").unwrap());
        assert_eq!(store.len().unwrap(), 2);

        // Delete non-existent
        assert!(!store.delete(b"nonexistent").unwrap());

        // Get non-existent
        assert_eq!(store.get(b"nonexistent").unwrap(), None);

        // Scan prefix
        store.put(b"pfx:a", b"1").unwrap();
        store.put(b"pfx:b", b"2").unwrap();
        store.put(b"other:c", b"3").unwrap();
        let results = store.scan_prefix(b"pfx:").unwrap();
        assert_eq!(results.len(), 2);

        // Keys list
        let keys = store.keys().unwrap();
        assert!(keys.len() >= 4); // key1, key3, pfx:a, pfx:b, other:c

        // Flush (should not error)
        store.flush().unwrap();
    }
}
