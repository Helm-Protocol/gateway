//! Governance Proposals — typed proposals with lifecycle management.
//!
//! Proposal types:
//! - ParameterChange: Modify protocol parameters (fees, thresholds, weights)
//! - Treasury: Fund a project from the treasury CapitalPool
//! - Upgrade: Propose a protocol upgrade (version bump, module swap)
//! - Emergency: Fast-track emergency actions (circuit breaker, slash)
//! - Custom: Free-form governance proposals

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique proposal identifier.
pub type ProposalId = u64;

/// Proposal lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalState {
    /// Submitted, waiting for voting period.
    Pending,
    /// Currently in voting period.
    Active,
    /// Voting ended, quorum met, majority approved.
    Passed,
    /// Voting ended, rejected (no quorum or majority against).
    Rejected,
    /// Passed and executed on-chain.
    Executed,
    /// Cancelled by proposer before voting ends.
    Cancelled,
    /// Emergency: fast-tracked (shorter voting, higher quorum).
    Emergency,
}

impl ProposalState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ProposalState::Executed | ProposalState::Rejected | ProposalState::Cancelled
        )
    }

    pub fn is_votable(&self) -> bool {
        matches!(self, ProposalState::Active | ProposalState::Emergency)
    }
}

/// Types of governance proposals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProposalType {
    /// Change a protocol parameter.
    ParameterChange {
        parameter: String,
        old_value: String,
        new_value: String,
    },
    /// Fund a project from treasury.
    TreasurySpend {
        recipient: String,
        amount: u128,
        project_name: String,
    },
    /// Protocol upgrade proposal.
    Upgrade {
        version: String,
        description: String,
    },
    /// Emergency action (fast-track).
    Emergency {
        action: String,
        justification: String,
    },
    /// Custom free-form proposal.
    Custom {
        title: String,
        body: String,
    },
}

/// A governance proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    /// Unique ID.
    pub id: ProposalId,
    /// Proposer (DID or address).
    pub proposer: String,
    /// Proposal type and payload.
    pub proposal_type: ProposalType,
    /// Current state.
    pub state: ProposalState,
    /// Epoch when voting starts.
    pub start_epoch: u64,
    /// Epoch when voting ends.
    pub end_epoch: u64,
    /// Creation epoch.
    pub created_at: u64,
    /// Votes for.
    pub votes_for: u128,
    /// Votes against.
    pub votes_against: u128,
    /// Individual votes: voter → (support, weight).
    pub votes: HashMap<String, (bool, u128)>,
    /// Execution result (if executed).
    pub execution_result: Option<String>,
}

impl Proposal {
    /// Total votes cast.
    pub fn total_votes(&self) -> u128 {
        self.votes_for.saturating_add(self.votes_against)
    }

    /// Approval percentage (0.0 to 1.0).
    pub fn approval_rate(&self) -> f64 {
        let total = self.total_votes();
        if total == 0 {
            return 0.0;
        }
        self.votes_for as f64 / total as f64
    }

    /// Number of unique voters.
    pub fn voter_count(&self) -> usize {
        self.votes.len()
    }

    /// Check if a specific voter has already voted.
    pub fn has_voted(&self, voter: &str) -> bool {
        self.votes.contains_key(voter)
    }
}

/// Proposal registry — manages all governance proposals.
pub struct ProposalRegistry {
    proposals: HashMap<ProposalId, Proposal>,
    next_id: ProposalId,
    /// Proposer index.
    proposer_index: HashMap<String, Vec<ProposalId>>,
}

impl ProposalRegistry {
    pub fn new() -> Self {
        Self {
            proposals: HashMap::new(),
            next_id: 1,
            proposer_index: HashMap::new(),
        }
    }

    /// Submit a new proposal.
    pub fn submit(
        &mut self,
        proposer: &str,
        proposal_type: ProposalType,
        start_epoch: u64,
        end_epoch: u64,
        current_epoch: u64,
    ) -> ProposalId {
        let id = self.next_id;
        self.next_id += 1;

        let state = if matches!(proposal_type, ProposalType::Emergency { .. }) {
            ProposalState::Emergency
        } else {
            ProposalState::Pending
        };

        let proposal = Proposal {
            id,
            proposer: proposer.to_string(),
            proposal_type,
            state,
            start_epoch,
            end_epoch,
            created_at: current_epoch,
            votes_for: 0,
            votes_against: 0,
            votes: HashMap::new(),
            execution_result: None,
        };

        self.proposals.insert(id, proposal);
        self.proposer_index
            .entry(proposer.to_string())
            .or_default()
            .push(id);

        id
    }

    /// Get a proposal by ID.
    pub fn get(&self, id: ProposalId) -> Option<&Proposal> {
        self.proposals.get(&id)
    }

    /// Get a proposal mutably.
    pub fn get_mut(&mut self, id: ProposalId) -> Option<&mut Proposal> {
        self.proposals.get_mut(&id)
    }

    /// All proposals by a proposer.
    pub fn by_proposer(&self, proposer: &str) -> Vec<&Proposal> {
        self.proposer_index
            .get(proposer)
            .map(|ids| ids.iter().filter_map(|id| self.proposals.get(id)).collect())
            .unwrap_or_default()
    }

    /// All active (votable) proposals.
    pub fn active_proposals(&self) -> Vec<&Proposal> {
        self.proposals
            .values()
            .filter(|p| p.state.is_votable())
            .collect()
    }

    /// Total proposals.
    pub fn total(&self) -> usize {
        self.proposals.len()
    }

    /// All proposals (for iteration).
    pub fn all(&self) -> Vec<&Proposal> {
        self.proposals.values().collect()
    }
}

impl Default for ProposalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_proposal() {
        let mut reg = ProposalRegistry::new();
        let id = reg.submit(
            "did:helm:abc",
            ProposalType::Custom {
                title: "Test".to_string(),
                body: "Body".to_string(),
            },
            10,
            20,
            5,
        );
        assert_eq!(id, 1);
        let p = reg.get(id).unwrap();
        assert_eq!(p.state, ProposalState::Pending);
    }

    #[test]
    fn emergency_proposal_state() {
        let mut reg = ProposalRegistry::new();
        let id = reg.submit(
            "did:helm:abc",
            ProposalType::Emergency {
                action: "halt".to_string(),
                justification: "attack".to_string(),
            },
            10,
            15,
            10,
        );
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Emergency);
    }

    #[test]
    fn proposal_approval_rate() {
        let p = Proposal {
            id: 1,
            proposer: "test".to_string(),
            proposal_type: ProposalType::Custom {
                title: "t".to_string(),
                body: "b".to_string(),
            },
            state: ProposalState::Active,
            start_epoch: 0,
            end_epoch: 10,
            created_at: 0,
            votes_for: 70,
            votes_against: 30,
            votes: HashMap::new(),
            execution_result: None,
        };
        assert!((p.approval_rate() - 0.7).abs() < 0.01);
        assert_eq!(p.total_votes(), 100);
    }

    #[test]
    fn proposal_zero_votes() {
        let p = Proposal {
            id: 1,
            proposer: "test".to_string(),
            proposal_type: ProposalType::Custom {
                title: "t".to_string(),
                body: "b".to_string(),
            },
            state: ProposalState::Active,
            start_epoch: 0,
            end_epoch: 10,
            created_at: 0,
            votes_for: 0,
            votes_against: 0,
            votes: HashMap::new(),
            execution_result: None,
        };
        assert_eq!(p.approval_rate(), 0.0);
    }

    #[test]
    fn state_predicates() {
        assert!(ProposalState::Executed.is_terminal());
        assert!(ProposalState::Rejected.is_terminal());
        assert!(ProposalState::Cancelled.is_terminal());
        assert!(!ProposalState::Active.is_terminal());
        assert!(ProposalState::Active.is_votable());
        assert!(ProposalState::Emergency.is_votable());
        assert!(!ProposalState::Pending.is_votable());
    }

    #[test]
    fn by_proposer() {
        let mut reg = ProposalRegistry::new();
        reg.submit("alice", ProposalType::Custom { title: "a".into(), body: "".into() }, 0, 10, 0);
        reg.submit("alice", ProposalType::Custom { title: "b".into(), body: "".into() }, 0, 10, 0);
        reg.submit("bob", ProposalType::Custom { title: "c".into(), body: "".into() }, 0, 10, 0);

        assert_eq!(reg.by_proposer("alice").len(), 2);
        assert_eq!(reg.by_proposer("bob").len(), 1);
    }

    #[test]
    fn auto_increment_ids() {
        let mut reg = ProposalRegistry::new();
        let id1 = reg.submit("a", ProposalType::Custom { title: "".into(), body: "".into() }, 0, 10, 0);
        let id2 = reg.submit("a", ProposalType::Custom { title: "".into(), body: "".into() }, 0, 10, 0);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn proposal_types_submit_and_verify() {
        let mut reg = ProposalRegistry::new();

        // ParameterChange
        let id1 = reg.submit(
            "did:helm:abc",
            ProposalType::ParameterChange {
                parameter: "fee_bp".into(),
                old_value: "40".into(),
                new_value: "50".into(),
            },
            5, 15, 0,
        );
        let p1 = reg.get(id1).unwrap();
        assert!(matches!(p1.proposal_type, ProposalType::ParameterChange { .. }));
        assert_eq!(p1.state, ProposalState::Pending);

        // TreasurySpend
        let id2 = reg.submit(
            "did:helm:abc",
            ProposalType::TreasurySpend {
                recipient: "did:helm:xyz".into(),
                amount: 1000,
                project_name: "research".into(),
            },
            5, 15, 0,
        );
        let p2 = reg.get(id2).unwrap();
        assert!(matches!(p2.proposal_type, ProposalType::TreasurySpend { amount: 1000, .. }));

        // Upgrade
        let id3 = reg.submit(
            "did:helm:abc",
            ProposalType::Upgrade {
                version: "0.2.0".into(),
                description: "add feature".into(),
            },
            5, 15, 0,
        );
        let p3 = reg.get(id3).unwrap();
        assert!(matches!(p3.proposal_type, ProposalType::Upgrade { .. }));

        // All 3 by same proposer
        assert_eq!(reg.by_proposer("did:helm:abc").len(), 3);
        assert_eq!(reg.total(), 3);
    }

    #[test]
    fn has_voted_and_voter_count() {
        let mut p = Proposal {
            id: 1,
            proposer: "test".to_string(),
            proposal_type: ProposalType::Custom { title: "t".into(), body: "b".into() },
            state: ProposalState::Active,
            start_epoch: 0,
            end_epoch: 10,
            created_at: 0,
            votes_for: 100,
            votes_against: 0,
            votes: HashMap::new(),
            execution_result: None,
        };
        assert!(!p.has_voted("alice"));
        assert_eq!(p.voter_count(), 0);

        p.votes.insert("alice".to_string(), (true, 100));
        assert!(p.has_voted("alice"));
        assert!(!p.has_voted("bob"));
        assert_eq!(p.voter_count(), 1);
    }
}
