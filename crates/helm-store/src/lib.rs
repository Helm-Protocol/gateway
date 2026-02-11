//! # Helm Store
//!
//! Distributed KV store for the Helm Protocol with CRDT support,
//! Merkle DAG for content-addressed storage, and anti-entropy sync.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              Helm Store                      │
//! │                                             │
//! │  ┌─────────┐  ┌──────────┐  ┌───────────┐  │
//! │  │  CRDT   │  │  Merkle  │  │   Sync    │  │
//! │  │ GC/LWW/ │  │   DAG    │  │ Protocol  │  │
//! │  │  OrSet  │  │ (SHA256) │  │(anti-ent) │  │
//! │  └────┬────┘  └────┬─────┘  └─────┬─────┘  │
//! │       │            │              │         │
//! │  ┌────▼────────────▼──────────────▼─────┐   │
//! │  │         KvStore Trait                  │  │
//! │  │   ┌──────────┐  ┌──────────────┐      │  │
//! │  │   │  Memory   │  │    Sled      │      │  │
//! │  │   │ Backend   │  │  Backend     │      │  │
//! │  │   └──────────┘  └──────────────┘      │  │
//! │  └───────────────────────────────────────┘  │
//! │                                             │
//! │  ┌───────────────────────────────────────┐  │
//! │  │        StorePlugin (helm-core)        │  │
//! │  └───────────────────────────────────────┘  │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! - **KvStore**: Trait abstraction over storage backends
//! - **MemoryBackend**: BTreeMap-based ephemeral store (testing)
//! - **SledBackend**: sled embedded DB for persistent storage
//! - **MerkleDag**: Content-addressed DAG with SHA-256 hashing
//! - **CRDTs**: GCounter, LwwRegister, OrSet, MerkleCrdt
//! - **Sync**: Anti-entropy protocol for state convergence
//! - **StorePlugin**: Integrates with helm-core Plugin system

pub mod kv;
pub mod backend;
pub mod merkle;
pub mod crdt;
pub mod sync;
pub mod plugin;

pub use kv::KvStore;
pub use backend::memory::MemoryBackend;
pub use backend::sled_backend::SledBackend;
pub use merkle::dag::{MerkleDag, DagNode, Hash};
pub use crdt::gcounter::{GCounter, Crdt};
pub use crdt::lww::LwwRegister;
pub use crdt::orset::OrSet;
pub use crdt::merkle_crdt::MerkleCrdt;
pub use sync::protocol::{SyncMessage, SyncSession};
pub use plugin::{StorePlugin, StorePluginConfig};
