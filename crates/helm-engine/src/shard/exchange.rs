//! High-speed shard exchange — the Data Plane.
//!
//! Deterministic O(1) shard storage and retrieval using HashMap.
//! This is the "muscle" layer — no semantic computation, pure speed.
//!
//! Design principle (Jeff Dean analysis):
//! - Data Plane uses KV (fast, deterministic)
//! - Control Plane uses QKV-G (smart, probabilistic)

use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use tracing::{info, warn};

use crate::grg::redstuff::Shard;
use crate::grg::pipeline::{GrgPipeline, GrgMode, GrgEncoded};

/// Unique identifier for a data blob.
pub type BlobId = u64;

/// Unique identifier for a shard within a blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShardKey {
    pub blob_id: BlobId,
    pub shard_index: usize,
}

/// Metadata about a stored blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobMeta {
    /// Blob identifier
    pub blob_id: BlobId,
    /// Original data size (bytes)
    pub original_size: usize,
    /// Compressed size (bytes, after Golomb)
    pub compressed_size: usize,
    /// Number of data shards
    pub data_shard_count: usize,
    /// Number of parity shards
    pub parity_shard_count: usize,
    /// GRG mode used
    pub grg_mode: GrgMode,
    /// Golomb M parameter used
    pub golomb_m: u32,
    /// Timestamp of storage (unix millis)
    pub stored_at_ms: u64,
}

/// The shard exchange — high-speed distributed shard storage.
///
/// ```text
/// Store: data → GRG encode → shards → HashMap (O(1) per shard)
/// Fetch: key → HashMap (O(1)) → shard → (collect K shards) → GRG decode → data
/// ```
pub struct ShardExchange {
    /// Shard storage: ShardKey → Shard data (O(1) lookup)
    store: HashMap<ShardKey, Shard>,
    /// Blob metadata: BlobId → BlobMeta
    meta: HashMap<BlobId, BlobMeta>,
    /// GRG pipeline for encoding/decoding
    grg: GrgPipeline,
    /// Next blob ID to assign
    next_blob_id: BlobId,
    /// Total bytes stored (raw shard data)
    total_stored_bytes: usize,
}

impl ShardExchange {
    /// Create a new shard exchange.
    pub fn new(mode: GrgMode) -> Self {
        Self {
            store: HashMap::new(),
            meta: HashMap::new(),
            grg: GrgPipeline::new(mode).with_data_shards(4),
            next_blob_id: 1,
            total_stored_bytes: 0,
        }
    }

    /// Store data: GRG-encode and distribute shards. Returns the blob ID.
    pub fn store(
        &mut self,
        data: &[u8],
        timestamp_ms: u64,
    ) -> Result<BlobId, anyhow::Error> {
        let blob_id = self.next_blob_id;
        self.next_blob_id += 1;

        let encoded = self.grg.encode(data)?;

        let meta = BlobMeta {
            blob_id,
            original_size: encoded.original_len,
            compressed_size: encoded.compressed_len,
            data_shard_count: encoded.shards.iter().filter(|s| !s.is_parity).count(),
            parity_shard_count: encoded.shards.iter().filter(|s| s.is_parity).count(),
            grg_mode: encoded.mode,
            golomb_m: encoded.golomb_m,
            stored_at_ms: timestamp_ms,
        };

        info!(
            "ShardExchange: stored blob {} ({} bytes → {} shards)",
            blob_id, data.len(), encoded.shards.len()
        );

        // Store each shard in the HashMap (O(1) per shard)
        for shard in &encoded.shards {
            let key = ShardKey {
                blob_id,
                shard_index: shard.index,
            };
            self.total_stored_bytes += shard.data.len();
            self.store.insert(key, shard.clone());
        }

        self.meta.insert(blob_id, meta);

        Ok(blob_id)
    }

    /// Fetch a single shard by key (O(1) — Data Plane fast path).
    pub fn fetch_shard(&self, key: &ShardKey) -> Option<&Shard> {
        self.store.get(key)
    }

    /// Retrieve the original data from stored shards.
    pub fn retrieve(&self, blob_id: BlobId) -> Result<Vec<u8>, anyhow::Error> {
        let meta = self.meta.get(&blob_id)
            .ok_or_else(|| anyhow::anyhow!("blob {} not found", blob_id))?;

        // Collect all shards for this blob
        let total_shards = meta.data_shard_count + meta.parity_shard_count;
        let mut shards = Vec::with_capacity(total_shards);

        for idx in 0..total_shards {
            let key = ShardKey {
                blob_id,
                shard_index: idx,
            };
            if let Some(shard) = self.store.get(&key) {
                shards.push(shard.clone());
            }
        }

        if shards.is_empty() {
            anyhow::bail!("no shards found for blob {}", blob_id);
        }

        // Reconstruct via GRG decode
        let encoded = GrgEncoded {
            shards,
            original_len: meta.original_size,
            compressed_len: meta.compressed_size,
            mode: meta.grg_mode,
            golomb_m: meta.golomb_m,
        };

        self.grg.decode(&encoded)
    }

    /// Remove a blob and all its shards.
    pub fn remove(&mut self, blob_id: BlobId) -> bool {
        if let Some(meta) = self.meta.remove(&blob_id) {
            let total_shards = meta.data_shard_count + meta.parity_shard_count;
            for idx in 0..total_shards {
                let key = ShardKey {
                    blob_id,
                    shard_index: idx,
                };
                if let Some(shard) = self.store.remove(&key) {
                    self.total_stored_bytes -= shard.data.len();
                }
            }
            info!("ShardExchange: removed blob {}", blob_id);
            true
        } else {
            warn!("ShardExchange: blob {} not found for removal", blob_id);
            false
        }
    }

    /// Get blob metadata.
    pub fn blob_meta(&self, blob_id: BlobId) -> Option<&BlobMeta> {
        self.meta.get(&blob_id)
    }

    /// Number of blobs stored.
    pub fn blob_count(&self) -> usize {
        self.meta.len()
    }

    /// Total number of shards stored.
    pub fn shard_count(&self) -> usize {
        self.store.len()
    }

    /// Total stored bytes (raw shard data).
    pub fn total_stored_bytes(&self) -> usize {
        self.total_stored_bytes
    }

    /// Set GRG mode (adaptive switching).
    pub fn set_mode(&mut self, mode: GrgMode) {
        self.grg.set_mode(mode);
    }

    /// Current GRG mode.
    pub fn mode(&self) -> GrgMode {
        self.grg.mode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_retrieve_turbo() {
        let mut exchange = ShardExchange::new(GrgMode::Turbo);
        let data = b"Hello, Helm Engine shard exchange!";

        let blob_id = exchange.store(data, 1000).unwrap();
        assert_eq!(blob_id, 1);
        assert_eq!(exchange.blob_count(), 1);
        assert!(exchange.shard_count() > 0);

        let retrieved = exchange.retrieve(blob_id).unwrap();
        assert_eq!(retrieved, data);
    }

    #[test]
    fn store_and_retrieve_safety() {
        let mut exchange = ShardExchange::new(GrgMode::Safety);
        let data: Vec<u8> = (0..200).map(|i| (i % 32) as u8).collect();

        let blob_id = exchange.store(&data, 2000).unwrap();
        let meta = exchange.blob_meta(blob_id).unwrap();
        assert!(meta.data_shard_count > 0);
        assert!(meta.parity_shard_count > 0);

        let retrieved = exchange.retrieve(blob_id).unwrap();
        assert_eq!(retrieved, data);
    }

    #[test]
    fn fetch_individual_shard() {
        let mut exchange = ShardExchange::new(GrgMode::Turbo);
        let data = b"shard test data";
        let blob_id = exchange.store(data, 0).unwrap();

        let key = ShardKey {
            blob_id,
            shard_index: 0,
        };
        let shard = exchange.fetch_shard(&key);
        assert!(shard.is_some());
    }

    #[test]
    fn remove_blob() {
        let mut exchange = ShardExchange::new(GrgMode::Turbo);
        let blob_id = exchange.store(b"remove me", 0).unwrap();

        assert_eq!(exchange.blob_count(), 1);
        assert!(exchange.remove(blob_id));
        assert_eq!(exchange.blob_count(), 0);
        assert_eq!(exchange.shard_count(), 0);
    }

    #[test]
    fn multiple_blobs() {
        let mut exchange = ShardExchange::new(GrgMode::Turbo);

        let id1 = exchange.store(b"blob one", 100).unwrap();
        let id2 = exchange.store(b"blob two", 200).unwrap();
        let id3 = exchange.store(b"blob three", 300).unwrap();

        assert_eq!(exchange.blob_count(), 3);
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);

        assert_eq!(exchange.retrieve(id1).unwrap(), b"blob one");
        assert_eq!(exchange.retrieve(id2).unwrap(), b"blob two");
        assert_eq!(exchange.retrieve(id3).unwrap(), b"blob three");
    }

    #[test]
    fn adaptive_mode_switch() {
        let mut exchange = ShardExchange::new(GrgMode::Turbo);
        assert_eq!(exchange.mode(), GrgMode::Turbo);

        exchange.set_mode(GrgMode::Rescue);
        assert_eq!(exchange.mode(), GrgMode::Rescue);
    }
}
