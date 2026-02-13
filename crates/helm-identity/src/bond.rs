//! Identity Bond — Non-transferable identity tokens for Helm agents.
//!
//! Inspired by the Soulbound Token concept (non-transferable, soul-bound
//! to a single entity), but implemented as a Helm-native primitive with
//! no dependency on Ethereum, ERC standards, or any external chain.
//!
//! An Identity Bond is issued at agent birth and encodes the immutable
//! properties of an agent: its DID, capabilities, genesis origin, and
//! the Womb that birthed it. Bonds cannot be transferred between agents.
//! They can only be revoked (burned) when an agent is terminated.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::did::Did;
use crate::IdentityError;

/// A Bond ID — deterministic hash of genesis data.
pub type BondId = [u8; 32];

/// Identity Bond — non-transferable, bound to one agent for its lifetime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityBond {
    /// Unique bond ID (hash of genesis data).
    pub id: BondId,
    /// The DID this bond is bound to (`did:helm:<id>`).
    pub did: Did,
    /// The agent ID this bond is bound to.
    pub agent_id: String,
    /// Genesis node that created this agent.
    pub genesis_origin: String,
    /// Womb ID that birthed the agent.
    pub womb_id: String,
    /// Immutable capability set at birth.
    pub capabilities: Vec<String>,
    /// Creation timestamp (unix seconds).
    pub created_at: u64,
    /// Whether this bond has been revoked (agent terminated).
    pub revoked: bool,
    /// Revocation timestamp (if revoked).
    pub revoked_at: Option<u64>,
    /// Arbitrary metadata.
    pub metadata: HashMap<String, String>,
}

impl IdentityBond {
    /// Issue a new Identity Bond for an agent.
    pub fn issue(
        did: &str,
        agent_id: &str,
        genesis_origin: &str,
        womb_id: &str,
        capabilities: Vec<String>,
        timestamp: u64,
    ) -> Self {
        let mut id_input = Vec::new();
        id_input.extend_from_slice(did.as_bytes());
        id_input.extend_from_slice(agent_id.as_bytes());
        id_input.extend_from_slice(genesis_origin.as_bytes());
        id_input.extend_from_slice(womb_id.as_bytes());
        id_input.extend_from_slice(&timestamp.to_be_bytes());

        let id = deterministic_hash(&id_input);

        Self {
            id,
            did: did.to_string(),
            agent_id: agent_id.to_string(),
            genesis_origin: genesis_origin.to_string(),
            womb_id: womb_id.to_string(),
            capabilities,
            created_at: timestamp,
            revoked: false,
            revoked_at: None,
            metadata: HashMap::new(),
        }
    }

    /// Revoke (burn) this bond — agent terminated.
    pub fn revoke(&mut self, timestamp: u64) {
        self.revoked = true;
        self.revoked_at = Some(timestamp);
    }

    /// Check if this bond is active (not revoked).
    pub fn is_active(&self) -> bool {
        !self.revoked
    }

    /// Add metadata to the bond.
    pub fn set_metadata(&mut self, key: &str, value: &str) {
        self.metadata.insert(key.to_string(), value.to_string());
    }

    /// Check if the agent has a specific capability.
    pub fn has_capability(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }
}

/// Bond Registry — tracks all issued Identity Bonds.
pub struct BondRegistry {
    bonds: HashMap<BondId, IdentityBond>,
    /// DID → BondId mapping (one bond per DID).
    did_index: HashMap<Did, BondId>,
    /// AgentId → BondId mapping.
    agent_index: HashMap<String, BondId>,
}

impl BondRegistry {
    pub fn new() -> Self {
        Self {
            bonds: HashMap::new(),
            did_index: HashMap::new(),
            agent_index: HashMap::new(),
        }
    }

    /// Issue and register a new Identity Bond. One bond per DID, one per agent.
    pub fn issue(
        &mut self,
        did: &str,
        agent_id: &str,
        genesis_origin: &str,
        womb_id: &str,
        capabilities: Vec<String>,
        timestamp: u64,
    ) -> Result<BondId, IdentityError> {
        if self.did_index.contains_key(did) {
            return Err(IdentityError::DuplicateBond(did.to_string()));
        }
        if self.agent_index.contains_key(agent_id) {
            return Err(IdentityError::DuplicateBond(agent_id.to_string()));
        }

        let bond = IdentityBond::issue(did, agent_id, genesis_origin, womb_id, capabilities, timestamp);
        let id = bond.id;

        self.did_index.insert(did.to_string(), id);
        self.agent_index.insert(agent_id.to_string(), id);
        self.bonds.insert(id, bond);

        Ok(id)
    }

    /// Revoke an agent's bond (agent terminated).
    pub fn revoke(&mut self, agent_id: &str, timestamp: u64) -> Result<(), IdentityError> {
        let bond_id = self
            .agent_index
            .get(agent_id)
            .ok_or_else(|| IdentityError::BondNotFound(agent_id.to_string()))?;

        let bond = self
            .bonds
            .get_mut(bond_id)
            .ok_or_else(|| IdentityError::BondNotFound(agent_id.to_string()))?;

        bond.revoke(timestamp);
        Ok(())
    }

    /// Look up a bond by agent ID.
    pub fn get_by_agent(&self, agent_id: &str) -> Option<&IdentityBond> {
        self.agent_index
            .get(agent_id)
            .and_then(|id| self.bonds.get(id))
    }

    /// Look up a bond by DID.
    pub fn get_by_did(&self, did: &str) -> Option<&IdentityBond> {
        self.did_index
            .get(did)
            .and_then(|id| self.bonds.get(id))
    }

    /// Look up a bond by its ID.
    pub fn get(&self, id: &BondId) -> Option<&IdentityBond> {
        self.bonds.get(id)
    }

    /// Number of active (non-revoked) bonds.
    pub fn active_count(&self) -> usize {
        self.bonds.values().filter(|b| b.is_active()).count()
    }

    /// Total bonds issued (including revoked).
    pub fn total_issued(&self) -> usize {
        self.bonds.len()
    }

    /// Verify an agent's identity: active bond exists with matching capability.
    pub fn verify_capability(&self, agent_id: &str, required_capability: &str) -> bool {
        self.get_by_agent(agent_id)
            .map(|bond| bond.is_active() && bond.has_capability(required_capability))
            .unwrap_or(false)
    }

    /// Verify a DID has an active bond.
    pub fn verify_did(&self, did: &str) -> bool {
        self.get_by_did(did)
            .map(|bond| bond.is_active())
            .unwrap_or(false)
    }
}

impl Default for BondRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Deterministic 32-byte hash (djb2-style, no external crate needed).
fn deterministic_hash(data: &[u8]) -> [u8; 32] {
    let mut hash = [0u8; 32];
    let mut h: u64 = 5381;
    for (i, &byte) in data.iter().enumerate() {
        h = h.wrapping_mul(33).wrapping_add(byte as u64);
        hash[i % 32] ^= (h & 0xFF) as u8;
    }
    for i in 0..32 {
        h = h.wrapping_mul(33).wrapping_add(hash[i] as u64);
        hash[i] = (h & 0xFF) as u8;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_bond() {
        let bond = IdentityBond::issue(
            "did:helm:abc",
            "agent-1",
            "genesis-node",
            "womb-alpha",
            vec!["compute".to_string(), "storage".to_string()],
            1000,
        );

        assert_eq!(bond.did, "did:helm:abc");
        assert_eq!(bond.agent_id, "agent-1");
        assert_eq!(bond.capabilities.len(), 2);
        assert!(bond.is_active());
    }

    #[test]
    fn revoke_bond() {
        let mut bond = IdentityBond::issue(
            "did:helm:abc", "agent-1", "genesis", "womb", vec![], 1000,
        );
        assert!(bond.is_active());

        bond.revoke(2000);
        assert!(!bond.is_active());
        assert_eq!(bond.revoked_at, Some(2000));
    }

    #[test]
    fn bond_metadata() {
        let mut bond = IdentityBond::issue(
            "did:helm:abc", "agent-1", "genesis", "womb", vec![], 1000,
        );
        bond.set_metadata("model", "qkv-g-v2");
        assert_eq!(bond.metadata.get("model").unwrap(), "qkv-g-v2");
    }

    #[test]
    fn bond_has_capability() {
        let bond = IdentityBond::issue(
            "did:helm:abc", "agent-1", "genesis", "womb",
            vec!["compute".to_string(), "security".to_string()],
            1000,
        );
        assert!(bond.has_capability("compute"));
        assert!(bond.has_capability("security"));
        assert!(!bond.has_capability("storage"));
    }

    #[test]
    fn registry_issue_and_lookup() {
        let mut reg = BondRegistry::new();
        let id = reg.issue(
            "did:helm:abc", "agent-1", "genesis", "womb-1",
            vec!["compute".to_string()], 1000,
        ).unwrap();

        let bond = reg.get(&id).unwrap();
        assert_eq!(bond.agent_id, "agent-1");

        let bond2 = reg.get_by_agent("agent-1").unwrap();
        assert_eq!(bond2.id, id);

        let bond3 = reg.get_by_did("did:helm:abc").unwrap();
        assert_eq!(bond3.id, id);
    }

    #[test]
    fn registry_duplicate_prevention_did() {
        let mut reg = BondRegistry::new();
        reg.issue("did:helm:abc", "agent-1", "genesis", "womb", vec![], 1000).unwrap();
        let result = reg.issue("did:helm:abc", "agent-2", "genesis", "womb", vec![], 2000);
        assert!(result.is_err());
    }

    #[test]
    fn registry_duplicate_prevention_agent() {
        let mut reg = BondRegistry::new();
        reg.issue("did:helm:abc", "agent-1", "genesis", "womb", vec![], 1000).unwrap();
        let result = reg.issue("did:helm:def", "agent-1", "genesis", "womb", vec![], 2000);
        assert!(result.is_err());
    }

    #[test]
    fn registry_revoke() {
        let mut reg = BondRegistry::new();
        reg.issue("did:helm:abc", "agent-1", "genesis", "womb", vec![], 1000).unwrap();

        assert_eq!(reg.active_count(), 1);
        reg.revoke("agent-1", 2000).unwrap();
        assert_eq!(reg.active_count(), 0);
        assert_eq!(reg.total_issued(), 1);
    }

    #[test]
    fn registry_verify_capability() {
        let mut reg = BondRegistry::new();
        reg.issue(
            "did:helm:abc", "agent-1", "genesis", "womb",
            vec!["compute".to_string(), "security".to_string()],
            1000,
        ).unwrap();

        assert!(reg.verify_capability("agent-1", "compute"));
        assert!(reg.verify_capability("agent-1", "security"));
        assert!(!reg.verify_capability("agent-1", "storage"));
        assert!(!reg.verify_capability("agent-2", "compute"));
    }

    #[test]
    fn registry_verify_did() {
        let mut reg = BondRegistry::new();
        reg.issue("did:helm:abc", "agent-1", "genesis", "womb", vec![], 1000).unwrap();

        assert!(reg.verify_did("did:helm:abc"));
        assert!(!reg.verify_did("did:helm:def"));

        reg.revoke("agent-1", 2000).unwrap();
        assert!(!reg.verify_did("did:helm:abc"));
    }

    #[test]
    fn registry_verify_revoked_capability() {
        let mut reg = BondRegistry::new();
        reg.issue(
            "did:helm:abc", "agent-1", "genesis", "womb",
            vec!["compute".to_string()],
            1000,
        ).unwrap();
        reg.revoke("agent-1", 2000).unwrap();

        assert!(!reg.verify_capability("agent-1", "compute"));
    }

    #[test]
    fn bond_id_deterministic() {
        let b1 = IdentityBond::issue("did:helm:abc", "a", "g", "w", vec![], 100);
        let b2 = IdentityBond::issue("did:helm:abc", "a", "g", "w", vec![], 100);
        assert_eq!(b1.id, b2.id);
    }

    #[test]
    fn bond_id_unique_for_different_agents() {
        let b1 = IdentityBond::issue("did:helm:abc", "a", "g", "w", vec![], 100);
        let b2 = IdentityBond::issue("did:helm:def", "b", "g", "w", vec![], 100);
        assert_ne!(b1.id, b2.id);
    }

    #[test]
    fn revoke_nonexistent_agent() {
        let mut reg = BondRegistry::new();
        assert!(reg.revoke("agent-none", 1000).is_err());
    }
}
