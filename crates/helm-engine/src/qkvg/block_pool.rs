//! LRU Block Pool — O(1) allocation and deallocation.
//!
//! Intrusive doubly-linked list with sentinel nodes.
//! Freed blocks go to the tail (most recently used),
//! allocation takes from the head (least recently used).

use std::collections::HashMap;
use super::cache_block::GapAwareCacheBlock;

/// LRU-managed pool of GapAwareCacheBlocks.
pub struct BlockPool {
    /// All blocks indexed by block_id
    blocks: HashMap<usize, GapAwareCacheBlock>,
    /// Head sentinel block_id (least recently freed)
    head_sentinel: usize,
    /// Tail sentinel block_id (most recently freed)
    tail_sentinel: usize,
    /// Number of free blocks in the queue
    free_count: usize,
    /// Total blocks in pool
    total_count: usize,
    /// Next block_id to assign
    next_id: usize,
}

impl BlockPool {
    /// Create a new block pool with `capacity` pre-allocated blocks.
    pub fn new(capacity: usize) -> Self {
        let head_sentinel_id = usize::MAX - 1;
        let tail_sentinel_id = usize::MAX;

        let mut blocks = HashMap::with_capacity(capacity + 2);

        // Create sentinel nodes
        let mut head = GapAwareCacheBlock::new(head_sentinel_id);
        head.next_free = Some(tail_sentinel_id);
        head.prev_free = None;

        let mut tail = GapAwareCacheBlock::new(tail_sentinel_id);
        tail.prev_free = Some(head_sentinel_id);
        tail.next_free = None;

        blocks.insert(head_sentinel_id, head);
        blocks.insert(tail_sentinel_id, tail);

        let mut pool = Self {
            blocks,
            head_sentinel: head_sentinel_id,
            tail_sentinel: tail_sentinel_id,
            free_count: 0,
            total_count: 0,
            next_id: 0,
        };

        // Pre-allocate blocks and add to free list
        for _ in 0..capacity {
            let id = pool.next_id;
            pool.next_id += 1;
            let block = GapAwareCacheBlock::new(id);
            pool.blocks.insert(id, block);
            pool.total_count += 1;
            pool.append_free(id);
        }

        pool
    }

    /// Allocate a block from the free pool (LRU — takes from head).
    /// Returns None if no free blocks available.
    /// Sets ref_count = 1 (block is now in use).
    pub fn allocate(&mut self) -> Option<usize> {
        let id = self.popleft()?;
        if let Some(block) = self.blocks.get_mut(&id) {
            block.ref_count = 1;
        }
        Some(id)
    }

    /// Free a block back to the pool (appends to tail — MRU position).
    pub fn free(&mut self, block_id: usize) {
        if let Some(block) = self.blocks.get_mut(&block_id) {
            block.reset();
            // Remove from any current position first
            self.remove_from_free_list(block_id);
            self.append_free(block_id);
        }
    }

    /// Get a reference to a block.
    pub fn get(&self, block_id: usize) -> Option<&GapAwareCacheBlock> {
        self.blocks.get(&block_id).filter(|b| {
            b.block_id != self.head_sentinel && b.block_id != self.tail_sentinel
        })
    }

    /// Get a mutable reference to a block.
    pub fn get_mut(&mut self, block_id: usize) -> Option<&mut GapAwareCacheBlock> {
        let hs = self.head_sentinel;
        let ts = self.tail_sentinel;
        self.blocks.get_mut(&block_id).filter(|b| {
            b.block_id != hs && b.block_id != ts
        })
    }

    /// Touch a block (increment ref_count, remove from free list if present).
    pub fn touch(&mut self, block_id: usize) {
        self.remove_from_free_list(block_id);
        if let Some(block) = self.blocks.get_mut(&block_id) {
            block.touch();
        }
    }

    /// Release a block (decrement ref_count, free if count reaches 0).
    pub fn release(&mut self, block_id: usize) {
        let should_free = self.blocks.get_mut(&block_id)
            .map(|b| b.release())
            .unwrap_or(false);
        if should_free {
            self.append_free(block_id);
        }
    }

    /// Number of free blocks available.
    pub fn free_count(&self) -> usize {
        self.free_count
    }

    /// Total blocks in pool.
    pub fn total_count(&self) -> usize {
        self.total_count
    }

    /// Number of active (in-use) blocks.
    pub fn active_count(&self) -> usize {
        self.total_count - self.free_count
    }

    /// Pop the least-recently-freed block from head.
    fn popleft(&mut self) -> Option<usize> {
        if self.free_count == 0 {
            return None;
        }

        // Get the first free block (after head sentinel)
        let first_free = self.blocks.get(&self.head_sentinel)?.next_free?;
        if first_free == self.tail_sentinel {
            return None;
        }

        self.remove_from_free_list(first_free);
        Some(first_free)
    }

    /// Append a block to the tail of the free list.
    fn append_free(&mut self, block_id: usize) {
        // Get current tail's prev
        let prev_of_tail = self.blocks.get(&self.tail_sentinel)
            .and_then(|t| t.prev_free)
            .unwrap_or(self.head_sentinel);

        // Link: prev_of_tail <-> block_id <-> tail_sentinel
        if let Some(prev_block) = self.blocks.get_mut(&prev_of_tail) {
            prev_block.next_free = Some(block_id);
        }
        if let Some(block) = self.blocks.get_mut(&block_id) {
            block.prev_free = Some(prev_of_tail);
            block.next_free = Some(self.tail_sentinel);
        }
        if let Some(tail) = self.blocks.get_mut(&self.tail_sentinel) {
            tail.prev_free = Some(block_id);
        }

        self.free_count += 1;
    }

    /// Remove a block from the free list (O(1) with intrusive pointers).
    fn remove_from_free_list(&mut self, block_id: usize) {
        let (prev, next) = match self.blocks.get(&block_id) {
            Some(block) => match (block.prev_free, block.next_free) {
                (Some(p), Some(n)) => (p, n),
                _ => return, // Not in free list
            },
            None => return,
        };

        // Unlink: prev <-> next
        if let Some(prev_block) = self.blocks.get_mut(&prev) {
            prev_block.next_free = Some(next);
        }
        if let Some(next_block) = self.blocks.get_mut(&next) {
            next_block.prev_free = Some(prev);
        }
        if let Some(block) = self.blocks.get_mut(&block_id) {
            block.prev_free = None;
            block.next_free = None;
        }

        self.free_count -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_creation() {
        let pool = BlockPool::new(10);
        assert_eq!(pool.total_count(), 10);
        assert_eq!(pool.free_count(), 10);
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn allocate_and_free() {
        let mut pool = BlockPool::new(3);

        let b0 = pool.allocate().unwrap();
        assert_eq!(pool.free_count(), 2);
        assert_eq!(pool.active_count(), 1);

        let b1 = pool.allocate().unwrap();
        let b2 = pool.allocate().unwrap();
        assert_eq!(pool.free_count(), 0);
        assert!(pool.allocate().is_none());

        pool.free(b1);
        assert_eq!(pool.free_count(), 1);

        let b3 = pool.allocate().unwrap();
        assert_eq!(b3, b1); // Reused
        assert_eq!(pool.free_count(), 0);

        pool.free(b0);
        pool.free(b2);
        pool.free(b3);
        assert_eq!(pool.free_count(), 3);
    }

    #[test]
    fn lru_order() {
        let mut pool = BlockPool::new(3);

        // Allocate all
        let b0 = pool.allocate().unwrap();
        let b1 = pool.allocate().unwrap();
        let b2 = pool.allocate().unwrap();

        // Free in order: b2, b0, b1
        pool.free(b2);
        pool.free(b0);
        pool.free(b1);

        // Allocate should return in LRU order: b2 (freed first), b0, b1
        assert_eq!(pool.allocate().unwrap(), b2);
        assert_eq!(pool.allocate().unwrap(), b0);
        assert_eq!(pool.allocate().unwrap(), b1);
    }

    #[test]
    fn touch_prevents_reuse() {
        let mut pool = BlockPool::new(2);
        let b0 = pool.allocate().unwrap();
        let _b1 = pool.allocate().unwrap();

        // Touch b0 (increment ref_count)
        pool.touch(b0);
        // Free b0 — ref_count = 1, shouldn't go to free list
        pool.release(b0);
        assert_eq!(pool.free_count(), 0);

        // Release again — ref_count = 0, now freed
        pool.release(b0);
        assert_eq!(pool.free_count(), 1);
    }

    #[test]
    fn get_block() {
        let mut pool = BlockPool::new(2);
        let b0 = pool.allocate().unwrap();

        let block = pool.get(b0).unwrap();
        assert_eq!(block.block_id, b0);

        let block = pool.get_mut(b0).unwrap();
        block.write_kv(vec![1.0; 128], vec![2.0; 128]);
        assert_eq!(pool.get(b0).unwrap().filled_slots, 1);
    }
}
