//! Voting Engine — stake-weighted voting with quorum and timelock.
//!
//! Voting power is proportional to staked HELM tokens.
//! Proposals require a quorum (minimum participation) and a threshold
//! (minimum approval percentage) to pass.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::proposal::{ProposalId, ProposalRegistry, ProposalState};

// --- Governance Defaults ---
/// Minimum stake required to submit a proposal.
pub const DEFAULT_MIN_PROPOSAL_STAKE: u128 = 1_000;
/// Default quorum: 10% of total stake must participate.
pub const DEFAULT_QUORUM: f64 = 0.1;
/// Default approval threshold: simple majority (51%).
pub const DEFAULT_APPROVAL_THRESHOLD: f64 = 0.51;
/// Default voting period in epochs.
pub const DEFAULT_VOTING_PERIOD_EPOCHS: u64 = 50;
/// Emergency voting period (shorter).
pub const DEFAULT_EMERGENCY_PERIOD_EPOCHS: u64 = 10;
/// Timelock: epochs between passing and execution.
pub const DEFAULT_TIMELOCK_EPOCHS: u64 = 5;

/// Governance configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceConfig {
    /// Minimum voting power to submit a proposal.
    pub min_proposal_stake: u128,
    /// Quorum: minimum fraction of total stake that must vote (0.0-1.0).
    pub quorum: f64,
    /// Approval threshold: minimum approval fraction to pass (0.0-1.0).
    pub approval_threshold: f64,
    /// Standard voting period in epochs.
    pub voting_period_epochs: u64,
    /// Emergency voting period (shorter).
    pub emergency_period_epochs: u64,
    /// Timelock: epochs between passing and execution.
    pub timelock_epochs: u64,
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            min_proposal_stake: DEFAULT_MIN_PROPOSAL_STAKE,
            quorum: DEFAULT_QUORUM,
            approval_threshold: DEFAULT_APPROVAL_THRESHOLD,
            voting_period_epochs: DEFAULT_VOTING_PERIOD_EPOCHS,
            emergency_period_epochs: DEFAULT_EMERGENCY_PERIOD_EPOCHS,
            timelock_epochs: DEFAULT_TIMELOCK_EPOCHS,
        }
    }
}

/// Voting engine — processes votes and determines outcomes.
pub struct VotingEngine {
    pub config: GovernanceConfig,
    /// Voter stakes: voter_id → voting power.
    stakes: HashMap<String, u128>,
    /// Total stake across all voters.
    total_stake: u128,
    /// Current epoch.
    current_epoch: u64,
}

impl VotingEngine {
    pub fn new(config: GovernanceConfig) -> Self {
        Self {
            config,
            stakes: HashMap::new(),
            total_stake: 0,
            current_epoch: 0,
        }
    }

    /// Set voting power for a voter (stake snapshot).
    pub fn set_stake(&mut self, voter: &str, power: u128) {
        let old = self.stakes.insert(voter.to_string(), power).unwrap_or(0);
        self.total_stake = self.total_stake.saturating_sub(old).saturating_add(power);
    }

    /// Remove a voter's stake.
    pub fn remove_stake(&mut self, voter: &str) {
        if let Some(old) = self.stakes.remove(voter) {
            self.total_stake = self.total_stake.saturating_sub(old);
        }
    }

    /// Get a voter's stake.
    pub fn get_stake(&self, voter: &str) -> u128 {
        self.stakes.get(voter).copied().unwrap_or(0)
    }

    /// Total registered stake.
    pub fn total_stake(&self) -> u128 {
        self.total_stake
    }

    /// Advance epoch and update proposal states.
    pub fn advance_epoch(&mut self, registry: &mut ProposalRegistry) {
        self.current_epoch += 1;

        // Activate pending proposals whose start_epoch has arrived
        let to_activate: Vec<ProposalId> = registry
            .all()
            .iter()
            .filter(|p| p.state == ProposalState::Pending && self.current_epoch >= p.start_epoch)
            .map(|p| p.id)
            .collect();

        for id in to_activate {
            if let Some(pm) = registry.get_mut(id) {
                pm.state = ProposalState::Active;
            }
        }

        // Finalize proposals whose end_epoch has passed
        let to_finalize: Vec<ProposalId> = registry
            .all()
            .iter()
            .filter(|p| p.state.is_votable() && self.current_epoch > p.end_epoch)
            .map(|p| p.id)
            .collect();

        for id in to_finalize {
            self.finalize(registry, id);
        }
    }

    /// Current epoch.
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Cast a vote on a proposal.
    pub fn vote(
        &self,
        registry: &mut ProposalRegistry,
        proposal_id: ProposalId,
        voter: &str,
        support: bool,
    ) -> Result<(), GovernanceError> {
        let stake = self.get_stake(voter);
        if stake == 0 {
            return Err(GovernanceError::NoVotingPower);
        }

        let proposal = registry
            .get(proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if !proposal.state.is_votable() {
            return Err(GovernanceError::NotVotable);
        }
        if proposal.has_voted(voter) {
            return Err(GovernanceError::AlreadyVoted);
        }

        let proposal = registry.get_mut(proposal_id).unwrap();
        proposal.votes.insert(voter.to_string(), (support, stake));
        if support {
            proposal.votes_for = proposal.votes_for.saturating_add(stake);
        } else {
            proposal.votes_against = proposal.votes_against.saturating_add(stake);
        }

        Ok(())
    }

    /// Finalize a proposal: check quorum + threshold → pass or reject.
    fn finalize(&self, registry: &mut ProposalRegistry, id: ProposalId) {
        let proposal = match registry.get(id) {
            Some(p) => p,
            None => return,
        };

        let total_voted = proposal.total_votes();
        let quorum_met = if self.total_stake == 0 {
            false
        } else {
            (total_voted as f64 / self.total_stake as f64) >= self.config.quorum
        };
        let threshold_met = proposal.approval_rate() >= self.config.approval_threshold;

        let new_state = if quorum_met && threshold_met {
            ProposalState::Passed
        } else {
            ProposalState::Rejected
        };

        if let Some(p) = registry.get_mut(id) {
            p.state = new_state;
        }
    }

    /// Execute a passed proposal (after timelock).
    pub fn execute(
        &self,
        registry: &mut ProposalRegistry,
        id: ProposalId,
    ) -> Result<(), GovernanceError> {
        let proposal = registry
            .get(id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.state != ProposalState::Passed {
            return Err(GovernanceError::NotPassed);
        }

        // Check timelock
        let earliest_exec = proposal.end_epoch.saturating_add(self.config.timelock_epochs);
        if self.current_epoch < earliest_exec {
            return Err(GovernanceError::TimelockActive(earliest_exec));
        }

        let proposal = registry.get_mut(id).unwrap();
        proposal.state = ProposalState::Executed;
        proposal.execution_result = Some("executed".to_string());

        Ok(())
    }

    /// Cancel a proposal (only by proposer, before it passes).
    pub fn cancel(
        &self,
        registry: &mut ProposalRegistry,
        id: ProposalId,
        caller: &str,
    ) -> Result<(), GovernanceError> {
        let proposal = registry
            .get(id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.proposer != caller {
            return Err(GovernanceError::Unauthorized);
        }
        if proposal.state.is_terminal() {
            return Err(GovernanceError::AlreadyFinalized);
        }

        let proposal = registry.get_mut(id).unwrap();
        proposal.state = ProposalState::Cancelled;
        Ok(())
    }
}

impl Default for VotingEngine {
    fn default() -> Self {
        Self::new(GovernanceConfig::default())
    }
}

/// Governance errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum GovernanceError {
    #[error("proposal not found")]
    ProposalNotFound,
    #[error("proposal is not in votable state")]
    NotVotable,
    #[error("voter has no staked tokens")]
    NoVotingPower,
    #[error("voter already voted on this proposal")]
    AlreadyVoted,
    #[error("proposal has not passed")]
    NotPassed,
    #[error("timelock active until epoch {0}")]
    TimelockActive(u64),
    #[error("unauthorized: not the proposer")]
    Unauthorized,
    #[error("proposal already finalized")]
    AlreadyFinalized,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proposal::ProposalType;

    const TEST_VOTING_PERIOD: u64 = 10;
    const TEST_TIMELOCK: u64 = 3;
    const ALICE_STAKE: u128 = 100;
    const BOB_STAKE: u128 = 50;
    const CAROL_STAKE: u128 = 30;

    fn setup() -> (VotingEngine, ProposalRegistry) {
        let mut engine = VotingEngine::new(GovernanceConfig {
            quorum: DEFAULT_QUORUM,
            approval_threshold: DEFAULT_APPROVAL_THRESHOLD,
            voting_period_epochs: TEST_VOTING_PERIOD,
            timelock_epochs: TEST_TIMELOCK,
            ..Default::default()
        });
        engine.set_stake("alice", ALICE_STAKE);
        engine.set_stake("bob", BOB_STAKE);
        engine.set_stake("carol", CAROL_STAKE);

        (engine, ProposalRegistry::new())
    }

    fn make_proposal(reg: &mut ProposalRegistry, start: u64, end: u64) -> ProposalId {
        reg.submit(
            "alice",
            ProposalType::Custom { title: "test".into(), body: "body".into() },
            start,
            end,
            0,
        )
    }

    /// Advance engine to a specific epoch.
    fn advance_to(engine: &mut VotingEngine, reg: &mut ProposalRegistry, target_epoch: u64) {
        while engine.current_epoch() < target_epoch {
            engine.advance_epoch(reg);
        }
    }

    #[test]
    fn set_and_get_stake() {
        let (engine, _) = setup();
        assert_eq!(engine.get_stake("alice"), ALICE_STAKE);
        assert_eq!(engine.get_stake("bob"), BOB_STAKE);
        assert_eq!(engine.get_stake("nobody"), 0);
        assert_eq!(engine.total_stake(), ALICE_STAKE + BOB_STAKE + CAROL_STAKE);
    }

    #[test]
    fn remove_stake() {
        let (mut engine, _) = setup();
        engine.remove_stake("bob");
        assert_eq!(engine.get_stake("bob"), 0);
        assert_eq!(engine.total_stake(), ALICE_STAKE + CAROL_STAKE);
    }

    #[test]
    fn vote_for() {
        let (mut engine, mut reg) = setup();
        // Proposal starts at epoch 1, ends at epoch 5
        let id = make_proposal(&mut reg, 1, 5);
        // Drive state through real epoch advance
        advance_to(&mut engine, &mut reg, 1);
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Active);

        engine.vote(&mut reg, id, "alice", true).unwrap();
        let p = reg.get(id).unwrap();
        assert_eq!(p.votes_for, ALICE_STAKE);
        assert_eq!(p.votes_against, 0);
        assert_eq!(p.voter_count(), 1);
        assert!(p.has_voted("alice"));
    }

    #[test]
    fn vote_against() {
        let (mut engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 1, 5);
        advance_to(&mut engine, &mut reg, 1);

        engine.vote(&mut reg, id, "bob", false).unwrap();
        let p = reg.get(id).unwrap();
        assert_eq!(p.votes_for, 0);
        assert_eq!(p.votes_against, BOB_STAKE);
    }

    #[test]
    fn double_vote_rejected() {
        let (mut engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 1, 5);
        advance_to(&mut engine, &mut reg, 1);

        engine.vote(&mut reg, id, "alice", true).unwrap();
        let result = engine.vote(&mut reg, id, "alice", false);
        assert!(matches!(result, Err(GovernanceError::AlreadyVoted)));
    }

    #[test]
    fn vote_no_stake_rejected() {
        let (mut engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 1, 5);
        advance_to(&mut engine, &mut reg, 1);

        let result = engine.vote(&mut reg, id, "nobody", true);
        assert!(matches!(result, Err(GovernanceError::NoVotingPower)));
    }

    #[test]
    fn vote_on_pending_rejected() {
        let (engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 5, 15);
        // Still epoch 0 — proposal starts at 5, so it's Pending
        let result = engine.vote(&mut reg, id, "alice", true);
        assert!(matches!(result, Err(GovernanceError::NotVotable)));
    }

    #[test]
    fn advance_activates_proposals() {
        let (mut engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 3, 13);
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Pending);

        // Advance to epoch 3
        engine.advance_epoch(&mut reg); // epoch 1
        engine.advance_epoch(&mut reg); // epoch 2
        engine.advance_epoch(&mut reg); // epoch 3

        assert_eq!(reg.get(id).unwrap().state, ProposalState::Active);
    }

    #[test]
    fn advance_finalizes_passed() {
        let (mut engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 1, 5);

        // Advance to activate
        advance_to(&mut engine, &mut reg, 1);
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Active);

        // Vote: alice(100) + bob(50) = 150 for, quorum = 10% of 180 = 18
        engine.vote(&mut reg, id, "alice", true).unwrap();
        engine.vote(&mut reg, id, "bob", true).unwrap();

        let p = reg.get(id).unwrap();
        assert_eq!(p.total_votes(), ALICE_STAKE + BOB_STAKE);
        assert!((p.approval_rate() - 1.0).abs() < f64::EPSILON);

        // Advance past end_epoch (5) — epoch 6 triggers finalization
        advance_to(&mut engine, &mut reg, 6);

        assert_eq!(reg.get(id).unwrap().state, ProposalState::Passed);
    }

    #[test]
    fn advance_finalizes_rejected_no_quorum() {
        let (mut engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 1, 5);

        advance_to(&mut engine, &mut reg, 1);
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Active);

        // Nobody votes → no quorum
        advance_to(&mut engine, &mut reg, 6);

        assert_eq!(reg.get(id).unwrap().state, ProposalState::Rejected);
        assert_eq!(reg.get(id).unwrap().total_votes(), 0);
    }

    #[test]
    fn advance_finalizes_with_votes() {
        let (mut engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 1, 5);
        advance_to(&mut engine, &mut reg, 1);

        // alice for (100), bob+carol against (80)
        engine.vote(&mut reg, id, "alice", true).unwrap();
        engine.vote(&mut reg, id, "bob", false).unwrap();
        engine.vote(&mut reg, id, "carol", false).unwrap();

        advance_to(&mut engine, &mut reg, 6);

        // 100/(100+80) = 55.5% > 51% threshold, quorum met → Passed
        let p = reg.get(id).unwrap();
        assert_eq!(p.state, ProposalState::Passed);
        assert_eq!(p.voter_count(), 3);
    }

    #[test]
    fn majority_against_rejects() {
        let (mut engine, mut reg) = setup();

        // Give bob+carol more weight so majority is against
        engine.set_stake("bob", 200);
        engine.set_stake("carol", 200);

        let id = make_proposal(&mut reg, 1, 5);
        advance_to(&mut engine, &mut reg, 1);

        engine.vote(&mut reg, id, "alice", true).unwrap();  // 100 for
        engine.vote(&mut reg, id, "bob", false).unwrap();   // 200 against
        engine.vote(&mut reg, id, "carol", false).unwrap();  // 200 against

        advance_to(&mut engine, &mut reg, 6);

        // 100/500 = 20% < 51% → Rejected
        let p = reg.get(id).unwrap();
        assert_eq!(p.state, ProposalState::Rejected);
        assert!((p.approval_rate() - 0.2).abs() < 0.01);
    }

    #[test]
    fn execute_after_timelock() {
        let (mut engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 1, 5);
        advance_to(&mut engine, &mut reg, 1);

        engine.vote(&mut reg, id, "alice", true).unwrap();
        engine.vote(&mut reg, id, "bob", true).unwrap();

        // Advance past end (epoch 6) → finalized
        advance_to(&mut engine, &mut reg, 6);
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Passed);

        // Too early to execute (timelock = 3 epochs, earliest = 5 + 3 = 8)
        let result = engine.execute(&mut reg, id);
        assert!(matches!(result, Err(GovernanceError::TimelockActive(8))));

        // Advance past timelock
        advance_to(&mut engine, &mut reg, 8);

        engine.execute(&mut reg, id).unwrap();
        let p = reg.get(id).unwrap();
        assert_eq!(p.state, ProposalState::Executed);
        assert_eq!(p.execution_result.as_deref(), Some("executed"));
    }

    #[test]
    fn execute_before_passed_fails() {
        let (mut engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 1, 5);
        advance_to(&mut engine, &mut reg, 1);

        // Try to execute while still Active
        let result = engine.execute(&mut reg, id);
        assert!(matches!(result, Err(GovernanceError::NotPassed)));
    }

    #[test]
    fn cancel_proposal() {
        let (engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 5, 15);

        engine.cancel(&mut reg, id, "alice").unwrap();
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Cancelled);
    }

    #[test]
    fn cancel_unauthorized() {
        let (engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 5, 15);

        let result = engine.cancel(&mut reg, id, "bob");
        assert!(matches!(result, Err(GovernanceError::Unauthorized)));
    }

    #[test]
    fn cancel_finalized_fails() {
        let (mut engine, mut reg) = setup();
        let id = make_proposal(&mut reg, 1, 5);
        advance_to(&mut engine, &mut reg, 1);
        engine.vote(&mut reg, id, "alice", true).unwrap();
        advance_to(&mut engine, &mut reg, 6);
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Passed);

        // Execute after timelock (end=5 + timelock=3 = 8) to reach terminal state
        advance_to(&mut engine, &mut reg, 8);
        engine.execute(&mut reg, id).unwrap();
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Executed);

        // Now cancel should fail — Executed is terminal
        let result = engine.cancel(&mut reg, id, "alice");
        assert!(matches!(result, Err(GovernanceError::AlreadyFinalized)));
    }

    #[test]
    fn config_default_uses_constants() {
        let config = GovernanceConfig::default();
        assert_eq!(config.voting_period_epochs, DEFAULT_VOTING_PERIOD_EPOCHS);
        assert_eq!(config.emergency_period_epochs, DEFAULT_EMERGENCY_PERIOD_EPOCHS);
        assert_eq!(config.timelock_epochs, DEFAULT_TIMELOCK_EPOCHS);
        assert!((config.quorum - DEFAULT_QUORUM).abs() < f64::EPSILON);
        assert!((config.approval_threshold - DEFAULT_APPROVAL_THRESHOLD).abs() < f64::EPSILON);
        assert_eq!(config.min_proposal_stake, DEFAULT_MIN_PROPOSAL_STAKE);
    }

    #[test]
    fn active_proposals_filter_via_epoch() {
        let (mut engine, mut reg) = setup();
        make_proposal(&mut reg, 5, 15); // won't activate until epoch 5
        make_proposal(&mut reg, 1, 10); // will activate at epoch 1

        // Before any advance: both pending
        assert_eq!(reg.active_proposals().len(), 0);

        advance_to(&mut engine, &mut reg, 1);
        // Only second proposal is active
        assert_eq!(reg.active_proposals().len(), 1);

        advance_to(&mut engine, &mut reg, 5);
        // Both active
        assert_eq!(reg.active_proposals().len(), 2);
    }

    #[test]
    fn vote_on_nonexistent_proposal() {
        let (engine, mut reg) = setup();
        let result = engine.vote(&mut reg, 999, "alice", true);
        assert!(matches!(result, Err(GovernanceError::ProposalNotFound)));
    }

    #[test]
    fn full_lifecycle_submit_vote_pass_execute() {
        let (mut engine, mut reg) = setup();
        // Submit
        let id = make_proposal(&mut reg, 1, 5);
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Pending);

        // Activate
        advance_to(&mut engine, &mut reg, 1);
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Active);

        // Vote
        engine.vote(&mut reg, id, "alice", true).unwrap();
        engine.vote(&mut reg, id, "bob", true).unwrap();

        // Finalize
        advance_to(&mut engine, &mut reg, 6);
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Passed);

        // Execute after timelock (end=5 + timelock=3 = 8)
        advance_to(&mut engine, &mut reg, 8);
        engine.execute(&mut reg, id).unwrap();
        assert_eq!(reg.get(id).unwrap().state, ProposalState::Executed);
        assert!(reg.get(id).unwrap().state.is_terminal());
    }
}
