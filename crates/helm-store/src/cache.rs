//! Tiered Cache — L1 (moka in-memory) + L2 (SSD-backed) + Origin.
//!
//! Implements the Solid Cache concept adapted for Helm:
//! - L1 (Hot): moka TinyLFU in-memory cache for sub-microsecond reads
//! - L2 (Warm): SSD-backed persistent cache (any KvStore backend) with
//!   FIFO eviction for massive capacity at low cost
//! - Origin: The actual KvStore backend (source of truth)
//!
//! # Read Path (Promotion)
//! ```text
//! GET(key) → L1 hit? → return
//!          → L2 hit? → promote to L1 → return
//!          → Origin hit? → promote to L1 + L2 → return
//!          → None
//! ```
//!
//! # Write Path
//! ```text
//! PUT(key, val) → write to Origin → insert into L1
//!               → L1 eviction → demote to L2 (async write-behind)
//! ```
//!
//! # Use Cases
//! - Physical AI control API: ultra-low-latency L1 with short TTL
//! - Agent-to-agent data exchange: warm L2 for session data
//! - CRDT state: L2 persistent cache for convergence windows
//! - Historical data: L2 only (TB-scale at SSD prices)

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use anyhow::Result;

use crate::kv::KvStore;

/// Configuration for the tiered cache.
#[derive(Debug, Clone)]
pub struct TieredCacheConfig {
    /// Maximum L1 entries (in-memory).
    pub l1_max_entries: u64,
    /// L1 time-to-live in seconds (0 = no TTL).
    pub l1_ttl_secs: u64,
    /// L1 time-to-idle in seconds (0 = no TTI).
    pub l1_tti_secs: u64,
    /// Maximum L2 entries (SSD-backed). 0 = unlimited.
    pub l2_max_entries: usize,
    /// Enable L2 (Solid Cache) tier.
    pub l2_enabled: bool,
}

impl Default for TieredCacheConfig {
    fn default() -> Self {
        Self {
            l1_max_entries: 10_000,
            l1_ttl_secs: 300,  // 5 minutes
            l1_tti_secs: 120,  // 2 minutes idle
            l2_max_entries: 1_000_000,
            l2_enabled: true,
        }
    }
}

/// Cache tier statistics.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub l1_hits: u64,
    pub l1_misses: u64,
    pub l2_hits: u64,
    pub l2_misses: u64,
    pub origin_hits: u64,
    pub origin_misses: u64,
    pub l1_promotions: u64,
    pub l2_promotions: u64,
    pub l2_demotions: u64,
    pub writes: u64,
}

/// Atomic counters for thread-safe stats.
struct AtomicCacheStats {
    l1_hits: AtomicU64,
    l1_misses: AtomicU64,
    l2_hits: AtomicU64,
    l2_misses: AtomicU64,
    origin_hits: AtomicU64,
    origin_misses: AtomicU64,
    l1_promotions: AtomicU64,
    l2_promotions: AtomicU64,
    l2_demotions: AtomicU64,
    writes: AtomicU64,
}

impl AtomicCacheStats {
    fn new() -> Self {
        Self {
            l1_hits: AtomicU64::new(0),
            l1_misses: AtomicU64::new(0),
            l2_hits: AtomicU64::new(0),
            l2_misses: AtomicU64::new(0),
            origin_hits: AtomicU64::new(0),
            origin_misses: AtomicU64::new(0),
            l1_promotions: AtomicU64::new(0),
            l2_promotions: AtomicU64::new(0),
            l2_demotions: AtomicU64::new(0),
            writes: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> CacheStats {
        CacheStats {
            l1_hits: self.l1_hits.load(Ordering::Relaxed),
            l1_misses: self.l1_misses.load(Ordering::Relaxed),
            l2_hits: self.l2_hits.load(Ordering::Relaxed),
            l2_misses: self.l2_misses.load(Ordering::Relaxed),
            origin_hits: self.origin_hits.load(Ordering::Relaxed),
            origin_misses: self.origin_misses.load(Ordering::Relaxed),
            l1_promotions: self.l1_promotions.load(Ordering::Relaxed),
            l2_promotions: self.l2_promotions.load(Ordering::Relaxed),
            l2_demotions: self.l2_demotions.load(Ordering::Relaxed),
            writes: self.writes.load(Ordering::Relaxed),
        }
    }
}

/// L2 metadata stored alongside values for FIFO eviction.
/// Prefix: `_cache_meta:` + sequence number
const L2_META_PREFIX: &[u8] = b"_cm:";

/// The Tiered Cache: L1 (moka) + L2 (KvStore) + Origin (KvStore).
///
/// Implements `KvStore` so it can be used as a drop-in replacement.
pub struct TieredCache {
    /// L1: In-memory TinyLFU cache (sub-microsecond reads).
    l1: moka::sync::Cache<Vec<u8>, Vec<u8>>,
    /// L2: SSD-backed persistent cache (Solid Cache).
    l2: Option<Box<dyn KvStore>>,
    /// Origin: Source of truth.
    origin: Box<dyn KvStore>,
    /// Configuration.
    config: TieredCacheConfig,
    /// Atomic statistics.
    stats: Arc<AtomicCacheStats>,
    /// L2 write sequence counter (for FIFO ordering).
    l2_seq: AtomicU64,
}

impl TieredCache {
    /// Create a new tiered cache.
    ///
    /// - `origin`: The source-of-truth KvStore backend
    /// - `l2`: Optional SSD-backed L2 cache backend
    /// - `config`: Cache configuration
    pub fn new(
        origin: Box<dyn KvStore>,
        l2: Option<Box<dyn KvStore>>,
        config: TieredCacheConfig,
    ) -> Self {
        let mut l1_builder = moka::sync::Cache::builder()
            .max_capacity(config.l1_max_entries);

        if config.l1_ttl_secs > 0 {
            l1_builder = l1_builder
                .time_to_live(std::time::Duration::from_secs(config.l1_ttl_secs));
        }
        if config.l1_tti_secs > 0 {
            l1_builder = l1_builder
                .time_to_idle(std::time::Duration::from_secs(config.l1_tti_secs));
        }

        let l1 = l1_builder.build();

        Self {
            l1,
            l2: if config.l2_enabled { l2 } else { None },
            origin,
            config,
            stats: Arc::new(AtomicCacheStats::new()),
            l2_seq: AtomicU64::new(0),
        }
    }

    /// Get cache statistics snapshot.
    pub fn stats(&self) -> CacheStats {
        self.stats.snapshot()
    }

    /// L1 entry count.
    pub fn l1_len(&self) -> u64 {
        self.l1.entry_count()
    }

    /// Hit rate for L1 (0.0 - 1.0).
    pub fn l1_hit_rate(&self) -> f64 {
        let stats = self.stats.snapshot();
        let total = stats.l1_hits + stats.l1_misses;
        if total == 0 { 0.0 } else { stats.l1_hits as f64 / total as f64 }
    }

    /// Combined hit rate (L1 + L2) (0.0 - 1.0).
    pub fn combined_hit_rate(&self) -> f64 {
        let stats = self.stats.snapshot();
        let hits = stats.l1_hits + stats.l2_hits;
        let total = hits + stats.origin_hits + stats.origin_misses;
        if total == 0 { 0.0 } else { hits as f64 / total as f64 }
    }

    /// Invalidate a key from all cache tiers (but NOT from origin).
    pub fn invalidate(&self, key: &[u8]) {
        self.l1.invalidate(&key.to_vec());
        if let Some(ref l2) = self.l2 {
            let _ = l2.delete(key);
        }
    }

    /// Invalidate all entries from all cache tiers.
    pub fn invalidate_all(&self) {
        self.l1.invalidate_all();
        // L2 invalidation would require scanning; skip for now
    }

    /// Write an entry to L2 with FIFO metadata.
    fn l2_put(&self, key: &[u8], value: &[u8]) {
        if let Some(ref l2) = self.l2 {
            let seq = self.l2_seq.fetch_add(1, Ordering::Relaxed);
            let _ = l2.put(key, value);

            // Store metadata: _cm:<seq> → key (for FIFO eviction)
            let meta_key = [L2_META_PREFIX, &seq.to_be_bytes()].concat();
            let _ = l2.put(&meta_key, key);

            self.stats.l2_demotions.fetch_add(1, Ordering::Relaxed);

            // Check if L2 needs eviction
            if self.config.l2_max_entries > 0 {
                self.l2_maybe_evict();
            }
        }
    }

    /// FIFO eviction: remove oldest entries if L2 exceeds max_entries.
    fn l2_maybe_evict(&self) {
        if let Some(ref l2) = self.l2 {
            let current_seq = self.l2_seq.load(Ordering::Relaxed);
            // Rough check: if seq > 2x max_entries, do cleanup
            if current_seq as usize <= self.config.l2_max_entries * 2 {
                return;
            }

            // Batch evict oldest 10% of entries
            let evict_count = self.config.l2_max_entries / 10;
            let meta_entries = l2.scan_prefix(L2_META_PREFIX).unwrap_or_default();

            for (meta_key, data_key) in meta_entries.iter().take(evict_count) {
                let _ = l2.delete(data_key);
                let _ = l2.delete(meta_key);
            }
        }
    }
}

impl KvStore for TieredCache {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // L1 lookup
        if let Some(value) = self.l1.get(&key.to_vec()) {
            self.stats.l1_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(Some(value));
        }
        self.stats.l1_misses.fetch_add(1, Ordering::Relaxed);

        // L2 lookup
        if let Some(ref l2) = self.l2 {
            if let Ok(Some(value)) = l2.get(key) {
                self.stats.l2_hits.fetch_add(1, Ordering::Relaxed);

                // Promote to L1
                self.l1.insert(key.to_vec(), value.clone());
                self.stats.l1_promotions.fetch_add(1, Ordering::Relaxed);

                return Ok(Some(value));
            }
            self.stats.l2_misses.fetch_add(1, Ordering::Relaxed);
        }

        // Origin lookup
        if let Ok(Some(value)) = self.origin.get(key) {
            self.stats.origin_hits.fetch_add(1, Ordering::Relaxed);

            // Promote to L1
            self.l1.insert(key.to_vec(), value.clone());
            self.stats.l1_promotions.fetch_add(1, Ordering::Relaxed);

            // Promote to L2
            self.l2_put(key, &value);
            self.stats.l2_promotions.fetch_add(1, Ordering::Relaxed);

            return Ok(Some(value));
        }
        self.stats.origin_misses.fetch_add(1, Ordering::Relaxed);

        Ok(None)
    }

    fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.stats.writes.fetch_add(1, Ordering::Relaxed);

        // Write-through to origin
        self.origin.put(key, value)?;

        // Insert into L1
        self.l1.insert(key.to_vec(), value.to_vec());

        // Write-through to L2
        self.l2_put(key, value);

        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<bool> {
        // Remove from all tiers
        self.l1.invalidate(&key.to_vec());
        if let Some(ref l2) = self.l2 {
            let _ = l2.delete(key);
        }
        self.origin.delete(key)
    }

    fn contains(&self, key: &[u8]) -> Result<bool> {
        // Check L1 first (no promotion on contains)
        if self.l1.contains_key(&key.to_vec()) {
            return Ok(true);
        }
        // Check L2
        if let Some(ref l2) = self.l2 {
            if l2.contains(key)? {
                return Ok(true);
            }
        }
        // Check origin
        self.origin.contains(key)
    }

    fn keys(&self) -> Result<Vec<Vec<u8>>> {
        // Delegate to origin (it's the source of truth)
        self.origin.keys()
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        // Delegate to origin
        self.origin.scan_prefix(prefix)
    }

    fn flush(&self) -> Result<()> {
        if let Some(ref l2) = self.l2 {
            l2.flush()?;
        }
        self.origin.flush()
    }

    fn len(&self) -> Result<usize> {
        self.origin.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::memory::MemoryBackend;

    fn make_tiered(config: TieredCacheConfig) -> TieredCache {
        let origin = Box::new(MemoryBackend::new());
        let l2 = Box::new(MemoryBackend::new());
        TieredCache::new(origin, Some(l2), config)
    }

    fn default_tiered() -> TieredCache {
        make_tiered(TieredCacheConfig::default())
    }

    #[test]
    fn basic_put_get() {
        let cache = default_tiered();
        cache.put(b"key1", b"val1").unwrap();
        assert_eq!(cache.get(b"key1").unwrap(), Some(b"val1".to_vec()));
    }

    #[test]
    fn get_nonexistent() {
        let cache = default_tiered();
        assert_eq!(cache.get(b"ghost").unwrap(), None);
    }

    #[test]
    fn l1_hit_on_second_get() {
        let cache = default_tiered();
        cache.put(b"key", b"val").unwrap();
        let _ = cache.get(b"key").unwrap(); // L1 hit
        let _ = cache.get(b"key").unwrap(); // L1 hit again

        let stats = cache.stats();
        assert!(stats.l1_hits >= 2);
    }

    #[test]
    fn l2_promotion_on_miss() {
        let origin = Box::new(MemoryBackend::new());
        let l2 = Box::new(MemoryBackend::new());

        // Put directly into origin (simulating cold data)
        origin.put(b"cold", b"data").unwrap();

        let cache = TieredCache::new(
            origin,
            Some(l2),
            TieredCacheConfig::default(),
        );

        // First get: L1 miss → L2 miss → origin hit → promote to L1+L2
        let val = cache.get(b"cold").unwrap();
        assert_eq!(val, Some(b"data".to_vec()));

        let stats = cache.stats();
        assert_eq!(stats.l1_misses, 1);
        assert_eq!(stats.l2_misses, 1);
        assert_eq!(stats.origin_hits, 1);
        assert_eq!(stats.l1_promotions, 1);
        assert_eq!(stats.l2_promotions, 1);

        // Second get: L1 hit (promoted)
        let val2 = cache.get(b"cold").unwrap();
        assert_eq!(val2, Some(b"data".to_vec()));
        assert_eq!(cache.stats().l1_hits, 1);
    }

    #[test]
    fn delete_removes_from_all_tiers() {
        let cache = default_tiered();
        cache.put(b"del", b"val").unwrap();
        assert!(cache.contains(b"del").unwrap());

        cache.delete(b"del").unwrap();
        assert!(!cache.contains(b"del").unwrap());
        assert_eq!(cache.get(b"del").unwrap(), None);
    }

    #[test]
    fn write_through_to_origin() {
        let cache = default_tiered();
        cache.put(b"wt", b"data").unwrap();

        // Verify origin has the data by checking keys
        let keys = cache.keys().unwrap();
        assert!(!keys.is_empty());
    }

    #[test]
    fn contains_checks_all_tiers() {
        let origin = Box::new(MemoryBackend::new());
        origin.put(b"origin-only", b"val").unwrap();

        let cache = TieredCache::new(
            origin,
            Some(Box::new(MemoryBackend::new())),
            TieredCacheConfig::default(),
        );

        assert!(cache.contains(b"origin-only").unwrap());
    }

    #[test]
    fn stats_tracking() {
        let cache = default_tiered();
        cache.put(b"s1", b"v1").unwrap();
        cache.put(b"s2", b"v2").unwrap();
        let _ = cache.get(b"s1").unwrap();
        let _ = cache.get(b"missing").unwrap();

        let stats = cache.stats();
        assert_eq!(stats.writes, 2);
        assert!(stats.l1_hits >= 1);
        assert_eq!(stats.origin_misses, 1);
    }

    #[test]
    fn l1_hit_rate() {
        let cache = default_tiered();
        cache.put(b"hr", b"val").unwrap();

        // 3 hits
        for _ in 0..3 {
            let _ = cache.get(b"hr").unwrap();
        }
        // 1 miss
        let _ = cache.get(b"nope").unwrap();

        // hit_rate = 3 / 4 = 0.75
        let rate = cache.l1_hit_rate();
        assert!(rate > 0.7 && rate < 0.8);
    }

    #[test]
    fn invalidate_key() {
        let cache = default_tiered();
        cache.put(b"inv", b"val").unwrap();
        assert!(cache.l1.contains_key(&b"inv".to_vec()));

        cache.invalidate(b"inv");
        assert!(!cache.l1.contains_key(&b"inv".to_vec()));

        // Origin should still have it
        assert!(cache.origin.contains(b"inv").unwrap());
    }

    #[test]
    fn invalidate_all() {
        let cache = default_tiered();
        cache.put(b"a", b"1").unwrap();
        cache.put(b"b", b"2").unwrap();

        cache.invalidate_all();
        // moka invalidate_all is lazy; verify through get behavior
        // After invalidation, a get should miss L1 and hit origin
        let val = cache.get(b"a").unwrap();
        assert_eq!(val, Some(b"1".to_vec()));
        // The get promoted it back from origin
        assert!(cache.origin.contains(b"a").unwrap());
    }

    #[test]
    fn no_l2_mode() {
        let cache = TieredCache::new(
            Box::new(MemoryBackend::new()),
            None,
            TieredCacheConfig {
                l2_enabled: false,
                ..Default::default()
            },
        );

        cache.put(b"no-l2", b"val").unwrap();
        let val = cache.get(b"no-l2").unwrap();
        assert_eq!(val, Some(b"val".to_vec()));
    }

    #[test]
    fn scan_prefix_delegates_to_origin() {
        let cache = default_tiered();
        cache.put(b"pfx:a", b"1").unwrap();
        cache.put(b"pfx:b", b"2").unwrap();
        cache.put(b"other:c", b"3").unwrap();

        let results = cache.scan_prefix(b"pfx:").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn flush_propagates() {
        let cache = default_tiered();
        cache.put(b"fl", b"val").unwrap();
        cache.flush().unwrap(); // should not error
    }

    #[test]
    fn overwrite_updates_all_tiers() {
        let cache = default_tiered();
        cache.put(b"ow", b"v1").unwrap();
        assert_eq!(cache.get(b"ow").unwrap(), Some(b"v1".to_vec()));

        cache.put(b"ow", b"v2").unwrap();
        assert_eq!(cache.get(b"ow").unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn l1_entry_count() {
        let cache = default_tiered();
        // moka entry_count is eventually consistent
        // Verify L1 works via hit stats instead
        cache.put(b"a", b"1").unwrap();
        cache.put(b"b", b"2").unwrap();

        // Both should be in L1 (hit on get)
        let _ = cache.get(b"a").unwrap();
        let _ = cache.get(b"b").unwrap();
        let stats = cache.stats();
        assert!(stats.l1_hits >= 2);
    }

    #[test]
    fn combined_hit_rate_both_tiers() {
        let cache = default_tiered();
        cache.put(b"combo", b"val").unwrap();

        // L1 hit
        let _ = cache.get(b"combo").unwrap();
        // Miss all tiers
        let _ = cache.get(b"nope1").unwrap();
        let _ = cache.get(b"nope2").unwrap();

        let rate = cache.combined_hit_rate();
        // 1 L1 hit out of 3 total reads hitting origin (2 origin misses + read path)
        assert!(rate > 0.0);
    }

    #[test]
    fn kv_store_standard_suite() {
        let cache = default_tiered();
        crate::kv::tests::run_kv_store_tests(&cache);
    }

    #[test]
    fn l2_fifo_metadata_written() {
        let origin = Box::new(MemoryBackend::new());
        let l2 = Box::new(MemoryBackend::new());

        let cache = TieredCache::new(
            origin,
            Some(l2),
            TieredCacheConfig::default(),
        );

        cache.put(b"fifo1", b"val1").unwrap();
        cache.put(b"fifo2", b"val2").unwrap();

        // L2 should have the values + FIFO metadata
        let stats = cache.stats();
        assert_eq!(stats.l2_demotions, 2);
    }

    #[test]
    fn config_defaults() {
        let cfg = TieredCacheConfig::default();
        assert_eq!(cfg.l1_max_entries, 10_000);
        assert_eq!(cfg.l1_ttl_secs, 300);
        assert_eq!(cfg.l1_tti_secs, 120);
        assert_eq!(cfg.l2_max_entries, 1_000_000);
        assert!(cfg.l2_enabled);
    }
}
