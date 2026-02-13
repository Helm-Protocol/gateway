//! 7-Category Agent Mining — contribution-based reward distribution.
//!
//! Agents earn mining rewards across 7 categories, each weighted to
//! incentivize the behaviors most valuable to the Helm network.
//!
//! | Category    | Weight | Description                         |
//! |-------------|--------|-------------------------------------|
//! | ServiceFee  | 30%    | Revenue from serving API/compute    |
//! | Code        | 15%    | Code contributions, bug fixes       |
//! | Staking     | 15%    | Proportional to staked amount       |
//! | Governance  | 10%    | Proposal voting, parameter tuning   |
//! | PeerReview  | 10%    | Verifying other agents' work        |
//! | Security    | 10%    | Vulnerability reports, audits       |
//! | Relay       | 10%    | Network relay, message forwarding   |
//!
//! Each epoch, the Mining Pool distributes rewards proportionally
//! to each agent's contribution score within each category.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Mining categories and their weights (basis points, total = 10000).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MiningCategory {
    /// Revenue from serving API requests and compute tasks.
    ServiceFee,
    /// Code contributions, patches, modules.
    Code,
    /// Staking-proportional rewards.
    Staking,
    /// Governance participation (voting, proposals).
    Governance,
    /// Peer review of other agents' outputs.
    PeerReview,
    /// Security audits, vulnerability reports.
    Security,
    /// Network relay and message forwarding.
    Relay,
}

impl MiningCategory {
    /// All categories in order.
    pub fn all() -> &'static [MiningCategory] {
        &[
            MiningCategory::ServiceFee,
            MiningCategory::Code,
            MiningCategory::Staking,
            MiningCategory::Governance,
            MiningCategory::PeerReview,
            MiningCategory::Security,
            MiningCategory::Relay,
        ]
    }

    /// Default weight in basis points.
    pub fn default_weight_bp(&self) -> u32 {
        match self {
            MiningCategory::ServiceFee => 3000,
            MiningCategory::Code => 1500,
            MiningCategory::Staking => 1500,
            MiningCategory::Governance => 1000,
            MiningCategory::PeerReview => 1000,
            MiningCategory::Security => 1000,
            MiningCategory::Relay => 1000,
        }
    }
}

/// A contribution event recorded for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    /// Agent that contributed.
    pub agent_id: String,
    /// Category of the contribution.
    pub category: MiningCategory,
    /// Magnitude of the contribution (category-specific units).
    pub amount: u64,
    /// Epoch in which this was recorded.
    pub epoch: u64,
    /// Optional description.
    pub memo: String,
}

/// Per-agent contribution scores for one epoch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentContributions {
    /// Score per category for this epoch.
    pub scores: HashMap<MiningCategory, u64>,
}

impl AgentContributions {
    pub fn record(&mut self, category: MiningCategory, amount: u64) {
        *self.scores.entry(category).or_insert(0) += amount;
    }

    pub fn score(&self, category: MiningCategory) -> u64 {
        self.scores.get(&category).copied().unwrap_or(0)
    }

    pub fn total_score(&self) -> u64 {
        self.scores.values().sum()
    }
}

/// Mining reward for a single agent in one epoch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningReward {
    pub agent_id: String,
    pub epoch: u64,
    /// Reward per category (base units).
    pub per_category: HashMap<MiningCategory, u128>,
    /// Total reward.
    pub total: u128,
}

/// Agent Mining Pool — tracks contributions and computes rewards.
pub struct MiningPool {
    /// Category weights in basis points (must sum to 10000).
    weights: HashMap<MiningCategory, u32>,
    /// Current epoch contributions: agent_id → scores.
    current_epoch: HashMap<String, AgentContributions>,
    /// Historical rewards per epoch.
    history: Vec<Vec<MiningReward>>,
    /// Current epoch number.
    epoch: u64,
    /// Total rewards distributed (base units).
    pub total_distributed: u128,
}

impl MiningPool {
    pub fn new() -> Self {
        let weights = MiningCategory::all()
            .iter()
            .map(|c| (*c, c.default_weight_bp()))
            .collect();

        Self {
            weights,
            current_epoch: HashMap::new(),
            history: Vec::new(),
            epoch: 0,
            total_distributed: 0,
        }
    }

    /// Create with custom category weights.
    pub fn with_weights(weights: HashMap<MiningCategory, u32>) -> Result<Self, MiningError> {
        let total: u32 = weights.values().sum();
        if total != 10_000 {
            return Err(MiningError::InvalidWeights(total));
        }
        Ok(Self {
            weights,
            current_epoch: HashMap::new(),
            history: Vec::new(),
            epoch: 0,
            total_distributed: 0,
        })
    }

    /// Record a contribution for an agent in the current epoch.
    pub fn record(
        &mut self,
        agent_id: &str,
        category: MiningCategory,
        amount: u64,
        memo: &str,
    ) {
        let _ = memo; // stored in events, not in pool state
        self.current_epoch
            .entry(agent_id.to_string())
            .or_default()
            .record(category, amount);
    }

    /// Get an agent's current epoch contributions.
    pub fn contributions(&self, agent_id: &str) -> Option<&AgentContributions> {
        self.current_epoch.get(agent_id)
    }

    /// Distribute rewards for the current epoch.
    ///
    /// `epoch_reward` is the total mining reward pool for this epoch (base units).
    /// Returns the list of rewards per agent.
    pub fn distribute(&mut self, epoch_reward: u128) -> Vec<MiningReward> {
        if epoch_reward == 0 || self.current_epoch.is_empty() {
            self.advance_epoch();
            return Vec::new();
        }

        // Per-category totals across all agents
        let mut category_totals: HashMap<MiningCategory, u64> = HashMap::new();
        for contributions in self.current_epoch.values() {
            for (&cat, &score) in &contributions.scores {
                *category_totals.entry(cat).or_insert(0) += score;
            }
        }

        // Per-category reward pool
        let mut category_pools: HashMap<MiningCategory, u128> = HashMap::new();
        for (&cat, &weight_bp) in &self.weights {
            let pool = proportional_u128(epoch_reward, weight_bp as u128, 10_000);
            category_pools.insert(cat, pool);
        }

        // Compute per-agent rewards
        let mut rewards = Vec::new();

        for (agent_id, contributions) in &self.current_epoch {
            let mut per_category = HashMap::new();
            let mut total: u128 = 0;

            for &cat in MiningCategory::all() {
                let agent_score = contributions.score(cat);
                let cat_total = category_totals.get(&cat).copied().unwrap_or(0);
                let cat_pool = category_pools.get(&cat).copied().unwrap_or(0);

                if agent_score == 0 || cat_total == 0 || cat_pool == 0 {
                    continue;
                }

                let reward = proportional_u128(cat_pool, agent_score as u128, cat_total as u128);
                if reward > 0 {
                    per_category.insert(cat, reward);
                    total += reward;
                }
            }

            if total > 0 {
                rewards.push(MiningReward {
                    agent_id: agent_id.clone(),
                    epoch: self.epoch,
                    per_category,
                    total,
                });
            }
        }

        self.total_distributed += rewards.iter().map(|r| r.total).sum::<u128>();
        self.history.push(rewards.clone());
        self.advance_epoch();

        rewards
    }

    /// Advance to next epoch (clears current contributions).
    fn advance_epoch(&mut self) {
        self.current_epoch.clear();
        self.epoch += 1;
    }

    /// Current epoch number.
    pub fn current_epoch(&self) -> u64 {
        self.epoch
    }

    /// Number of contributing agents in current epoch.
    pub fn active_miners(&self) -> usize {
        self.current_epoch.len()
    }

    /// Get rewards for a specific past epoch.
    pub fn epoch_rewards(&self, epoch: u64) -> Option<&Vec<MiningReward>> {
        self.history.get(epoch as usize)
    }

    /// Get total epochs completed.
    pub fn total_epochs(&self) -> u64 {
        self.history.len() as u64
    }

    /// Get category weight.
    pub fn weight(&self, category: MiningCategory) -> u32 {
        self.weights.get(&category).copied().unwrap_or(0)
    }

    /// Leaderboard for current epoch: agents sorted by total contribution score.
    pub fn leaderboard(&self) -> Vec<(&str, u64)> {
        let mut board: Vec<(&str, u64)> = self
            .current_epoch
            .iter()
            .map(|(id, c)| (id.as_str(), c.total_score()))
            .collect();
        board.sort_by(|a, b| b.1.cmp(&a.1));
        board
    }
}

impl Default for MiningPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Overflow-safe proportional calculation: (value * num) / den.
fn proportional_u128(value: u128, num: u128, den: u128) -> u128 {
    if den == 0 {
        return 0;
    }
    // Use GCD to reduce before multiplication
    let g1 = gcd(value, den);
    let g2 = gcd(num, den / g1);
    let reduced_value = value / g1;
    let reduced_num = num / g2;
    let reduced_den = den / g1 / g2;

    if reduced_den == 0 {
        return 0;
    }

    reduced_value
        .checked_mul(reduced_num)
        .map(|product| product / reduced_den)
        .unwrap_or_else(|| {
            // Fallback: scale down
            (value / den) * num + ((value % den) * num) / den
        })
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

/// Mining errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum MiningError {
    #[error("category weights must sum to 10000, got {0}")]
    InvalidWeights(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_weights_sum_to_10000() {
        let total: u32 = MiningCategory::all()
            .iter()
            .map(|c| c.default_weight_bp())
            .sum();
        assert_eq!(total, 10_000);
    }

    #[test]
    fn all_categories() {
        assert_eq!(MiningCategory::all().len(), 7);
    }

    #[test]
    fn record_contribution() {
        let mut pool = MiningPool::new();
        pool.record("agent-1", MiningCategory::ServiceFee, 100, "api calls");
        pool.record("agent-1", MiningCategory::Code, 50, "patch");

        let c = pool.contributions("agent-1").unwrap();
        assert_eq!(c.score(MiningCategory::ServiceFee), 100);
        assert_eq!(c.score(MiningCategory::Code), 50);
        assert_eq!(c.total_score(), 150);
    }

    #[test]
    fn record_accumulates() {
        let mut pool = MiningPool::new();
        pool.record("agent-1", MiningCategory::Relay, 10, "");
        pool.record("agent-1", MiningCategory::Relay, 20, "");

        assert_eq!(
            pool.contributions("agent-1").unwrap().score(MiningCategory::Relay),
            30
        );
    }

    #[test]
    fn distribute_single_agent() {
        let mut pool = MiningPool::new();
        pool.record("agent-1", MiningCategory::ServiceFee, 100, "");

        let rewards = pool.distribute(1_000_000);

        assert_eq!(rewards.len(), 1);
        assert_eq!(rewards[0].agent_id, "agent-1");
        // ServiceFee is 30% = 300,000
        assert_eq!(
            *rewards[0].per_category.get(&MiningCategory::ServiceFee).unwrap(),
            300_000
        );
        assert_eq!(rewards[0].total, 300_000);
    }

    #[test]
    fn distribute_multiple_agents() {
        let mut pool = MiningPool::new();
        pool.record("agent-1", MiningCategory::Code, 70, "");
        pool.record("agent-2", MiningCategory::Code, 30, "");

        let rewards = pool.distribute(10_000);

        // Code = 15% of 10000 = 1500
        // agent-1 gets 70/100 = 70% of 1500 = 1050
        // agent-2 gets 30/100 = 30% of 1500 = 450
        let r1 = rewards.iter().find(|r| r.agent_id == "agent-1").unwrap();
        let r2 = rewards.iter().find(|r| r.agent_id == "agent-2").unwrap();

        assert_eq!(*r1.per_category.get(&MiningCategory::Code).unwrap(), 1050);
        assert_eq!(*r2.per_category.get(&MiningCategory::Code).unwrap(), 450);
    }

    #[test]
    fn distribute_multi_category() {
        let mut pool = MiningPool::new();
        pool.record("agent-1", MiningCategory::ServiceFee, 100, "");
        pool.record("agent-1", MiningCategory::Security, 50, "");
        pool.record("agent-2", MiningCategory::Security, 50, "");

        let rewards = pool.distribute(100_000);

        let r1 = rewards.iter().find(|r| r.agent_id == "agent-1").unwrap();
        let r2 = rewards.iter().find(|r| r.agent_id == "agent-2").unwrap();

        // agent-1: ServiceFee 30% (solo) + Security 10% * 50/100 = 30000 + 5000
        assert_eq!(r1.total, 35_000);
        // agent-2: Security 10% * 50/100 = 5000
        assert_eq!(r2.total, 5_000);
    }

    #[test]
    fn distribute_zero_reward() {
        let mut pool = MiningPool::new();
        pool.record("agent-1", MiningCategory::Code, 100, "");
        let rewards = pool.distribute(0);
        assert!(rewards.is_empty());
    }

    #[test]
    fn distribute_no_contributions() {
        let mut pool = MiningPool::new();
        let rewards = pool.distribute(100_000);
        assert!(rewards.is_empty());
    }

    #[test]
    fn epoch_advances() {
        let mut pool = MiningPool::new();
        assert_eq!(pool.current_epoch(), 0);

        pool.record("agent-1", MiningCategory::Code, 100, "");
        pool.distribute(1000);

        assert_eq!(pool.current_epoch(), 1);
        assert_eq!(pool.active_miners(), 0); // cleared
        assert_eq!(pool.total_epochs(), 1);
    }

    #[test]
    fn history_tracking() {
        let mut pool = MiningPool::new();
        pool.record("agent-1", MiningCategory::Code, 100, "");
        pool.distribute(1000);

        let epoch_0 = pool.epoch_rewards(0).unwrap();
        assert_eq!(epoch_0.len(), 1);
    }

    #[test]
    fn total_distributed_accumulates() {
        let mut pool = MiningPool::new();

        pool.record("agent-1", MiningCategory::ServiceFee, 100, "");
        pool.distribute(10_000);

        pool.record("agent-1", MiningCategory::ServiceFee, 100, "");
        pool.distribute(10_000);

        assert_eq!(pool.total_distributed, 6000); // 3000 + 3000
    }

    #[test]
    fn leaderboard() {
        let mut pool = MiningPool::new();
        pool.record("agent-1", MiningCategory::Code, 100, "");
        pool.record("agent-2", MiningCategory::Code, 200, "");
        pool.record("agent-3", MiningCategory::Code, 50, "");

        let board = pool.leaderboard();
        assert_eq!(board[0].0, "agent-2");
        assert_eq!(board[0].1, 200);
        assert_eq!(board[1].0, "agent-1");
        assert_eq!(board[2].0, "agent-3");
    }

    #[test]
    fn custom_weights() {
        let mut weights = HashMap::new();
        weights.insert(MiningCategory::ServiceFee, 5000);
        weights.insert(MiningCategory::Code, 2000);
        weights.insert(MiningCategory::Staking, 1000);
        weights.insert(MiningCategory::Governance, 500);
        weights.insert(MiningCategory::PeerReview, 500);
        weights.insert(MiningCategory::Security, 500);
        weights.insert(MiningCategory::Relay, 500);

        let mut pool = MiningPool::with_weights(weights).unwrap();
        pool.record("agent-1", MiningCategory::ServiceFee, 100, "");

        let rewards = pool.distribute(100_000);
        // ServiceFee now 50% = 50000
        assert_eq!(rewards[0].total, 50_000);
    }

    #[test]
    fn custom_weights_invalid() {
        let mut weights = HashMap::new();
        weights.insert(MiningCategory::ServiceFee, 5000);
        weights.insert(MiningCategory::Code, 5001); // total = 10001

        let result = MiningPool::with_weights(weights);
        assert!(result.is_err());
    }

    #[test]
    fn weight_getter() {
        let pool = MiningPool::new();
        assert_eq!(pool.weight(MiningCategory::ServiceFee), 3000);
        assert_eq!(pool.weight(MiningCategory::Code), 1500);
        assert_eq!(pool.weight(MiningCategory::Relay), 1000);
    }

    #[test]
    fn proportional_basic() {
        assert_eq!(proportional_u128(1000, 30, 100), 300);
        assert_eq!(proportional_u128(10_000, 1500, 10_000), 1500);
    }

    #[test]
    fn proportional_zero_den() {
        assert_eq!(proportional_u128(1000, 30, 0), 0);
    }

    #[test]
    fn proportional_large_values() {
        // Simulates 333B tokens with 18 decimals
        let supply: u128 = 333_000_000_000 * 1_000_000_000_000_000_000;
        let result = proportional_u128(supply, 3000, 10_000); // 30%
        let expected: u128 = 99_900_000_000 * 1_000_000_000_000_000_000;
        assert_eq!(result, expected);
    }

    #[test]
    fn agent_contributions_default() {
        let c = AgentContributions::default();
        assert_eq!(c.total_score(), 0);
        assert_eq!(c.score(MiningCategory::Code), 0);
    }

    #[test]
    fn mining_reward_multi_category() {
        let mut pool = MiningPool::new();
        pool.record("agent-1", MiningCategory::Governance, 100, "voted");
        pool.record("agent-1", MiningCategory::PeerReview, 200, "reviewed");
        pool.record("agent-1", MiningCategory::Security, 50, "audit");

        let rewards = pool.distribute(100_000);
        let r = &rewards[0];

        assert!(r.per_category.contains_key(&MiningCategory::Governance));
        assert!(r.per_category.contains_key(&MiningCategory::PeerReview));
        assert!(r.per_category.contains_key(&MiningCategory::Security));

        // Governance: 10% of 100k = 10000 (solo agent, gets all)
        assert_eq!(*r.per_category.get(&MiningCategory::Governance).unwrap(), 10_000);
        // PeerReview: 10% = 10000
        assert_eq!(*r.per_category.get(&MiningCategory::PeerReview).unwrap(), 10_000);
        // Security: 10% = 10000
        assert_eq!(*r.per_category.get(&MiningCategory::Security).unwrap(), 10_000);

        assert_eq!(r.total, 30_000);
    }

    #[test]
    fn staking_proportional() {
        let mut pool = MiningPool::new();
        // Simulate staking amounts as contribution scores
        pool.record("agent-1", MiningCategory::Staking, 1000, "stake");
        pool.record("agent-2", MiningCategory::Staking, 3000, "stake");

        let rewards = pool.distribute(100_000);

        let r1 = rewards.iter().find(|r| r.agent_id == "agent-1").unwrap();
        let r2 = rewards.iter().find(|r| r.agent_id == "agent-2").unwrap();

        // Staking = 15% of 100k = 15000
        // agent-1: 1000/4000 = 25% of 15000 = 3750
        // agent-2: 3000/4000 = 75% of 15000 = 11250
        assert_eq!(*r1.per_category.get(&MiningCategory::Staking).unwrap(), 3750);
        assert_eq!(*r2.per_category.get(&MiningCategory::Staking).unwrap(), 11250);
    }

    #[test]
    fn active_miners() {
        let mut pool = MiningPool::new();
        assert_eq!(pool.active_miners(), 0);

        pool.record("agent-1", MiningCategory::Code, 10, "");
        pool.record("agent-2", MiningCategory::Relay, 20, "");
        assert_eq!(pool.active_miners(), 2);
    }

    #[test]
    fn epoch_rewards_out_of_range() {
        let pool = MiningPool::new();
        assert!(pool.epoch_rewards(99).is_none());
    }

    #[test]
    fn gcd_basic() {
        assert_eq!(gcd(12, 8), 4);
        assert_eq!(gcd(100, 10), 10);
        assert_eq!(gcd(7, 3), 1);
        assert_eq!(gcd(0, 5), 5);
    }
}
