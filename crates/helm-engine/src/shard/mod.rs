//! Distributed shard exchange for high-speed data transfer.
//!
//! Data Plane: Simple KV lookup for raw shard exchange (O(1)).
//! Uses standard HashMap — no QKV-G overhead on the hot path.
//!
//! The GRG pipeline protects shards in transit.
//! The QKV-G engine monitors the control plane (anomaly detection).

pub mod exchange;
