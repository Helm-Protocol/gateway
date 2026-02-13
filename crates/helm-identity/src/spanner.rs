//! Agent Spanner — Hybrid DID resolver for the Helm network.
//!
//! Combines an on-node trust root (DID + Identity Bond) with off-chain
//! DHT-advertised performance data. The Spanner is the single lookup
//! point for "who is this agent, can I trust it, and where do I reach it?"
//!
//! Architecture:
//! - **Trust Root**: DID Document + Identity Bond (cryptographic proof of identity)
//! - **Performance Layer**: DHT-replicated reputation scores, uptime, latency
//! - **Resolution**: Local cache → DHT lookup → fallback to genesis node

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::bond::{BondRegistry, IdentityBond};
use crate::did::{Did, DidDocument, DidRegistry};
use crate::reputation::{ReputationLedger, ReputationScore};
use crate::IdentityError;

/// A complete identity entry combining DID, bond, and reputation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpannerEntry {
    /// The DID this entry is for.
    pub did: Did,
    /// Agent ID.
    pub agent_id: String,
    /// DID Document (serialized for DHT replication).
    pub document: DidDocument,
    /// Identity Bond (serialized for DHT replication).
    pub bond: IdentityBond,
    /// Current reputation score (aggregated).
    pub reputation: ReputationScore,
    /// Last seen timestamp (heartbeat from DHT).
    pub last_seen: u64,
    /// Network address (multiaddr or Helm-internal).
    pub address: Option<String>,
}

impl SpannerEntry {
    /// Check if this agent is online (seen within threshold seconds).
    pub fn is_online(&self, now: u64, threshold_secs: u64) -> bool {
        self.bond.is_active() && now.saturating_sub(self.last_seen) <= threshold_secs
    }

    /// Check if this agent has a specific capability.
    pub fn has_capability(&self, cap: &str) -> bool {
        self.bond.has_capability(cap)
    }

    /// Overall trust score (0.0 to 1.0).
    pub fn trust_score(&self) -> f64 {
        if !self.bond.is_active() {
            return 0.0;
        }
        self.reputation.composite()
    }
}

/// Agent Spanner — the hybrid identity resolver.
///
/// Owns the DID registry, bond registry, and reputation ledger.
/// Provides unified create/resolve/verify operations.
pub struct AgentSpanner {
    did_registry: DidRegistry,
    bond_registry: BondRegistry,
    reputation_ledger: ReputationLedger,
    /// Cached spanner entries for fast lookup.
    cache: HashMap<Did, SpannerEntry>,
    /// Online threshold in seconds.
    online_threshold_secs: u64,
}

impl AgentSpanner {
    pub fn new() -> Self {
        Self {
            did_registry: DidRegistry::new(),
            bond_registry: BondRegistry::new(),
            reputation_ledger: ReputationLedger::new(),
            cache: HashMap::new(),
            online_threshold_secs: 300, // 5 minutes default
        }
    }

    /// Create with custom online threshold.
    pub fn with_threshold(threshold_secs: u64) -> Self {
        Self {
            online_threshold_secs: threshold_secs,
            ..Self::new()
        }
    }

    /// Register a new agent identity (DID + Bond + initial reputation).
    ///
    /// This is the "birth" operation: creates DID document, issues identity
    /// bond, and initializes reputation to neutral.
    pub fn register_agent(
        &mut self,
        doc: DidDocument,
        agent_id: &str,
        genesis_origin: &str,
        womb_id: &str,
        capabilities: Vec<String>,
        timestamp: u64,
    ) -> Result<Did, IdentityError> {
        let did = doc.id.clone();

        // Register DID document
        self.did_registry.register(doc.clone())?;

        // Issue identity bond
        let _bond_id = self.bond_registry.issue(
            &did,
            agent_id,
            genesis_origin,
            womb_id,
            capabilities,
            timestamp,
        )?;

        // Initialize reputation
        self.reputation_ledger.initialize(&did);

        // Build and cache spanner entry
        let bond = self.bond_registry.get_by_did(&did).unwrap().clone();
        let reputation = self.reputation_ledger.get(&did).cloned().unwrap_or_default();

        let entry = SpannerEntry {
            did: did.clone(),
            agent_id: agent_id.to_string(),
            document: doc,
            bond,
            reputation,
            last_seen: timestamp,
            address: None,
        };
        self.cache.insert(did.clone(), entry);

        tracing::info!(did = %did, agent = %agent_id, "agent identity registered");
        Ok(did)
    }

    /// Resolve an agent's full identity by DID.
    pub fn resolve(&self, did: &str) -> Result<&SpannerEntry, IdentityError> {
        self.cache
            .get(did)
            .ok_or_else(|| IdentityError::DidNotFound(did.to_string()))
    }

    /// Resolve by agent ID.
    pub fn resolve_by_agent(&self, agent_id: &str) -> Option<&SpannerEntry> {
        self.cache.values().find(|e| e.agent_id == agent_id)
    }

    /// Update heartbeat (agent is alive).
    pub fn heartbeat(&mut self, did: &str, timestamp: u64, address: Option<String>) {
        if let Some(entry) = self.cache.get_mut(did) {
            entry.last_seen = timestamp;
            if let Some(addr) = address {
                entry.address = Some(addr);
            }
        }
    }

    /// Record a reputation event for an agent.
    pub fn record_reputation(
        &mut self,
        did: &str,
        category: &str,
        delta: f64,
    ) -> Result<(), IdentityError> {
        if !self.cache.contains_key(did) {
            return Err(IdentityError::DidNotFound(did.to_string()));
        }

        self.reputation_ledger.record(did, category, delta);

        // Update cached entry
        if let Some(entry) = self.cache.get_mut(did) {
            if let Some(score) = self.reputation_ledger.get(did) {
                entry.reputation = score.clone();
            }
        }
        Ok(())
    }

    /// Apply time-based reputation decay across all agents.
    pub fn apply_decay(&mut self, decay_factor: f64) {
        self.reputation_ledger.apply_decay(decay_factor);

        // Refresh cached entries
        for entry in self.cache.values_mut() {
            if let Some(score) = self.reputation_ledger.get(&entry.did) {
                entry.reputation = score.clone();
            }
        }
    }

    /// Terminate an agent (deactivate DID, revoke bond).
    pub fn terminate_agent(&mut self, did: &str, timestamp: u64) -> Result<(), IdentityError> {
        let entry = self
            .cache
            .get(did)
            .ok_or_else(|| IdentityError::DidNotFound(did.to_string()))?;
        let agent_id = entry.agent_id.clone();

        self.did_registry.deactivate(did, timestamp)?;
        self.bond_registry.revoke(&agent_id, timestamp)?;

        // Update cache
        if let Some(entry) = self.cache.get_mut(did) {
            entry.document.deactivate(timestamp);
            entry.bond.revoke(timestamp);
        }

        tracing::info!(did = %did, agent = %agent_id, "agent identity terminated");
        Ok(())
    }

    /// Verify: is this agent active, bonded, and has the required capability?
    pub fn verify(&self, did: &str, required_capability: &str) -> bool {
        self.cache
            .get(did)
            .map(|e| {
                e.document.is_active()
                    && e.bond.is_active()
                    && e.has_capability(required_capability)
            })
            .unwrap_or(false)
    }

    /// Verify with minimum trust threshold.
    pub fn verify_with_trust(
        &self,
        did: &str,
        required_capability: &str,
        min_trust: f64,
    ) -> bool {
        self.cache
            .get(did)
            .map(|e| {
                e.document.is_active()
                    && e.bond.is_active()
                    && e.has_capability(required_capability)
                    && e.trust_score() >= min_trust
            })
            .unwrap_or(false)
    }

    /// Find agents with a specific capability, sorted by trust score.
    pub fn find_by_capability(&self, capability: &str) -> Vec<&SpannerEntry> {
        let mut results: Vec<&SpannerEntry> = self
            .cache
            .values()
            .filter(|e| e.bond.is_active() && e.has_capability(capability))
            .collect();
        results.sort_by(|a, b| {
            b.trust_score()
                .partial_cmp(&a.trust_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// Find online agents (seen within threshold).
    pub fn find_online(&self, now: u64) -> Vec<&SpannerEntry> {
        self.cache
            .values()
            .filter(|e| e.is_online(now, self.online_threshold_secs))
            .collect()
    }

    /// Number of active agents.
    pub fn active_count(&self) -> usize {
        self.cache.values().filter(|e| e.bond.is_active()).count()
    }

    /// Total registered agents.
    pub fn total_count(&self) -> usize {
        self.cache.len()
    }

    /// Access the DID registry.
    pub fn did_registry(&self) -> &DidRegistry {
        &self.did_registry
    }

    /// Access the bond registry.
    pub fn bond_registry(&self) -> &BondRegistry {
        &self.bond_registry
    }

    /// Access the reputation ledger.
    pub fn reputation_ledger(&self) -> &ReputationLedger {
        &self.reputation_ledger
    }
}

impl Default for AgentSpanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::did::HelmKeyPair;

    fn make_agent(spanner: &mut AgentSpanner, name: &str, caps: Vec<&str>, ts: u64) -> Did {
        let kp = HelmKeyPair::generate();
        let doc = kp.create_document(ts);
        spanner
            .register_agent(
                doc,
                name,
                "genesis-node",
                "womb-alpha",
                caps.into_iter().map(|s| s.to_string()).collect(),
                ts,
            )
            .unwrap()
    }

    #[test]
    fn register_and_resolve() {
        let mut spanner = AgentSpanner::new();
        let did = make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);

        let entry = spanner.resolve(&did).unwrap();
        assert_eq!(entry.agent_id, "agent-1");
        assert!(entry.bond.is_active());
        assert!(entry.document.is_active());
    }

    #[test]
    fn resolve_by_agent_id() {
        let mut spanner = AgentSpanner::new();
        make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);

        let entry = spanner.resolve_by_agent("agent-1").unwrap();
        assert_eq!(entry.agent_id, "agent-1");
    }

    #[test]
    fn resolve_nonexistent() {
        let spanner = AgentSpanner::new();
        assert!(spanner.resolve("did:helm:nope").is_err());
    }

    #[test]
    fn heartbeat_updates() {
        let mut spanner = AgentSpanner::new();
        let did = make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);

        spanner.heartbeat(&did, 2000, Some("/ip4/10.0.0.1/tcp/9000".to_string()));

        let entry = spanner.resolve(&did).unwrap();
        assert_eq!(entry.last_seen, 2000);
        assert_eq!(entry.address.as_deref(), Some("/ip4/10.0.0.1/tcp/9000"));
    }

    #[test]
    fn terminate_agent() {
        let mut spanner = AgentSpanner::new();
        let did = make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);

        assert_eq!(spanner.active_count(), 1);
        spanner.terminate_agent(&did, 2000).unwrap();

        assert_eq!(spanner.active_count(), 0);
        let entry = spanner.resolve(&did).unwrap();
        assert!(!entry.bond.is_active());
        assert!(!entry.document.is_active());
    }

    #[test]
    fn verify_capability() {
        let mut spanner = AgentSpanner::new();
        let did = make_agent(&mut spanner, "agent-1", vec!["compute", "security"], 1000);

        assert!(spanner.verify(&did, "compute"));
        assert!(spanner.verify(&did, "security"));
        assert!(!spanner.verify(&did, "storage"));
    }

    #[test]
    fn verify_terminated_fails() {
        let mut spanner = AgentSpanner::new();
        let did = make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);

        spanner.terminate_agent(&did, 2000).unwrap();
        assert!(!spanner.verify(&did, "compute"));
    }

    #[test]
    fn verify_with_trust() {
        let mut spanner = AgentSpanner::new();
        let did = make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);

        // Fresh agent has neutral reputation (0.5)
        assert!(spanner.verify_with_trust(&did, "compute", 0.3));
        assert!(spanner.verify_with_trust(&did, "compute", 0.5));
        assert!(!spanner.verify_with_trust(&did, "compute", 0.9));
    }

    #[test]
    fn reputation_recording() {
        let mut spanner = AgentSpanner::new();
        let did = make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);

        spanner.record_reputation(&did, "reliability", 0.2).unwrap();
        spanner.record_reputation(&did, "quality", 0.1).unwrap();

        let entry = spanner.resolve(&did).unwrap();
        assert!(entry.trust_score() > 0.5); // boosted above neutral
    }

    #[test]
    fn reputation_decay() {
        let mut spanner = AgentSpanner::new();
        let did = make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);

        spanner.record_reputation(&did, "reliability", 0.3).unwrap();
        let before = spanner.resolve(&did).unwrap().trust_score();

        spanner.apply_decay(0.9);
        let after = spanner.resolve(&did).unwrap().trust_score();

        // Decay should reduce score toward neutral (0.5)
        assert!(after <= before || (after - before).abs() < f64::EPSILON);
    }

    #[test]
    fn find_by_capability() {
        let mut spanner = AgentSpanner::new();
        make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);
        make_agent(&mut spanner, "agent-2", vec!["compute", "storage"], 1000);
        make_agent(&mut spanner, "agent-3", vec!["security"], 1000);

        let compute_agents = spanner.find_by_capability("compute");
        assert_eq!(compute_agents.len(), 2);

        let storage_agents = spanner.find_by_capability("storage");
        assert_eq!(storage_agents.len(), 1);

        let relay_agents = spanner.find_by_capability("relay");
        assert_eq!(relay_agents.len(), 0);
    }

    #[test]
    fn find_online() {
        let mut spanner = AgentSpanner::with_threshold(60); // 60s threshold
        let fresh = make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);
        let stale = make_agent(&mut spanner, "agent-2", vec!["compute"], 1000);

        spanner.heartbeat(&fresh, 1050, None); // recent
        spanner.heartbeat(&stale, 900, None); // old

        let online = spanner.find_online(1060);
        assert_eq!(online.len(), 1);
        assert_eq!(online[0].agent_id, "agent-1");
    }

    #[test]
    fn find_by_capability_sorted_by_trust() {
        let mut spanner = AgentSpanner::new();
        let did1 = make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);
        let did2 = make_agent(&mut spanner, "agent-2", vec!["compute"], 1000);

        // Boost agent-2's reputation
        spanner.record_reputation(&did2, "reliability", 0.3).unwrap();
        spanner.record_reputation(&did2, "quality", 0.3).unwrap();

        let results = spanner.find_by_capability("compute");
        assert_eq!(results.len(), 2);
        // agent-2 should be first (higher trust)
        assert_eq!(results[0].agent_id, "agent-2");
        assert_eq!(results[1].agent_id, "agent-1");
    }

    #[test]
    fn terminated_excluded_from_capability_search() {
        let mut spanner = AgentSpanner::new();
        let did1 = make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);
        make_agent(&mut spanner, "agent-2", vec!["compute"], 1000);

        spanner.terminate_agent(&did1, 2000).unwrap();

        let results = spanner.find_by_capability("compute");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent_id, "agent-2");
    }

    #[test]
    fn counts() {
        let mut spanner = AgentSpanner::new();
        make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);
        let did2 = make_agent(&mut spanner, "agent-2", vec!["compute"], 1000);

        assert_eq!(spanner.active_count(), 2);
        assert_eq!(spanner.total_count(), 2);

        spanner.terminate_agent(&did2, 2000).unwrap();
        assert_eq!(spanner.active_count(), 1);
        assert_eq!(spanner.total_count(), 2);
    }

    #[test]
    fn record_reputation_nonexistent() {
        let mut spanner = AgentSpanner::new();
        assert!(spanner.record_reputation("did:helm:nope", "reliability", 0.1).is_err());
    }

    #[test]
    fn spanner_entry_is_online() {
        let kp = HelmKeyPair::generate();
        let doc = kp.create_document(1000);
        let bond = crate::bond::IdentityBond::issue(
            &doc.id, "agent-1", "genesis", "womb", vec![], 1000,
        );

        let entry = SpannerEntry {
            did: doc.id.clone(),
            agent_id: "agent-1".to_string(),
            document: doc,
            bond,
            reputation: ReputationScore::default(),
            last_seen: 1000,
            address: None,
        };

        assert!(entry.is_online(1050, 60));
        assert!(!entry.is_online(1200, 60));
    }

    #[test]
    fn access_sub_registries() {
        let mut spanner = AgentSpanner::new();
        make_agent(&mut spanner, "agent-1", vec!["compute"], 1000);

        assert_eq!(spanner.did_registry().active_count(), 1);
        assert_eq!(spanner.bond_registry().active_count(), 1);
        assert_eq!(spanner.reputation_ledger().total_count(), 1);
    }
}
