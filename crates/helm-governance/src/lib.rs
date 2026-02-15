//! # Helm Governance
//!
//! On-chain governance for the Helm Protocol: proposals, stake-weighted voting,
//! quorum, timelock, and parameter tuning.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │              helm-governance                  │
//! │                                               │
//! │  ┌───────────┐  ┌───────────┐  ┌──────────┐  │
//! │  │ Proposals │  │  Voting   │  │  Plugin  │  │
//! │  │ 5 types   │  │ stake-   │  │ EventLoop│  │
//! │  │ lifecycle │  │ weighted  │  │ bus msgs │  │
//! │  └─────┬─────┘  └────┬─────┘  └────┬─────┘  │
//! │        │              │             │         │
//! │  ┌─────▼──────────────▼─────────────▼──────┐  │
//! │  │     ProposalRegistry + VotingEngine      │  │
//! │  │  quorum 10% │ threshold 51% │ timelock  │  │
//! │  └──────────────────────────────────────────┘  │
//! └──────────────────────────────────────────────┘
//! ```
//!
//! ## Proposal Types
//!
//! | Type            | Description                               |
//! |-----------------|-------------------------------------------|
//! | ParameterChange | Modify protocol parameters                |
//! | TreasurySpend   | Fund a project from treasury CapitalPool  |
//! | Upgrade         | Protocol version upgrade                  |
//! | Emergency       | Fast-track emergency actions               |
//! | Custom          | Free-form governance proposals             |

pub mod proposal;
pub mod voting;
pub mod plugin;

// Re-exports
pub use proposal::{ProposalId, ProposalState, ProposalType, Proposal, ProposalRegistry};
pub use voting::{
    GovernanceConfig, GovernanceError, VotingEngine,
    DEFAULT_QUORUM, DEFAULT_APPROVAL_THRESHOLD,
    DEFAULT_VOTING_PERIOD_EPOCHS, DEFAULT_TIMELOCK_EPOCHS,
    DEFAULT_EMERGENCY_PERIOD_EPOCHS, DEFAULT_MIN_PROPOSAL_STAKE,
};
pub use plugin::{
    GovernancePlugin,
    PLUGIN_NAME as GOVERNANCE_PLUGIN_NAME,
    EVENT_SUBMIT_PROPOSAL, EVENT_PROPOSAL_SUBMITTED,
    EVENT_VOTE, EVENT_VOTE_RESULT,
};
