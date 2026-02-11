//! QKV-G (Query-Key-Value-Gap) Attention Engine
//!
//! Gap-aware paged attention inspired by vLLM's PagedAttention with
//! an additional G-metric that measures orthogonality between queries
//! and stored knowledge. When G exceeds a threshold, the engine
//! halts computation (early exit) and signals a knowledge gap.
//!
//! ## Data Plane vs Control Plane
//!
//! - **Data Plane**: Uses standard KV lookup (O(1)) for raw shard exchange
//! - **Control Plane**: Uses QKV-G attention for anomaly detection,
//!   intent-based routing, and security monitoring

pub mod cache_block;
pub mod attention;
pub mod block_pool;
pub mod block_table;
