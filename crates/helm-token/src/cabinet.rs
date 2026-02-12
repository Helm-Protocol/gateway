//! Cabinet — protocol governance departments with voting-based funding.
//!
//! The Cabinet manages Helm Protocol operations through departments:
//! Finance, Research, Security, Technology, Operations, Legal.
//!
//! - 2.5% token allocation with indefinite lockup
//! - DeFi revenue pays member salaries
//! - Project funding via member voting from capital pool or DeFi revenue
//! - Each department has a lead and members

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::token::{TokenAmount, TokenError};
use crate::wallet::Address;

/// Cabinet departments responsible for protocol operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Department {
    Finance,
    Research,
    Security,
    Technology,
    Operations,
    Legal,
}

impl Department {
    pub fn all() -> &'static [Department] {
        &[
            Self::Finance,
            Self::Research,
            Self::Security,
            Self::Technology,
            Self::Operations,
            Self::Legal,
        ]
    }
}

impl std::fmt::Display for Department {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finance => write!(f, "Finance"),
            Self::Research => write!(f, "Research"),
            Self::Security => write!(f, "Security"),
            Self::Technology => write!(f, "Technology"),
            Self::Operations => write!(f, "Operations"),
            Self::Legal => write!(f, "Legal"),
        }
    }
}

/// Role within a department.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CabinetRole {
    Lead,
    Member,
}

/// A cabinet member.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CabinetMember {
    pub address: Address,
    pub department: Department,
    pub role: CabinetRole,
    /// Monthly salary in tokens (paid from DeFi revenue).
    pub monthly_salary: TokenAmount,
    pub joined_epoch: u64,
    pub active: bool,
}

/// Proposal status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalStatus {
    /// Open for voting.
    Active,
    /// Approved (met quorum and majority).
    Approved,
    /// Rejected (failed to meet quorum or majority).
    Rejected,
    /// Executed (funds disbursed).
    Executed,
    /// Expired (voting period ended without quorum).
    Expired,
}

/// Funding source for proposals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FundingSource {
    /// From the treasury capital pool.
    CapitalPool,
    /// From accumulated DeFi revenue.
    DefiRevenue,
}

/// A funding proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub department: Department,
    pub title: String,
    pub description: String,
    pub amount: TokenAmount,
    pub funding_source: FundingSource,
    pub status: ProposalStatus,
    /// Epoch when voting ends.
    pub voting_deadline: u64,
    /// Votes: address → approve(true)/reject(false).
    pub votes: HashMap<String, bool>,
    pub created_epoch: u64,
}

/// The Cabinet system.
#[derive(Debug)]
pub struct Cabinet {
    members: Vec<CabinetMember>,
    proposals: Vec<Proposal>,
    next_proposal_id: u64,
    current_epoch: u64,
    /// Minimum votes required (as fraction of total members).
    /// Default: 50% quorum.
    pub quorum_bp: u32,
    /// Approval threshold in basis points of votes cast.
    /// Default: 6000 = 60% majority.
    pub approval_threshold_bp: u32,
    /// Voting period in epochs.
    pub voting_period: u64,
}

impl Cabinet {
    pub fn new() -> Self {
        Self {
            members: Vec::new(),
            proposals: Vec::new(),
            next_proposal_id: 1,
            current_epoch: 0,
            quorum_bp: 5000,          // 50%
            approval_threshold_bp: 6000, // 60%
            voting_period: 30,         // 30 epochs
        }
    }

    pub fn advance_epoch(&mut self) {
        self.current_epoch += 1;
    }

    /// Add a cabinet member.
    pub fn add_member(
        &mut self,
        address: Address,
        department: Department,
        role: CabinetRole,
        monthly_salary: TokenAmount,
    ) -> Result<(), TokenError> {
        // Check for duplicate
        if self.members.iter().any(|m| m.address == address && m.active) {
            return Err(TokenError::InvalidAmount(format!(
                "member {} already active",
                address
            )));
        }

        self.members.push(CabinetMember {
            address,
            department,
            role,
            monthly_salary,
            joined_epoch: self.current_epoch,
            active: true,
        });

        Ok(())
    }

    /// Remove (deactivate) a cabinet member.
    pub fn remove_member(&mut self, address: &Address) -> Result<(), TokenError> {
        let member = self
            .members
            .iter_mut()
            .find(|m| m.address == *address && m.active)
            .ok_or_else(|| TokenError::NotFound(format!("member {}", address)))?;

        member.active = false;
        Ok(())
    }

    /// Get all active members.
    pub fn active_members(&self) -> Vec<&CabinetMember> {
        self.members.iter().filter(|m| m.active).collect()
    }

    /// Get active members of a department.
    pub fn department_members(&self, dept: Department) -> Vec<&CabinetMember> {
        self.members
            .iter()
            .filter(|m| m.active && m.department == dept)
            .collect()
    }

    /// Total monthly salary obligation.
    pub fn total_monthly_salary(&self) -> TokenAmount {
        self.active_members()
            .iter()
            .fold(TokenAmount::ZERO, |acc, m| {
                TokenAmount(acc.0 + m.monthly_salary.0)
            })
    }

    /// Calculate individual salary payments for the current period.
    pub fn calculate_salaries(&self) -> Vec<(Address, TokenAmount)> {
        self.active_members()
            .iter()
            .map(|m| (m.address.clone(), m.monthly_salary))
            .collect()
    }

    /// Create a funding proposal.
    pub fn create_proposal(
        &mut self,
        proposer: &Address,
        department: Department,
        title: &str,
        description: &str,
        amount: TokenAmount,
        funding_source: FundingSource,
    ) -> Result<u64, TokenError> {
        // Proposer must be an active member
        if !self.members.iter().any(|m| m.address == *proposer && m.active) {
            return Err(TokenError::Unauthorized(
                "only active cabinet members can propose".into(),
            ));
        }

        let id = self.next_proposal_id;
        self.next_proposal_id += 1;

        self.proposals.push(Proposal {
            id,
            proposer: proposer.clone(),
            department,
            title: title.to_string(),
            description: description.to_string(),
            amount,
            funding_source,
            status: ProposalStatus::Active,
            voting_deadline: self.current_epoch + self.voting_period,
            votes: HashMap::new(),
            created_epoch: self.current_epoch,
        });

        Ok(id)
    }

    /// Vote on a proposal.
    pub fn vote(
        &mut self,
        voter: &Address,
        proposal_id: u64,
        approve: bool,
    ) -> Result<(), TokenError> {
        // Voter must be active member
        if !self.members.iter().any(|m| m.address == *voter && m.active) {
            return Err(TokenError::Unauthorized("only active members can vote".into()));
        }

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or_else(|| TokenError::NotFound(format!("proposal {}", proposal_id)))?;

        if proposal.status != ProposalStatus::Active {
            return Err(TokenError::VoteError(format!(
                "proposal {} is {:?}, not Active",
                proposal_id, proposal.status
            )));
        }

        if self.current_epoch > proposal.voting_deadline {
            return Err(TokenError::VoteError("voting period has ended".into()));
        }

        // One vote per member
        if proposal.votes.contains_key(&voter.0) {
            return Err(TokenError::VoteError("already voted".into()));
        }

        proposal.votes.insert(voter.0.clone(), approve);
        Ok(())
    }

    /// Tally votes and finalize a proposal.
    pub fn finalize_proposal(&mut self, proposal_id: u64) -> Result<ProposalStatus, TokenError> {
        let active_count = self.active_members().len();
        if active_count == 0 {
            return Err(TokenError::VoteError("no active members".into()));
        }

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or_else(|| TokenError::NotFound(format!("proposal {}", proposal_id)))?;

        if proposal.status != ProposalStatus::Active {
            return Err(TokenError::VoteError(format!(
                "proposal {} already finalized",
                proposal_id
            )));
        }

        let total_votes = proposal.votes.len();
        let approve_votes = proposal.votes.values().filter(|&&v| v).count();

        // Check quorum
        let quorum_needed = active_count * self.quorum_bp as usize / 10_000;
        if total_votes < quorum_needed.max(1) {
            if self.current_epoch > proposal.voting_deadline {
                proposal.status = ProposalStatus::Expired;
                return Ok(ProposalStatus::Expired);
            }
            return Err(TokenError::VoteError(format!(
                "quorum not met: {}/{} votes, need {}",
                total_votes, active_count, quorum_needed
            )));
        }

        // Check approval threshold
        let approval_needed = total_votes * self.approval_threshold_bp as usize / 10_000;
        if approve_votes >= approval_needed.max(1) {
            proposal.status = ProposalStatus::Approved;
            Ok(ProposalStatus::Approved)
        } else {
            proposal.status = ProposalStatus::Rejected;
            Ok(ProposalStatus::Rejected)
        }
    }

    /// Mark a proposal as executed (after funds are disbursed).
    pub fn mark_executed(&mut self, proposal_id: u64) -> Result<(), TokenError> {
        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or_else(|| TokenError::NotFound(format!("proposal {}", proposal_id)))?;

        if proposal.status != ProposalStatus::Approved {
            return Err(TokenError::VoteError(
                "can only execute approved proposals".into(),
            ));
        }

        proposal.status = ProposalStatus::Executed;
        Ok(())
    }

    /// Get a proposal by ID.
    pub fn get_proposal(&self, id: u64) -> Option<&Proposal> {
        self.proposals.iter().find(|p| p.id == id)
    }

    /// All proposals.
    pub fn proposals(&self) -> &[Proposal] {
        &self.proposals
    }

    /// Active (open) proposals.
    pub fn active_proposals(&self) -> Vec<&Proposal> {
        self.proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Active)
            .collect()
    }

    /// Member count.
    pub fn member_count(&self) -> usize {
        self.active_members().len()
    }
}

impl Default for Cabinet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> Address {
        Address(format!("{:0>64}", s))
    }

    fn setup_cabinet() -> Cabinet {
        let mut cab = Cabinet::new();
        cab.add_member(
            addr("lead1"),
            Department::Finance,
            CabinetRole::Lead,
            TokenAmount::from_tokens(10_000),
        )
        .unwrap();
        cab.add_member(
            addr("mem1"),
            Department::Finance,
            CabinetRole::Member,
            TokenAmount::from_tokens(5_000),
        )
        .unwrap();
        cab.add_member(
            addr("lead2"),
            Department::Technology,
            CabinetRole::Lead,
            TokenAmount::from_tokens(10_000),
        )
        .unwrap();
        cab.add_member(
            addr("mem2"),
            Department::Technology,
            CabinetRole::Member,
            TokenAmount::from_tokens(5_000),
        )
        .unwrap();
        cab.add_member(
            addr("sec"),
            Department::Security,
            CabinetRole::Lead,
            TokenAmount::from_tokens(8_000),
        )
        .unwrap();
        cab
    }

    #[test]
    fn add_member() {
        let cab = setup_cabinet();
        assert_eq!(cab.member_count(), 5);
    }

    #[test]
    fn duplicate_member_fails() {
        let mut cab = Cabinet::new();
        cab.add_member(
            addr("aa"),
            Department::Finance,
            CabinetRole::Lead,
            TokenAmount::from_tokens(1000),
        )
        .unwrap();

        assert!(cab
            .add_member(
                addr("aa"),
                Department::Research,
                CabinetRole::Member,
                TokenAmount::from_tokens(500),
            )
            .is_err());
    }

    #[test]
    fn remove_member() {
        let mut cab = setup_cabinet();
        cab.remove_member(&addr("mem1")).unwrap();
        assert_eq!(cab.member_count(), 4);
    }

    #[test]
    fn department_members() {
        let cab = setup_cabinet();
        let finance = cab.department_members(Department::Finance);
        assert_eq!(finance.len(), 2);

        let security = cab.department_members(Department::Security);
        assert_eq!(security.len(), 1);
    }

    #[test]
    fn total_monthly_salary() {
        let cab = setup_cabinet();
        // 10k + 5k + 10k + 5k + 8k = 38k
        assert_eq!(cab.total_monthly_salary().whole_tokens(), 38_000);
    }

    #[test]
    fn calculate_salaries() {
        let cab = setup_cabinet();
        let salaries = cab.calculate_salaries();
        assert_eq!(salaries.len(), 5);
    }

    #[test]
    fn create_and_vote_proposal() {
        let mut cab = setup_cabinet();
        let proposal_id = cab
            .create_proposal(
                &addr("lead1"),
                Department::Finance,
                "Security Audit",
                "Fund external security audit",
                TokenAmount::from_tokens(50_000),
                FundingSource::CapitalPool,
            )
            .unwrap();

        assert_eq!(proposal_id, 1);

        // Vote: 3 approve, 1 reject, 1 abstain
        cab.vote(&addr("lead1"), 1, true).unwrap();
        cab.vote(&addr("mem1"), 1, true).unwrap();
        cab.vote(&addr("lead2"), 1, true).unwrap();
        cab.vote(&addr("mem2"), 1, false).unwrap();

        // Finalize: 4 voted / 5 members = 80% quorum ✓, 3/4 approve = 75% > 60% ✓
        let status = cab.finalize_proposal(1).unwrap();
        assert_eq!(status, ProposalStatus::Approved);
    }

    #[test]
    fn proposal_rejected_insufficient_approval() {
        let mut cab = setup_cabinet();
        cab.create_proposal(
            &addr("lead1"),
            Department::Finance,
            "Bad Idea",
            "Questionable spending",
            TokenAmount::from_tokens(1_000_000),
            FundingSource::CapitalPool,
        )
        .unwrap();

        // 1 approve, 3 reject
        cab.vote(&addr("lead1"), 1, true).unwrap();
        cab.vote(&addr("mem1"), 1, false).unwrap();
        cab.vote(&addr("lead2"), 1, false).unwrap();
        cab.vote(&addr("mem2"), 1, false).unwrap();

        let status = cab.finalize_proposal(1).unwrap();
        assert_eq!(status, ProposalStatus::Rejected);
    }

    #[test]
    fn proposal_double_vote_fails() {
        let mut cab = setup_cabinet();
        cab.create_proposal(
            &addr("lead1"),
            Department::Finance,
            "Test",
            "Test",
            TokenAmount::from_tokens(100),
            FundingSource::DefiRevenue,
        )
        .unwrap();

        cab.vote(&addr("lead1"), 1, true).unwrap();
        assert!(cab.vote(&addr("lead1"), 1, false).is_err());
    }

    #[test]
    fn non_member_cannot_propose() {
        let mut cab = setup_cabinet();
        assert!(cab
            .create_proposal(
                &addr("outsider"),
                Department::Finance,
                "Hack",
                "steal funds",
                TokenAmount::from_tokens(999_999),
                FundingSource::CapitalPool,
            )
            .is_err());
    }

    #[test]
    fn non_member_cannot_vote() {
        let mut cab = setup_cabinet();
        cab.create_proposal(
            &addr("lead1"),
            Department::Finance,
            "Test",
            "Test",
            TokenAmount::from_tokens(100),
            FundingSource::CapitalPool,
        )
        .unwrap();

        assert!(cab.vote(&addr("outsider"), 1, true).is_err());
    }

    #[test]
    fn mark_executed() {
        let mut cab = setup_cabinet();
        cab.create_proposal(
            &addr("lead1"),
            Department::Finance,
            "Grant",
            "Dev grant",
            TokenAmount::from_tokens(10_000),
            FundingSource::CapitalPool,
        )
        .unwrap();

        cab.vote(&addr("lead1"), 1, true).unwrap();
        cab.vote(&addr("mem1"), 1, true).unwrap();
        cab.vote(&addr("lead2"), 1, true).unwrap();

        cab.finalize_proposal(1).unwrap();
        cab.mark_executed(1).unwrap();

        assert_eq!(
            cab.get_proposal(1).unwrap().status,
            ProposalStatus::Executed
        );
    }

    #[test]
    fn cannot_execute_unapproved() {
        let mut cab = setup_cabinet();
        cab.create_proposal(
            &addr("lead1"),
            Department::Finance,
            "Test",
            "Test",
            TokenAmount::from_tokens(100),
            FundingSource::CapitalPool,
        )
        .unwrap();

        assert!(cab.mark_executed(1).is_err());
    }

    #[test]
    fn active_proposals() {
        let mut cab = setup_cabinet();
        cab.create_proposal(
            &addr("lead1"),
            Department::Finance,
            "P1",
            "d1",
            TokenAmount::from_tokens(100),
            FundingSource::CapitalPool,
        )
        .unwrap();
        cab.create_proposal(
            &addr("lead2"),
            Department::Technology,
            "P2",
            "d2",
            TokenAmount::from_tokens(200),
            FundingSource::DefiRevenue,
        )
        .unwrap();

        assert_eq!(cab.active_proposals().len(), 2);

        // Finalize first
        cab.vote(&addr("lead1"), 1, true).unwrap();
        cab.vote(&addr("mem1"), 1, true).unwrap();
        cab.vote(&addr("lead2"), 1, true).unwrap();
        cab.finalize_proposal(1).unwrap();

        assert_eq!(cab.active_proposals().len(), 1);
    }

    #[test]
    fn department_display() {
        assert_eq!(Department::Finance.to_string(), "Finance");
        assert_eq!(Department::Security.to_string(), "Security");
        assert_eq!(Department::Technology.to_string(), "Technology");
    }

    #[test]
    fn all_departments() {
        assert_eq!(Department::all().len(), 6);
    }

    #[test]
    fn funding_source_variants() {
        let cp = FundingSource::CapitalPool;
        let dr = FundingSource::DefiRevenue;
        assert_ne!(cp, dr);
    }
}
