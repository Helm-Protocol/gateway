//! Edge API — public-facing API for external agents.
//!
//! Individual node servers expose this API to allow agents using
//! other protocols (bnkr, etc.) to access Helm Engine capabilities:
//! - GRG encode/decode (data protection)
//! - QKV-G attention queries (semantic search / anomaly check)
//! - Network acceleration (relay through Helm mesh)
//!
//! All calls are metered through the billing ledger.
//! 15% of revenue goes to Helm treasury.

use serde::{Serialize, Deserialize};
use tracing::info;

use super::billing::BillingLedger;
use crate::grg::pipeline::{GrgPipeline, GrgMode};
use crate::qkvg::attention::{HelmAttentionEngine, AttentionOutput};
use crate::qkvg::cache_block::Vector;

/// Request to the Edge API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeRequest {
    /// Encode data through the GRG pipeline
    GrgEncode {
        data: Vec<u8>,
        mode: GrgMode,
    },
    /// Perform a QKV-G attention query
    AttentionQuery {
        query_vector: Vec<f32>,
        sequence_id: u64,
    },
    /// Relay data through the Helm mesh network
    NetworkRelay {
        destination: String,
        payload: Vec<u8>,
    },
    /// Check network health
    HealthCheck,
}

/// Response from the Edge API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeResponse {
    /// GRG encoding result
    GrgEncoded {
        shard_count: usize,
        compressed_ratio: f64,
        mode: GrgMode,
    },
    /// Attention query result
    AttentionResult {
        /// Whether context was found or gap detected
        found: bool,
        /// G-metric value
        g_metric: f32,
        /// Output vector (if found)
        output: Option<Vec<f32>>,
    },
    /// Relay acknowledgment
    RelayAck {
        accepted: bool,
        hops: u32,
    },
    /// Health status
    Health {
        pool_total: usize,
        pool_active: usize,
        pool_free: usize,
        sequences: usize,
    },
    /// Error
    Error {
        message: String,
    },
}

/// The Edge API handler.
pub struct EdgeApi {
    /// Billing ledger
    billing: BillingLedger,
    /// GRG pipeline
    grg: GrgPipeline,
    /// Attention engine
    engine: HelmAttentionEngine,
    /// Sequence ID → table index mapping
    sequence_map: std::collections::HashMap<u64, usize>,
}

impl EdgeApi {
    /// Create a new Edge API handler.
    pub fn new(pool_capacity: usize) -> Self {
        let mut billing = BillingLedger::new();
        billing.set_price("grg/encode", 10);
        billing.set_price("grg/decode", 5);
        billing.set_price("attention/query", 50);
        billing.set_price("network/relay", 20);
        billing.set_price("health", 0);

        Self {
            billing,
            grg: GrgPipeline::default(),
            engine: HelmAttentionEngine::new(pool_capacity),
            sequence_map: std::collections::HashMap::new(),
        }
    }

    /// Handle an incoming Edge API request.
    pub fn handle(
        &mut self,
        caller: &str,
        request: EdgeRequest,
        timestamp_ms: u64,
    ) -> EdgeResponse {
        match request {
            EdgeRequest::GrgEncode { data, mode } => {
                self.billing.record_call(caller, None, "grg/encode", 1, timestamp_ms);
                self.grg.set_mode(mode);

                match self.grg.encode(&data) {
                    Ok(encoded) => {
                        info!("Edge API: GRG encode for {} ({} bytes)", caller, data.len());
                        EdgeResponse::GrgEncoded {
                            shard_count: encoded.shards.len(),
                            compressed_ratio: encoded.compressed_len as f64 / encoded.original_len as f64,
                            mode: encoded.mode,
                        }
                    }
                    Err(e) => EdgeResponse::Error { message: e.to_string() },
                }
            }

            EdgeRequest::AttentionQuery { query_vector, sequence_id } => {
                self.billing.record_call(caller, None, "attention/query", 1, timestamp_ms);

                let table_idx = *self.sequence_map.entry(sequence_id).or_insert_with(|| {
                    self.engine.create_sequence(sequence_id)
                });

                match self.engine.forward(table_idx, &query_vector) {
                    Ok(AttentionOutput::Success { output, g_metric, .. }) => {
                        EdgeResponse::AttentionResult {
                            found: true,
                            g_metric,
                            output: Some(output),
                        }
                    }
                    Ok(AttentionOutput::GapDetected { g_metric, .. }) => {
                        EdgeResponse::AttentionResult {
                            found: false,
                            g_metric,
                            output: None,
                        }
                    }
                    Err(e) => EdgeResponse::Error { message: e.to_string() },
                }
            }

            EdgeRequest::NetworkRelay { destination, payload } => {
                self.billing.record_call(caller, None, "network/relay", 1, timestamp_ms);
                info!(
                    "Edge API: relay {} bytes to {} for {}",
                    payload.len(), destination, caller
                );
                // Relay is handled by the network layer (helm-net)
                EdgeResponse::RelayAck {
                    accepted: true,
                    hops: 0, // Filled by network layer
                }
            }

            EdgeRequest::HealthCheck => {
                let (total, active, free) = self.engine.pool_stats();
                EdgeResponse::Health {
                    pool_total: total,
                    pool_active: active,
                    pool_free: free,
                    sequences: self.engine.sequence_count(),
                }
            }
        }
    }

    /// Store knowledge for a sequence (used by node operators).
    pub fn store_knowledge(
        &mut self,
        sequence_id: u64,
        token_pos: usize,
        key: Vector,
        value: Vector,
    ) -> Result<(), anyhow::Error> {
        let table_idx = *self.sequence_map.entry(sequence_id).or_insert_with(|| {
            self.engine.create_sequence(sequence_id)
        });
        self.engine.store_kv(table_idx, token_pos, key, value)
    }

    /// Get billing ledger reference.
    pub fn billing(&self) -> &BillingLedger {
        &self.billing
    }

    /// Get mutable billing ledger.
    pub fn billing_mut(&mut self) -> &mut BillingLedger {
        &mut self.billing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_health_check() {
        let mut api = EdgeApi::new(32);
        match api.handle("test-agent", EdgeRequest::HealthCheck, 0) {
            EdgeResponse::Health { pool_total, pool_free, .. } => {
                assert_eq!(pool_total, 32);
                assert_eq!(pool_free, 32);
            }
            _ => panic!("expected Health response"),
        }
    }

    #[test]
    fn edge_grg_encode() {
        let mut api = EdgeApi::new(16);
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        match api.handle("agent-1", EdgeRequest::GrgEncode {
            data,
            mode: GrgMode::Turbo,
        }, 1000) {
            EdgeResponse::GrgEncoded { shard_count, mode, .. } => {
                assert_eq!(shard_count, 1); // Turbo mode = 1 shard
                assert_eq!(mode, GrgMode::Turbo);
            }
            EdgeResponse::Error { message } => panic!("encode failed: {message}"),
            _ => panic!("unexpected response"),
        }

        // Verify billing
        assert_eq!(api.billing().call_count(), 1);
        assert!(api.billing().treasury_balance() > 0);
    }

    #[test]
    fn edge_attention_query_gap() {
        let mut api = EdgeApi::new(16);

        // Query without stored knowledge → gap
        let query = vec![1.0; 128];
        match api.handle("agent-1", EdgeRequest::AttentionQuery {
            query_vector: query,
            sequence_id: 1,
        }, 1000) {
            EdgeResponse::AttentionResult { found, g_metric, .. } => {
                assert!(!found);
                assert_eq!(g_metric, 1.0);
            }
            _ => panic!("expected AttentionResult"),
        }

        assert_eq!(api.billing().call_count(), 1);
    }

    #[test]
    fn edge_billing_accumulates() {
        let mut api = EdgeApi::new(16);

        for i in 0..5 {
            api.handle(
                "agent-bnkr",
                EdgeRequest::HealthCheck,
                i * 1000,
            );
        }

        // Health checks are free
        assert_eq!(api.billing().total_api_revenue(), 0);

        // GRG calls cost 10
        api.handle("agent-bnkr", EdgeRequest::GrgEncode {
            data: vec![1; 20],
            mode: GrgMode::Turbo,
        }, 5000);

        // 85% treasury, 15% referrer; no referrer → 100% to treasury
        assert_eq!(api.billing().total_api_revenue(), 10);
        assert_eq!(api.billing().treasury_balance(), 10);
    }
}
