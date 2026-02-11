//! # Helm Engine
//!
//! QKV-G (Query-Key-Value-Gap) attention engine with GRG
//! (Golomb-RedStuff-Golay) distributed codec pipeline.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              Helm Engine                     │
//! │                                             │
//! │  ┌─────────┐  ┌──────────┐  ┌───────────┐  │
//! │  │  GRG    │  │  QKV-G   │  │   APIs    │  │
//! │  │Pipeline │  │ Attention │  │Edge/Core  │  │
//! │  └────┬────┘  └────┬─────┘  └─────┬─────┘  │
//! │       │            │              │         │
//! │  ┌────▼────────────▼──────────────▼─────┐   │
//! │  │       Distributed Shard Exchange      │  │
//! │  └───────────────────────────────────────┘  │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! - **Data Plane**: GRG pipeline + KV cache for raw shard exchange (O(1))
//! - **Control Plane**: QKV-G attention for anomaly detection, routing, security

pub mod grg;
pub mod qkvg;
pub mod api;
pub mod shard;

pub use grg::pipeline::{GrgPipeline, GrgMode};
pub use qkvg::attention::{HelmAttentionEngine, AttentionOutput};
pub use qkvg::cache_block::GapAwareCacheBlock;
pub use api::edge::EdgeApi;
pub use api::core_api::CoreApi;
pub use api::billing::BillingLedger;
pub use shard::exchange::ShardExchange;
