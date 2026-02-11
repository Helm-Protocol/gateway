//! In-memory KV store backend.
//!
//! Fast, ephemeral storage for testing and short-lived nodes.
//! All data lives in a BTreeMap (sorted keys for efficient prefix scans).

use std::collections::BTreeMap;
use std::sync::RwLock;
use anyhow::Result;

use crate::kv::KvStore;

/// In-memory backend using BTreeMap with RwLock for thread safety.
pub struct MemoryBackend {
    data: RwLock<BTreeMap<Vec<u8>, Vec<u8>>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(BTreeMap::new()),
        }
    }
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl KvStore for MemoryBackend {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let data = self.data.read().map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
        Ok(data.get(key).cloned())
    }

    fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut data = self.data.write().map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
        data.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<bool> {
        let mut data = self.data.write().map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
        Ok(data.remove(key).is_some())
    }

    fn contains(&self, key: &[u8]) -> Result<bool> {
        let data = self.data.read().map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
        Ok(data.contains_key(key))
    }

    fn keys(&self) -> Result<Vec<Vec<u8>>> {
        let data = self.data.read().map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
        Ok(data.keys().cloned().collect())
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let data = self.data.read().map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
        let prefix_vec = prefix.to_vec();
        Ok(data
            .range(prefix_vec..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    fn flush(&self) -> Result<()> {
        Ok(()) // No-op for in-memory
    }

    fn len(&self) -> Result<usize> {
        let data = self.data.read().map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
        Ok(data.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::tests::run_kv_store_tests;

    #[test]
    fn memory_backend_standard_suite() {
        let store = MemoryBackend::new();
        run_kv_store_tests(&store);
    }

    #[test]
    fn empty_prefix_returns_all() {
        let store = MemoryBackend::new();
        store.put(b"a", b"1").unwrap();
        store.put(b"b", b"2").unwrap();
        let all = store.scan_prefix(b"").unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn concurrent_reads() {
        use std::sync::Arc;
        let store = Arc::new(MemoryBackend::new());
        store.put(b"shared", b"data").unwrap();

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let s = Arc::clone(&store);
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        assert_eq!(s.get(b"shared").unwrap(), Some(b"data".to_vec()));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
    }
}
