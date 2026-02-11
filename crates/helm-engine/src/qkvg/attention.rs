//! Gap-Aware Paged Attention — the core QKV-G kernel.
//!
//! Performs attention computation with G-metric (orthogonality) checking.
//! When the G-metric exceeds a threshold, computation halts early and
//! a GapDetected signal is returned instead of a hallucinated answer.
//!
//! Two planes:
//! - Data Plane: Standard KV lookup for shard exchange (O(1))
//! - Control Plane: Full QKV-G attention for security/routing/anomaly

use serde::{Serialize, Deserialize};
use tracing::{info, warn};

use super::cache_block::{Vector, HEAD_DIM, BLOCK_SIZE};
use super::block_pool::BlockPool;
use super::block_table::BlockTable;

/// G-metric threshold. Above this value → knowledge gap detected.
pub const G_THRESHOLD: f32 = 0.4;

/// Result of attention computation.
#[derive(Debug, Clone)]
pub enum AttentionOutput {
    /// Normal output — context was found, weighted value produced.
    Success {
        /// The attention-weighted output vector
        output: Vector,
        /// Maximum similarity score found
        max_similarity: f32,
        /// G-metric (0.0 = perfect match, 1.0 = fully orthogonal)
        g_metric: f32,
    },
    /// Knowledge gap detected — no sufficient context exists.
    GapDetected {
        /// G-metric score (above threshold)
        g_metric: f32,
        /// The query vector that couldn't be matched (material for question generation)
        missing_intent: Vector,
        /// Closest block ID (if any partial match exists)
        closest_block: Option<usize>,
    },
}

/// Alert from the hidden core API when anomaly detected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyAlert {
    /// Source identifier (peer, agent, etc.)
    pub source: String,
    /// G-metric value (high = anomalous)
    pub g_metric: f32,
    /// Description of the anomaly
    pub description: String,
    /// Severity: 0.0 = info, 0.5 = warning, 1.0 = critical
    pub severity: f32,
}

/// The Helm Attention Engine.
///
/// Manages block pools, block tables, and performs gap-aware attention.
pub struct HelmAttentionEngine {
    /// Physical block pool with LRU management
    pool: BlockPool,
    /// Per-sequence block tables
    tables: Vec<BlockTable>,
    /// G-metric threshold (configurable)
    g_threshold: f32,
    /// Anomaly history for control plane
    anomaly_log: Vec<AnomalyAlert>,
}

impl HelmAttentionEngine {
    /// Create a new engine with the given block pool capacity.
    pub fn new(pool_capacity: usize) -> Self {
        Self {
            pool: BlockPool::new(pool_capacity),
            tables: Vec::new(),
            g_threshold: G_THRESHOLD,
            anomaly_log: Vec::new(),
        }
    }

    /// Set custom G-metric threshold.
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.g_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Register a new sequence and return its table index.
    pub fn create_sequence(&mut self, sequence_id: u64) -> usize {
        let idx = self.tables.len();
        self.tables.push(BlockTable::new(sequence_id));
        idx
    }

    /// Store a KV pair for a sequence at a given token position.
    pub fn store_kv(
        &mut self,
        table_idx: usize,
        token_pos: usize,
        key: Vector,
        value: Vector,
    ) -> Result<(), anyhow::Error> {
        let logical_idx = token_pos / BLOCK_SIZE;
        let table = self.tables.get_mut(table_idx)
            .ok_or_else(|| anyhow::anyhow!("invalid table index"))?;

        // Allocate a new block if needed
        if table.get_physical(logical_idx).is_none() {
            let block_id = self.pool.allocate()
                .ok_or_else(|| anyhow::anyhow!("block pool exhausted"))?;
            if let Some(block) = self.pool.get_mut(block_id) {
                block.ref_count = 1;
            }
            table.map_block(logical_idx, block_id);
        }

        let physical_id = table.get_physical(logical_idx)
            .ok_or_else(|| anyhow::anyhow!("block mapping failed"))?;

        let block = self.pool.get_mut(physical_id)
            .ok_or_else(|| anyhow::anyhow!("block not found in pool"))?;

        block.write_kv(key, value)
            .ok_or_else(|| anyhow::anyhow!("block is full"))?;

        Ok(())
    }

    /// Perform gap-aware attention (Control Plane).
    ///
    /// Scans all blocks in the sequence's block table, computes attention
    /// scores, and checks the G-metric for knowledge gaps.
    pub fn forward(
        &mut self,
        table_idx: usize,
        query: &Vector,
    ) -> Result<AttentionOutput, anyhow::Error> {
        let table = self.tables.get(table_idx)
            .ok_or_else(|| anyhow::anyhow!("invalid table index"))?;

        let physical_blocks = table.physical_blocks();
        if physical_blocks.is_empty() {
            return Ok(AttentionOutput::GapDetected {
                g_metric: 1.0,
                missing_intent: query.clone(),
                closest_block: None,
            });
        }

        let mut max_score = f32::NEG_INFINITY;
        let mut total_exp_sum = 0.0f32;
        let mut weighted_sum = vec![0.0f32; HEAD_DIM];
        let mut closest_block = None;

        let scale = 1.0 / (HEAD_DIM as f32).sqrt();

        // Scan all blocks (the attention kernel)
        for &block_id in &physical_blocks {
            let block = match self.pool.get(block_id) {
                Some(b) => b,
                None => continue,
            };

            for slot in 0..block.filled_slots {
                let key = &block.k_cache[slot];
                let value = &block.v_cache[slot];

                // Scaled dot product: score = (Q · K) / sqrt(d)
                let score: f32 = query.iter()
                    .zip(key.iter())
                    .map(|(q, k)| q * k)
                    .sum::<f32>() * scale;

                if score > max_score {
                    max_score = score;
                    closest_block = Some(block_id);
                }

                let exp_score = score.exp();
                total_exp_sum += exp_score;

                // Accumulate weighted values
                for (j, &v) in value.iter().enumerate() {
                    weighted_sum[j] += exp_score * v;
                }
            }
        }

        // G-metric: 1.0 - normalized_similarity
        let normalized_similarity = max_score.tanh().max(0.0);
        let g_metric = 1.0 - normalized_similarity;

        // Update orthogonality scores on blocks
        for &block_id in &physical_blocks {
            if let Some(block) = self.pool.get_mut(block_id) {
                block.orthogonality_score = g_metric;
            }
        }

        // G-metric check: early exit if knowledge gap detected
        if g_metric > self.g_threshold {
            warn!(
                "QKV-G gap detected: G={:.4} (threshold={:.4})",
                g_metric, self.g_threshold
            );
            return Ok(AttentionOutput::GapDetected {
                g_metric,
                missing_intent: query.clone(),
                closest_block,
            });
        }

        // Normal output: softmax-normalized weighted sum
        if total_exp_sum > 0.0 {
            for v in &mut weighted_sum {
                *v /= total_exp_sum;
            }
        }

        info!("QKV-G forward: G={:.4}, max_sim={:.4}", g_metric, max_score);

        Ok(AttentionOutput::Success {
            output: weighted_sum,
            max_similarity: max_score,
            g_metric,
        })
    }

    /// Data Plane lookup — O(1) direct KV access by token position.
    /// Used for raw shard exchange (no attention computation).
    pub fn lookup_kv(
        &self,
        table_idx: usize,
        token_pos: usize,
    ) -> Option<(&Vector, &Vector)> {
        let table = self.tables.get(table_idx)?;
        let (physical_id, slot) = table.resolve_token(token_pos, BLOCK_SIZE)?;
        let block = self.pool.get(physical_id)?;
        block.read_kv(slot)
    }

    /// Control Plane: monitor a pattern vector for anomalies.
    /// Returns an alert if the pattern is orthogonal to known behavior.
    pub fn detect_anomaly(
        &mut self,
        source: &str,
        pattern: &Vector,
        reference_table_idx: usize,
    ) -> Option<AnomalyAlert> {
        let result = self.forward(reference_table_idx, pattern).ok()?;

        match result {
            AttentionOutput::GapDetected { g_metric, .. } => {
                let severity = if g_metric > 0.9 { 1.0 }
                    else if g_metric > 0.7 { 0.5 }
                    else { 0.2 };

                let alert = AnomalyAlert {
                    source: source.to_string(),
                    g_metric,
                    description: format!(
                        "Behavior orthogonal to known patterns (G={:.3})",
                        g_metric
                    ),
                    severity,
                };

                self.anomaly_log.push(alert.clone());
                Some(alert)
            }
            AttentionOutput::Success { g_metric, .. } if g_metric > 0.3 => {
                let alert = AnomalyAlert {
                    source: source.to_string(),
                    g_metric,
                    description: format!(
                        "Marginal pattern deviation detected (G={:.3})",
                        g_metric
                    ),
                    severity: 0.1,
                };
                self.anomaly_log.push(alert.clone());
                Some(alert)
            }
            _ => None,
        }
    }

    /// Get anomaly log.
    pub fn anomaly_log(&self) -> &[AnomalyAlert] {
        &self.anomaly_log
    }

    /// Pool statistics.
    pub fn pool_stats(&self) -> (usize, usize, usize) {
        (self.pool.total_count(), self.pool.active_count(), self.pool.free_count())
    }

    /// Number of active sequences.
    pub fn sequence_count(&self) -> usize {
        self.tables.len()
    }

    /// Get current G threshold.
    pub fn g_threshold(&self) -> f32 {
        self.g_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vector(val: f32) -> Vector {
        vec![val; HEAD_DIM]
    }

    #[test]
    fn empty_sequence_returns_gap() {
        let mut engine = HelmAttentionEngine::new(16);
        let seq = engine.create_sequence(1);
        let query = make_vector(1.0);

        match engine.forward(seq, &query).unwrap() {
            AttentionOutput::GapDetected { g_metric, .. } => {
                assert_eq!(g_metric, 1.0);
            }
            _ => panic!("expected GapDetected for empty sequence"),
        }
    }

    #[test]
    fn matching_query_returns_success() {
        let mut engine = HelmAttentionEngine::new(16);
        let seq = engine.create_sequence(1);

        // Store knowledge
        let key = make_vector(0.5);
        let value = make_vector(1.0);
        engine.store_kv(seq, 0, key.clone(), value).unwrap();

        // Query that matches
        let query = make_vector(0.5);
        match engine.forward(seq, &query).unwrap() {
            AttentionOutput::Success { g_metric, .. } => {
                assert!(g_metric < G_THRESHOLD, "g_metric={g_metric} should be below threshold");
            }
            AttentionOutput::GapDetected { g_metric, .. } => {
                panic!("expected Success but got GapDetected(G={g_metric})");
            }
        }
    }

    #[test]
    fn orthogonal_query_returns_gap() {
        let mut engine = HelmAttentionEngine::new(16).with_threshold(0.3);
        let seq = engine.create_sequence(1);

        // Store positive knowledge
        let key = make_vector(1.0);
        let value = make_vector(1.0);
        engine.store_kv(seq, 0, key, value).unwrap();

        // Query that is orthogonal (negative direction)
        let query = make_vector(-2.0);
        match engine.forward(seq, &query).unwrap() {
            AttentionOutput::GapDetected { g_metric, .. } => {
                assert!(g_metric > 0.3, "g_metric={g_metric} should indicate gap");
            }
            AttentionOutput::Success { g_metric, .. } => {
                panic!("expected GapDetected but got Success(G={g_metric})");
            }
        }
    }

    #[test]
    fn data_plane_lookup() {
        let mut engine = HelmAttentionEngine::new(16);
        let seq = engine.create_sequence(1);

        let key = make_vector(42.0);
        let value = make_vector(99.0);
        engine.store_kv(seq, 0, key.clone(), value.clone()).unwrap();

        // O(1) direct lookup
        let (k, v) = engine.lookup_kv(seq, 0).unwrap();
        assert_eq!(*k, key);
        assert_eq!(*v, value);
    }

    #[test]
    fn anomaly_detection() {
        let mut engine = HelmAttentionEngine::new(16);
        let seq = engine.create_sequence(1);

        // Store normal behavior pattern
        engine.store_kv(seq, 0, make_vector(1.0), make_vector(1.0)).unwrap();

        // Test with abnormal pattern (opposite direction)
        let anomalous = make_vector(-3.0);
        let alert = engine.detect_anomaly("peer-12D3Koo", &anomalous, seq);
        assert!(alert.is_some());
        assert!(alert.unwrap().g_metric > 0.3);
        assert_eq!(engine.anomaly_log().len(), 1);
    }

    #[test]
    fn pool_stats() {
        let engine = HelmAttentionEngine::new(32);
        let (total, active, free) = engine.pool_stats();
        assert_eq!(total, 32);
        assert_eq!(active, 0);
        assert_eq!(free, 32);
    }
}
