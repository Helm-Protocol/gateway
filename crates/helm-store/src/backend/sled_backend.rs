//! Sled-based persistent KV store backend.
//!
//! Uses the sled embedded database for durable storage.
//! Ideal for production nodes that need data to survive restarts.

use anyhow::Result;
use tracing::info;

use crate::kv::KvStore;

/// Persistent backend using sled embedded database.
pub struct SledBackend {
    db: sled::Db,
}

impl SledBackend {
    /// Open a sled database at the given path.
    pub fn open(path: &str) -> Result<Self> {
        let db = sled::open(path)?;
        info!("SledBackend: opened database at {}", path);
        Ok(Self { db })
    }

    /// Open a temporary sled database (for testing).
    pub fn temporary() -> Result<Self> {
        let config = sled::Config::new().temporary(true);
        let db = config.open()?;
        Ok(Self { db })
    }
}

impl KvStore for SledBackend {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.db.get(key)?.map(|v| v.to_vec()))
    }

    fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.db.insert(key, value)?;
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<bool> {
        Ok(self.db.remove(key)?.is_some())
    }

    fn contains(&self, key: &[u8]) -> Result<bool> {
        Ok(self.db.contains_key(key)?)
    }

    fn keys(&self) -> Result<Vec<Vec<u8>>> {
        let mut keys = Vec::new();
        for item in self.db.iter() {
            let (k, _) = item?;
            keys.push(k.to_vec());
        }
        Ok(keys)
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut results = Vec::new();
        for item in self.db.scan_prefix(prefix) {
            let (k, v) = item?;
            results.push((k.to_vec(), v.to_vec()));
        }
        Ok(results)
    }

    fn flush(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }

    fn len(&self) -> Result<usize> {
        Ok(self.db.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::tests::run_kv_store_tests;

    #[test]
    fn sled_backend_standard_suite() {
        let store = SledBackend::temporary().unwrap();
        run_kv_store_tests(&store);
    }

    #[test]
    fn sled_persistence_within_session() {
        let store = SledBackend::temporary().unwrap();
        store.put(b"persist", b"this").unwrap();
        store.flush().unwrap();
        assert_eq!(store.get(b"persist").unwrap(), Some(b"this".to_vec()));
    }

    #[test]
    fn sled_large_values() {
        let store = SledBackend::temporary().unwrap();
        let big_value = vec![42u8; 1024 * 64]; // 64KB
        store.put(b"big", &big_value).unwrap();
        assert_eq!(store.get(b"big").unwrap(), Some(big_value));
    }
}
