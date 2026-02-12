//! Staking — lock tokens, earn DeFi revenue proportional to stake share.
//!
//! Stake types:
//! - Founder: 1.5% genesis allocation, staked, DeFi revenue withdrawable, unlockable
//! - Cabinet: 2.5% indefinite lock, DeFi revenue for salaries/operations
//! - Mining:  60% pool staked for DeFi + governance-directed use
//! - General: Any holder can stake for DeFi revenue

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::token::{TokenAmount, TokenError};
use crate::wallet::Address;

/// Classification of stake lockup behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StakeType {
    /// Genesis founder — staked, DeFi revenue withdrawable to wallet.
    Founder,
    /// Cabinet — indefinite lockup, DeFi revenue for operations.
    Cabinet,
    /// Mining pool — governance-directed staking.
    Mining,
    /// General staking by any holder.
    General,
}

impl StakeType {
    /// Whether this stake type allows principal withdrawal.
    pub fn is_unlockable(&self) -> bool {
        match self {
            Self::Founder => true,   // unlockable — founder's last resort for emergencies
            Self::Cabinet => false,  // indefinite lockup
            Self::Mining => false,   // governance controlled
            Self::General => true,   // can unlock after lock_until
        }
    }
}

/// A single staking entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeEntry {
    pub staker: Address,
    pub amount: TokenAmount,
    pub stake_type: StakeType,
    /// Epoch when lock expires (0 = no expiry / indefinite).
    pub lock_until: u64,
    /// Epoch when the stake was created.
    pub created_at: u64,
    /// Accumulated unclaimed revenue for this stake.
    pub unclaimed_revenue: TokenAmount,
    /// Whether this stake has been slashed.
    pub slashed: bool,
}

/// The staking pool — manages all stakes and distributes DeFi revenue.
#[derive(Debug)]
pub struct StakePool {
    stakes: HashMap<Address, Vec<StakeEntry>>,
    /// Total staked across all entries.
    total_staked: TokenAmount,
    /// Current epoch.
    current_epoch: u64,
    /// Total revenue distributed so far.
    total_revenue_distributed: TokenAmount,
    /// Slash rate in basis points (e.g., 1000 = 10%).
    pub slash_rate_bp: u32,
}

impl StakePool {
    pub fn new() -> Self {
        Self {
            stakes: HashMap::new(),
            total_staked: TokenAmount::ZERO,
            current_epoch: 0,
            total_revenue_distributed: TokenAmount::ZERO,
            slash_rate_bp: 1000, // 10% default slash
        }
    }

    pub fn advance_epoch(&mut self) {
        self.current_epoch += 1;
    }

    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    pub fn total_staked(&self) -> TokenAmount {
        self.total_staked
    }

    pub fn total_revenue_distributed(&self) -> TokenAmount {
        self.total_revenue_distributed
    }

    /// Stake tokens.
    pub fn stake(
        &mut self,
        staker: &Address,
        amount: TokenAmount,
        stake_type: StakeType,
        lock_epochs: u64,
    ) -> Result<(), TokenError> {
        if amount.is_zero() {
            return Err(TokenError::InvalidAmount("zero stake".into()));
        }

        let lock_until = if lock_epochs == 0 {
            0 // indefinite
        } else {
            self.current_epoch + lock_epochs
        };

        let entry = StakeEntry {
            staker: staker.clone(),
            amount,
            stake_type,
            lock_until,
            created_at: self.current_epoch,
            unclaimed_revenue: TokenAmount::ZERO,
            slashed: false,
        };

        self.stakes
            .entry(staker.clone())
            .or_default()
            .push(entry);
        self.total_staked = self.total_staked.checked_add(amount)?;

        Ok(())
    }

    /// Unstake (unlock) a general stake after lock period.
    /// Returns the amount unlocked.
    pub fn unstake(
        &mut self,
        staker: &Address,
        index: usize,
    ) -> Result<TokenAmount, TokenError> {
        let entries = self
            .stakes
            .get_mut(staker)
            .ok_or_else(|| TokenError::NotFound(format!("stakes for {}", staker)))?;

        if index >= entries.len() {
            return Err(TokenError::NotFound(format!("stake index {}", index)));
        }

        let entry = &entries[index];

        if !entry.stake_type.is_unlockable() {
            return Err(TokenError::Locked(format!(
                "{:?} stakes cannot be unlocked",
                entry.stake_type
            )));
        }

        if entry.lock_until > 0 && self.current_epoch < entry.lock_until {
            return Err(TokenError::Locked(format!(
                "locked until epoch {}, current={}",
                entry.lock_until, self.current_epoch
            )));
        }

        if entry.slashed {
            return Err(TokenError::Locked("stake has been slashed".into()));
        }

        let amount = entry.amount;
        self.total_staked = self.total_staked.checked_sub(amount)?;
        entries.remove(index);

        if entries.is_empty() {
            self.stakes.remove(staker);
        }

        Ok(amount)
    }

    /// Distribute DeFi revenue proportionally to all stakers.
    /// Returns a map of address → revenue earned this epoch.
    pub fn distribute_revenue(
        &mut self,
        total_revenue: TokenAmount,
    ) -> Result<HashMap<Address, TokenAmount>, TokenError> {
        if total_revenue.is_zero() || self.total_staked.is_zero() {
            return Ok(HashMap::new());
        }

        let mut distribution: HashMap<Address, TokenAmount> = HashMap::new();

        for entries in self.stakes.values_mut() {
            for entry in entries.iter_mut() {
                if entry.slashed {
                    continue;
                }
                // Revenue share = (entry.amount / total_staked) * total_revenue
                let share = total_revenue.proportional(
                    entry.amount.base_units(),
                    self.total_staked.base_units(),
                )?;

                entry.unclaimed_revenue = entry.unclaimed_revenue.checked_add(share)?;

                *distribution.entry(entry.staker.clone()).or_insert(TokenAmount::ZERO) =
                    distribution
                        .get(&entry.staker)
                        .unwrap_or(&TokenAmount::ZERO)
                        .checked_add(share)?;
            }
        }

        self.total_revenue_distributed =
            self.total_revenue_distributed.checked_add(total_revenue)?;

        Ok(distribution)
    }

    /// Claim accumulated DeFi revenue for a staker.
    /// Returns total revenue claimed across all their stakes.
    pub fn claim_revenue(&mut self, staker: &Address) -> Result<TokenAmount, TokenError> {
        let entries = self
            .stakes
            .get_mut(staker)
            .ok_or_else(|| TokenError::NotFound(format!("stakes for {}", staker)))?;

        let mut total_claimed = TokenAmount::ZERO;
        for entry in entries.iter_mut() {
            total_claimed = total_claimed.checked_add(entry.unclaimed_revenue)?;
            entry.unclaimed_revenue = TokenAmount::ZERO;
        }

        Ok(total_claimed)
    }

    /// Get unclaimed revenue for a staker.
    pub fn unclaimed_revenue(&self, staker: &Address) -> TokenAmount {
        self.stakes
            .get(staker)
            .map(|entries| {
                entries
                    .iter()
                    .fold(TokenAmount::ZERO, |acc, e| TokenAmount(acc.0 + e.unclaimed_revenue.0))
            })
            .unwrap_or(TokenAmount::ZERO)
    }

    /// Slash a staker's stake (misbehavior penalty).
    /// Slashed amount goes to treasury.
    pub fn slash(&mut self, staker: &Address) -> Result<TokenAmount, TokenError> {
        let entries = self
            .stakes
            .get_mut(staker)
            .ok_or_else(|| TokenError::NotFound(format!("stakes for {}", staker)))?;

        let mut total_slashed = TokenAmount::ZERO;
        for entry in entries.iter_mut() {
            if entry.slashed {
                continue;
            }
            let slash_amount = TokenAmount::from_base(
                entry.amount.base_units() * self.slash_rate_bp as u128 / 10_000,
            );
            entry.amount = entry.amount.checked_sub(slash_amount)?;
            self.total_staked = self.total_staked.checked_sub(slash_amount)?;
            entry.slashed = true;
            total_slashed = total_slashed.checked_add(slash_amount)?;
        }

        Ok(total_slashed)
    }

    /// Total staked by a specific address.
    pub fn staked_by(&self, staker: &Address) -> TokenAmount {
        self.stakes
            .get(staker)
            .map(|entries| {
                entries
                    .iter()
                    .fold(TokenAmount::ZERO, |acc, e| TokenAmount(acc.0 + e.amount.0))
            })
            .unwrap_or(TokenAmount::ZERO)
    }

    /// Number of active stakers.
    pub fn staker_count(&self) -> usize {
        self.stakes.len()
    }

    /// All stake entries for an address.
    pub fn stakes_for(&self, staker: &Address) -> &[StakeEntry] {
        self.stakes
            .get(staker)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

impl Default for StakePool {
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

    #[test]
    fn stake_and_total() {
        let mut pool = StakePool::new();
        pool.stake(&addr("aa"), TokenAmount::from_tokens(1000), StakeType::General, 10)
            .unwrap();
        assert_eq!(pool.total_staked().whole_tokens(), 1000);
        assert_eq!(pool.staker_count(), 1);
    }

    #[test]
    fn stake_zero_fails() {
        let mut pool = StakePool::new();
        assert!(pool
            .stake(&addr("aa"), TokenAmount::ZERO, StakeType::General, 10)
            .is_err());
    }

    #[test]
    fn unstake_after_lock() {
        let mut pool = StakePool::new();
        pool.stake(&addr("aa"), TokenAmount::from_tokens(500), StakeType::General, 5)
            .unwrap();

        // Before lock expires
        assert!(pool.unstake(&addr("aa"), 0).is_err());

        // Advance past lock
        for _ in 0..6 {
            pool.advance_epoch();
        }

        let returned = pool.unstake(&addr("aa"), 0).unwrap();
        assert_eq!(returned.whole_tokens(), 500);
        assert_eq!(pool.total_staked(), TokenAmount::ZERO);
    }

    #[test]
    fn founder_stake_unlockable() {
        let mut pool = StakePool::new();
        pool.stake(&addr("founder"), TokenAmount::from_tokens(1000), StakeType::Founder, 0)
            .unwrap();

        // Founder can unstake anytime (last resort for emergencies)
        let returned = pool.unstake(&addr("founder"), 0).unwrap();
        assert_eq!(returned.whole_tokens(), 1000);
        assert_eq!(pool.total_staked(), TokenAmount::ZERO);
    }

    #[test]
    fn cabinet_stake_cannot_unstake() {
        let mut pool = StakePool::new();
        pool.stake(&addr("cabinet"), TokenAmount::from_tokens(1000), StakeType::Cabinet, 0)
            .unwrap();

        assert!(pool.unstake(&addr("cabinet"), 0).is_err());
    }

    #[test]
    fn revenue_distribution_proportional() {
        let mut pool = StakePool::new();
        let alice = addr("aa");
        let bob = addr("bb");

        pool.stake(&alice, TokenAmount::from_tokens(700), StakeType::General, 10)
            .unwrap();
        pool.stake(&bob, TokenAmount::from_tokens(300), StakeType::General, 10)
            .unwrap();

        let dist = pool
            .distribute_revenue(TokenAmount::from_tokens(100))
            .unwrap();

        // Alice: 700/1000 * 100 = 70, Bob: 300/1000 * 100 = 30
        assert_eq!(dist[&alice].whole_tokens(), 70);
        assert_eq!(dist[&bob].whole_tokens(), 30);
    }

    #[test]
    fn founder_defi_revenue_withdrawable() {
        let mut pool = StakePool::new();
        let founder = addr("founder");

        pool.stake(&founder, TokenAmount::from_tokens(1000), StakeType::Founder, 0)
            .unwrap();

        pool.distribute_revenue(TokenAmount::from_tokens(50)).unwrap();

        // Founder can claim revenue even though stake is locked
        let revenue = pool.claim_revenue(&founder).unwrap();
        assert_eq!(revenue.whole_tokens(), 50);

        // After claiming, unclaimed is zero
        assert_eq!(pool.unclaimed_revenue(&founder), TokenAmount::ZERO);
    }

    #[test]
    fn cabinet_defi_revenue_for_salaries() {
        let mut pool = StakePool::new();
        let cabinet = addr("cabinet");

        pool.stake(&cabinet, TokenAmount::from_tokens(2000), StakeType::Cabinet, 0)
            .unwrap();

        // Distribute revenue over 3 epochs
        for _ in 0..3 {
            pool.distribute_revenue(TokenAmount::from_tokens(100)).unwrap();
            pool.advance_epoch();
        }

        let total = pool.unclaimed_revenue(&cabinet);
        assert_eq!(total.whole_tokens(), 300);

        // Claim for salary distribution
        let claimed = pool.claim_revenue(&cabinet).unwrap();
        assert_eq!(claimed.whole_tokens(), 300);
    }

    #[test]
    fn slash_reduces_stake() {
        let mut pool = StakePool::new();
        let bad_actor = addr("bad");

        pool.stake(&bad_actor, TokenAmount::from_tokens(1000), StakeType::General, 10)
            .unwrap();

        let slashed = pool.slash(&bad_actor).unwrap();
        // Default 10% slash
        assert_eq!(slashed.whole_tokens(), 100);
        assert_eq!(pool.staked_by(&bad_actor).whole_tokens(), 900);
    }

    #[test]
    fn slashed_stake_excluded_from_revenue() {
        let mut pool = StakePool::new();
        let alice = addr("aa");
        let bad = addr("bad");

        pool.stake(&alice, TokenAmount::from_tokens(500), StakeType::General, 10)
            .unwrap();
        pool.stake(&bad, TokenAmount::from_tokens(500), StakeType::General, 10)
            .unwrap();

        pool.slash(&bad).unwrap();

        let dist = pool
            .distribute_revenue(TokenAmount::from_tokens(100))
            .unwrap();

        // Bad actor is slashed, gets no revenue
        assert_eq!(dist.get(&bad), None);
        // Alice gets proportional to her stake vs remaining total
        assert!(dist[&alice].whole_tokens() > 0);
    }

    #[test]
    fn multiple_stakes_same_address() {
        let mut pool = StakePool::new();
        let alice = addr("aa");

        pool.stake(&alice, TokenAmount::from_tokens(100), StakeType::General, 5)
            .unwrap();
        pool.stake(&alice, TokenAmount::from_tokens(200), StakeType::General, 10)
            .unwrap();

        assert_eq!(pool.staked_by(&alice).whole_tokens(), 300);
        assert_eq!(pool.stakes_for(&alice).len(), 2);
    }

    #[test]
    fn revenue_distribution_empty_pool() {
        let mut pool = StakePool::new();
        let dist = pool
            .distribute_revenue(TokenAmount::from_tokens(100))
            .unwrap();
        assert!(dist.is_empty());
    }

    #[test]
    fn total_revenue_distributed_tracks() {
        let mut pool = StakePool::new();
        pool.stake(&addr("aa"), TokenAmount::from_tokens(100), StakeType::General, 10)
            .unwrap();

        pool.distribute_revenue(TokenAmount::from_tokens(10)).unwrap();
        pool.distribute_revenue(TokenAmount::from_tokens(20)).unwrap();

        assert_eq!(pool.total_revenue_distributed().whole_tokens(), 30);
    }
}
