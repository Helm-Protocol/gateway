//! Agent capability system.
//!
//! Capabilities declare what an agent can do within the network.
//! The registry uses capabilities for discovery and permission checks.

use serde::{Deserialize, Serialize};

/// A capability that an agent declares or requests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// General computation (CPU-bound tasks).
    Compute,
    /// Data storage operations (KV store access).
    Storage,
    /// Network relay and message forwarding.
    Network,
    /// Governance participation (voting, proposals).
    Governance,
    /// Security monitoring and audit.
    Security,
    /// GRG codec pipeline access.
    Codec,
    /// Socratic reasoning (can answer gap queries).
    Socratic,
    /// Agent spawning (Helm Womb access).
    Spawning,
    /// Token operations (minting, staking, transfer).
    Token,
    /// External API exposure (Edge API).
    EdgeApi,
    /// Custom capability with a string identifier.
    Custom(String),
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compute => write!(f, "compute"),
            Self::Storage => write!(f, "storage"),
            Self::Network => write!(f, "network"),
            Self::Governance => write!(f, "governance"),
            Self::Security => write!(f, "security"),
            Self::Codec => write!(f, "codec"),
            Self::Socratic => write!(f, "socratic"),
            Self::Spawning => write!(f, "spawning"),
            Self::Token => write!(f, "token"),
            Self::EdgeApi => write!(f, "edge-api"),
            Self::Custom(s) => write!(f, "custom:{s}"),
        }
    }
}

/// A set of capabilities with helper methods.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitySet {
    caps: Vec<Capability>,
}

impl CapabilitySet {
    pub fn new() -> Self {
        Self { caps: Vec::new() }
    }

    pub fn with(mut self, cap: Capability) -> Self {
        if !self.caps.contains(&cap) {
            self.caps.push(cap);
        }
        self
    }

    pub fn add(&mut self, cap: Capability) {
        if !self.caps.contains(&cap) {
            self.caps.push(cap);
        }
    }

    pub fn remove(&mut self, cap: &Capability) {
        self.caps.retain(|c| c != cap);
    }

    pub fn has(&self, cap: &Capability) -> bool {
        self.caps.contains(cap)
    }

    /// Check if this set contains ALL required capabilities.
    pub fn satisfies(&self, required: &[Capability]) -> bool {
        required.iter().all(|r| self.has(r))
    }

    /// Check if this set contains ANY of the listed capabilities.
    pub fn has_any(&self, caps: &[Capability]) -> bool {
        caps.iter().any(|c| self.has(c))
    }

    pub fn as_slice(&self) -> &[Capability] {
        &self.caps
    }

    pub fn len(&self) -> usize {
        self.caps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.caps.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.caps.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_display() {
        assert_eq!(Capability::Compute.to_string(), "compute");
        assert_eq!(Capability::Storage.to_string(), "storage");
        assert_eq!(Capability::Governance.to_string(), "governance");
        assert_eq!(Capability::Custom("my-cap".into()).to_string(), "custom:my-cap");
    }

    #[test]
    fn capability_equality() {
        assert_eq!(Capability::Compute, Capability::Compute);
        assert_ne!(Capability::Compute, Capability::Storage);
        assert_eq!(
            Capability::Custom("x".into()),
            Capability::Custom("x".into())
        );
        assert_ne!(
            Capability::Custom("x".into()),
            Capability::Custom("y".into())
        );
    }

    #[test]
    fn capability_set_builder() {
        let set = CapabilitySet::new()
            .with(Capability::Compute)
            .with(Capability::Storage)
            .with(Capability::Compute); // duplicate ignored
        assert_eq!(set.len(), 2);
        assert!(set.has(&Capability::Compute));
        assert!(set.has(&Capability::Storage));
    }

    #[test]
    fn capability_set_add_remove() {
        let mut set = CapabilitySet::new();
        set.add(Capability::Network);
        set.add(Capability::Security);
        assert_eq!(set.len(), 2);

        set.remove(&Capability::Network);
        assert_eq!(set.len(), 1);
        assert!(!set.has(&Capability::Network));
        assert!(set.has(&Capability::Security));
    }

    #[test]
    fn capability_set_satisfies() {
        let set = CapabilitySet::new()
            .with(Capability::Compute)
            .with(Capability::Storage)
            .with(Capability::Network);

        assert!(set.satisfies(&[Capability::Compute, Capability::Storage]));
        assert!(!set.satisfies(&[Capability::Compute, Capability::Governance]));
        assert!(set.satisfies(&[])); // empty requirement always satisfied
    }

    #[test]
    fn capability_set_has_any() {
        let set = CapabilitySet::new()
            .with(Capability::Codec);

        assert!(set.has_any(&[Capability::Codec, Capability::Token]));
        assert!(!set.has_any(&[Capability::Token, Capability::Governance]));
        assert!(!set.has_any(&[])); // empty list → false
    }

    #[test]
    fn capability_set_empty() {
        let set = CapabilitySet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn capability_serialize_roundtrip() {
        let set = CapabilitySet::new()
            .with(Capability::Socratic)
            .with(Capability::Custom("bridge".into()));
        let json = serde_json::to_string(&set).unwrap();
        let decoded: CapabilitySet = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.len(), 2);
        assert!(decoded.has(&Capability::Socratic));
        assert!(decoded.has(&Capability::Custom("bridge".into())));
    }
}
