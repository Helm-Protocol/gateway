//! Helm Identity — Decentralized Identity for the Helm Protocol.
//!
//! Self-sovereign agent identity built on the `did:helm:` DID method.
//! No dependency on Ethereum, ERC standards, or any external chain.
//!
//! # Architecture
//!
//! - **DID**: `did:helm:<base58-pubkey>` — W3C DID-Core compatible, Helm-native
//! - **Identity Bond**: Non-transferable identity tokens (concept borrowed from SBTs)
//! - **Agent Spanner**: Hybrid resolver — on-node trust root + off-chain DHT performance
//! - **Reputation**: Multi-category trust scores with decay and fraud proofs
//! - **Plugin**: EventLoop integration for automatic identity management

pub mod did;
pub mod bond;
pub mod reputation;
pub mod spanner;
pub mod plugin;

// Re-exports
pub use did::{Did, DidDocument, DidRegistry, HelmKeyPair, ServiceEndpoint, VerificationMethod};
pub use bond::{BondId, BondRegistry, IdentityBond};
pub use reputation::{
    CategoryScore, FraudProof, FraudType, ReputationLedger, ReputationScore,
};
pub use spanner::{AgentSpanner, SpannerEntry};
pub use plugin::{IdentityPlugin, IdentityPluginConfig};

/// Identity errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum IdentityError {
    #[error("duplicate DID: '{0}'")]
    DuplicateDid(String),

    #[error("DID not found: '{0}'")]
    DidNotFound(String),

    #[error("duplicate identity bond for '{0}'")]
    DuplicateBond(String),

    #[error("identity bond not found for '{0}'")]
    BondNotFound(String),

    #[error("key rotation failed: {0}")]
    KeyRotationFailed(String),

    #[error("reputation error: {0}")]
    ReputationError(String),

    #[error("fraud proof: {0}")]
    FraudProof(String),
}
