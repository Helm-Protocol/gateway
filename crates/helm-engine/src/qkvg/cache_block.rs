//! GapAwareCacheBlock — physical memory block with G-metric awareness.
//!
//! Extends vLLM's KVCacheBlock concept with:
//! - `orthogonality_score`: How orthogonal this block is to the current query
//! - `is_void_block`: Ghost block marking a knowledge gap (triggers questioning)

use serde::{Serialize, Deserialize};

/// Dimension of each key/value vector.
pub const HEAD_DIM: usize = 128;

/// Number of token slots per block (vLLM default: 16).
pub const BLOCK_SIZE: usize = 16;

/// A single key or value vector.
pub type Vector = Vec<f32>;

/// Physical memory block storing KV pairs with gap awareness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapAwareCacheBlock {
    /// Unique block identifier
    pub block_id: usize,

    /// Key vectors stored in this block [BLOCK_SIZE × HEAD_DIM]
    pub k_cache: Vec<Vector>,

    /// Value vectors stored in this block [BLOCK_SIZE × HEAD_DIM]
    pub v_cache: Vec<Vector>,

    /// Number of filled token slots (0..BLOCK_SIZE)
    pub filled_slots: usize,

    /// Reference count for copy-on-write sharing (prefix caching)
    pub ref_count: usize,

    /// G-metric: orthogonality score relative to last query.
    /// 0.0 = perfect match, 1.0 = completely orthogonal (unknown)
    pub orthogonality_score: f32,

    /// Void block flag: this block represents a knowledge gap.
    /// Data fields are empty; existence triggers question generation.
    pub is_void_block: bool,

    /// Pointer for intrusive doubly-linked free list (prev block ID)
    pub prev_free: Option<usize>,

    /// Pointer for intrusive doubly-linked free list (next block ID)
    pub next_free: Option<usize>,

    /// Content hash for prefix matching / deduplication
    pub content_hash: Option<u64>,
}

impl GapAwareCacheBlock {
    /// Create a new empty block.
    pub fn new(block_id: usize) -> Self {
        Self {
            block_id,
            k_cache: vec![vec![0.0; HEAD_DIM]; BLOCK_SIZE],
            v_cache: vec![vec![0.0; HEAD_DIM]; BLOCK_SIZE],
            filled_slots: 0,
            ref_count: 0,
            orthogonality_score: 1.0, // Initially unknown
            is_void_block: false,
            prev_free: None,
            next_free: None,
            content_hash: None,
        }
    }

    /// Create a void (ghost) block representing a knowledge gap.
    pub fn new_void(block_id: usize) -> Self {
        Self {
            block_id,
            k_cache: Vec::new(),
            v_cache: Vec::new(),
            filled_slots: 0,
            ref_count: 1,
            orthogonality_score: 1.0,
            is_void_block: true,
            prev_free: None,
            next_free: None,
            content_hash: None,
        }
    }

    /// Write a KV pair into the next available slot.
    /// Returns the slot index, or None if the block is full.
    pub fn write_kv(&mut self, key: Vector, value: Vector) -> Option<usize> {
        if self.filled_slots >= BLOCK_SIZE || self.is_void_block {
            return None;
        }
        let slot = self.filled_slots;
        self.k_cache[slot] = key;
        self.v_cache[slot] = value;
        self.filled_slots += 1;
        self.content_hash = None; // Invalidate hash
        Some(slot)
    }

    /// Read the KV pair at a given slot.
    pub fn read_kv(&self, slot: usize) -> Option<(&Vector, &Vector)> {
        if slot >= self.filled_slots {
            return None;
        }
        Some((&self.k_cache[slot], &self.v_cache[slot]))
    }

    /// Check if block is full.
    pub fn is_full(&self) -> bool {
        self.filled_slots >= BLOCK_SIZE
    }

    /// Check if block is empty.
    pub fn is_empty(&self) -> bool {
        self.filled_slots == 0
    }

    /// Increment reference count (block sharing).
    pub fn touch(&mut self) {
        self.ref_count += 1;
    }

    /// Decrement reference count. Returns true if block can be freed.
    pub fn release(&mut self) -> bool {
        self.ref_count = self.ref_count.saturating_sub(1);
        self.ref_count == 0
    }

    /// Reset block for reuse.
    pub fn reset(&mut self) {
        self.filled_slots = 0;
        self.ref_count = 0;
        self.orthogonality_score = 1.0;
        self.is_void_block = false;
        self.content_hash = None;
        // Zero out caches
        for slot in &mut self.k_cache {
            slot.fill(0.0);
        }
        for slot in &mut self.v_cache {
            slot.fill(0.0);
        }
    }

    /// Compute a simple content hash for prefix matching.
    pub fn compute_hash(&mut self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64; // FNV offset basis
        for slot in 0..self.filled_slots {
            for &v in &self.k_cache[slot] {
                hash ^= v.to_bits() as u64;
                hash = hash.wrapping_mul(0x100000001b3); // FNV prime
            }
        }
        self.content_hash = Some(hash);
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_block_empty() {
        let block = GapAwareCacheBlock::new(0);
        assert!(block.is_empty());
        assert!(!block.is_full());
        assert_eq!(block.orthogonality_score, 1.0);
        assert!(!block.is_void_block);
    }

    #[test]
    fn write_and_read_kv() {
        let mut block = GapAwareCacheBlock::new(0);
        let key = vec![1.0; HEAD_DIM];
        let value = vec![2.0; HEAD_DIM];

        let slot = block.write_kv(key.clone(), value.clone()).unwrap();
        assert_eq!(slot, 0);
        assert_eq!(block.filled_slots, 1);

        let (k, v) = block.read_kv(0).unwrap();
        assert_eq!(*k, key);
        assert_eq!(*v, value);
    }

    #[test]
    fn block_fills_up() {
        let mut block = GapAwareCacheBlock::new(0);
        for i in 0..BLOCK_SIZE {
            let slot = block.write_kv(vec![i as f32; HEAD_DIM], vec![0.0; HEAD_DIM]);
            assert!(slot.is_some());
        }
        assert!(block.is_full());
        assert!(block.write_kv(vec![0.0; HEAD_DIM], vec![0.0; HEAD_DIM]).is_none());
    }

    #[test]
    fn void_block_rejects_writes() {
        let mut block = GapAwareCacheBlock::new_void(99);
        assert!(block.is_void_block);
        assert!(block.write_kv(vec![0.0; HEAD_DIM], vec![0.0; HEAD_DIM]).is_none());
    }

    #[test]
    fn ref_counting() {
        let mut block = GapAwareCacheBlock::new(0);
        block.touch();
        block.touch();
        assert_eq!(block.ref_count, 2);
        assert!(!block.release());
        assert!(block.release());
    }

    #[test]
    fn reset_clears_state() {
        let mut block = GapAwareCacheBlock::new(0);
        block.write_kv(vec![1.0; HEAD_DIM], vec![2.0; HEAD_DIM]);
        block.orthogonality_score = 0.5;
        block.reset();
        assert!(block.is_empty());
        assert_eq!(block.orthogonality_score, 1.0);
    }
}
