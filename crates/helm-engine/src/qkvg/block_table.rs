//! Block Table — logical-to-physical page mapping.
//!
//! Maps logical block indices (per-request, contiguous) to
//! physical block IDs (scattered in GPU/memory pool).
//! Equivalent to an OS page table for KV cache.

use std::collections::HashMap;

/// Maps a request's logical blocks to physical blocks in the pool.
#[derive(Debug, Clone)]
pub struct BlockTable {
    /// Request/sequence identifier
    pub sequence_id: u64,
    /// Logical block index → Physical block ID
    mapping: Vec<Option<usize>>,
    /// Reverse map for quick physical → logical lookup
    reverse: HashMap<usize, usize>,
}

impl BlockTable {
    /// Create a new block table for a sequence.
    pub fn new(sequence_id: u64) -> Self {
        Self {
            sequence_id,
            mapping: Vec::new(),
            reverse: HashMap::new(),
        }
    }

    /// Map a logical block to a physical block.
    pub fn map_block(&mut self, logical_idx: usize, physical_id: usize) {
        if logical_idx >= self.mapping.len() {
            self.mapping.resize(logical_idx + 1, None);
        }
        // Remove old reverse mapping if any
        if let Some(old_phys) = self.mapping[logical_idx] {
            self.reverse.remove(&old_phys);
        }
        self.mapping[logical_idx] = Some(physical_id);
        self.reverse.insert(physical_id, logical_idx);
    }

    /// Resolve a token position to (physical_block_id, slot_offset).
    pub fn resolve_token(&self, token_pos: usize, block_size: usize) -> Option<(usize, usize)> {
        let logical_idx = token_pos / block_size;
        let slot_offset = token_pos % block_size;
        let physical_id = self.mapping.get(logical_idx).copied().flatten()?;
        Some((physical_id, slot_offset))
    }

    /// Get all physical block IDs in logical order.
    pub fn physical_blocks(&self) -> Vec<usize> {
        self.mapping.iter().filter_map(|&b| b).collect()
    }

    /// Get the physical block ID for a logical index.
    pub fn get_physical(&self, logical_idx: usize) -> Option<usize> {
        self.mapping.get(logical_idx).copied().flatten()
    }

    /// Number of logical blocks allocated.
    pub fn len(&self) -> usize {
        self.mapping.iter().filter(|b| b.is_some()).count()
    }

    /// Check if the table is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove a logical block mapping. Returns the physical ID.
    pub fn unmap_block(&mut self, logical_idx: usize) -> Option<usize> {
        if logical_idx >= self.mapping.len() {
            return None;
        }
        let physical_id = self.mapping[logical_idx].take()?;
        self.reverse.remove(&physical_id);
        Some(physical_id)
    }

    /// Find the logical index for a physical block.
    pub fn logical_index_of(&self, physical_id: usize) -> Option<usize> {
        self.reverse.get(&physical_id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qkvg::cache_block::BLOCK_SIZE;

    #[test]
    fn map_and_resolve() {
        let mut table = BlockTable::new(1);
        table.map_block(0, 42); // Logical 0 → Physical 42
        table.map_block(1, 17); // Logical 1 → Physical 17

        // Token 0 → block 42, slot 0
        assert_eq!(table.resolve_token(0, BLOCK_SIZE), Some((42, 0)));
        // Token 15 → block 42, slot 15
        assert_eq!(table.resolve_token(15, BLOCK_SIZE), Some((42, 15)));
        // Token 16 → block 17, slot 0
        assert_eq!(table.resolve_token(16, BLOCK_SIZE), Some((17, 0)));
        // Token 31 → block 17, slot 15
        assert_eq!(table.resolve_token(31, BLOCK_SIZE), Some((17, 15)));
    }

    #[test]
    fn physical_blocks_ordered() {
        let mut table = BlockTable::new(1);
        table.map_block(0, 5);
        table.map_block(1, 3);
        table.map_block(2, 9);
        assert_eq!(table.physical_blocks(), vec![5, 3, 9]);
    }

    #[test]
    fn unmap_block() {
        let mut table = BlockTable::new(1);
        table.map_block(0, 42);
        assert_eq!(table.len(), 1);

        let freed = table.unmap_block(0);
        assert_eq!(freed, Some(42));
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn reverse_lookup() {
        let mut table = BlockTable::new(1);
        table.map_block(0, 42);
        table.map_block(1, 17);

        assert_eq!(table.logical_index_of(42), Some(0));
        assert_eq!(table.logical_index_of(17), Some(1));
        assert_eq!(table.logical_index_of(99), None);
    }
}
