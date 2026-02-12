//! HelmTreasury — revenue collection, allocation, capital pool, and audit ledger.
//!
//! Revenue sources:
//! - Edge API fees (15% of gross)
//! - Transaction fees
//! - Slashing penalties
//!
//! Revenue allocation:
//! - Staking rewards pool
//! - Development fund
//! - Capital pool (for cabinet project funding via voting)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::token::{TokenAmount, TokenError};
use crate::wallet::Address;

/// Treasury allocation buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TreasuryBucket {
    /// Distributed to stakers as DeFi yield.
    StakingRewards,
    /// Protocol development and operations.
    Development,
    /// Capital pool for cabinet-approved project funding.
    CapitalPool,
    /// Token burn (deflationary pressure).
    Burn,
}

impl TreasuryBucket {
    /// Default allocation in basis points.
    pub fn default_basis_points(&self) -> u32 {
        match self {
            Self::StakingRewards => 5000, // 50%
            Self::Development => 2000,    // 20%
            Self::CapitalPool => 2500,    // 25%
            Self::Burn => 500,            //  5%
        }
    }

    pub fn all() -> &'static [TreasuryBucket] {
        &[
            Self::StakingRewards,
            Self::Development,
            Self::CapitalPool,
            Self::Burn,
        ]
    }
}

/// A ledger entry recording a treasury operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub epoch: u64,
    pub operation: LedgerOperation,
    pub amount: TokenAmount,
    pub bucket: Option<TreasuryBucket>,
    pub memo: String,
}

/// Types of treasury operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LedgerOperation {
    /// Revenue collected from a source.
    Collect,
    /// Allocated to a bucket.
    Allocate,
    /// Disbursed from a bucket.
    Disburse,
    /// Cabinet-approved project funding.
    ProjectFund,
    /// Burned tokens.
    Burn,
}

/// The Helm Protocol treasury.
#[derive(Debug)]
pub struct HelmTreasury {
    /// Unallocated revenue balance.
    unallocated: TokenAmount,
    /// Bucket balances.
    buckets: HashMap<TreasuryBucket, TokenAmount>,
    /// Allocation ratios in basis points (customizable via governance).
    allocation_bp: HashMap<TreasuryBucket, u32>,
    /// Transparent audit ledger.
    ledger: Vec<LedgerEntry>,
    /// Current epoch.
    current_epoch: u64,
    /// Total revenue ever collected.
    total_collected: TokenAmount,
    /// Total burned.
    total_burned: TokenAmount,
    /// Edge API revenue rate (basis points of gross revenue → treasury).
    pub edge_api_rate_bp: u32,
}

impl HelmTreasury {
    pub fn new() -> Self {
        let mut allocation_bp = HashMap::new();
        for bucket in TreasuryBucket::all() {
            allocation_bp.insert(*bucket, bucket.default_basis_points());
        }

        Self {
            unallocated: TokenAmount::ZERO,
            buckets: HashMap::new(),
            allocation_bp,
            ledger: Vec::new(),
            current_epoch: 0,
            total_collected: TokenAmount::ZERO,
            total_burned: TokenAmount::ZERO,
            edge_api_rate_bp: 1500, // 15%
        }
    }

    pub fn advance_epoch(&mut self) {
        self.current_epoch += 1;
    }

    /// Collect revenue into the treasury.
    pub fn collect_revenue(
        &mut self,
        amount: TokenAmount,
        memo: &str,
    ) -> Result<(), TokenError> {
        if amount.is_zero() {
            return Ok(());
        }

        self.unallocated = self.unallocated.checked_add(amount)?;
        self.total_collected = self.total_collected.checked_add(amount)?;

        self.ledger.push(LedgerEntry {
            epoch: self.current_epoch,
            operation: LedgerOperation::Collect,
            amount,
            bucket: None,
            memo: memo.to_string(),
        });

        Ok(())
    }

    /// Collect Edge API revenue (takes 15% of gross).
    pub fn collect_edge_api_revenue(
        &mut self,
        gross_revenue: TokenAmount,
    ) -> Result<TokenAmount, TokenError> {
        let treasury_share = TokenAmount::from_base(
            gross_revenue.base_units() * self.edge_api_rate_bp as u128 / 10_000,
        );
        self.collect_revenue(treasury_share, "Edge API revenue")?;
        Ok(treasury_share)
    }

    /// Allocate unallocated revenue to buckets based on configured ratios.
    pub fn allocate(&mut self) -> Result<HashMap<TreasuryBucket, TokenAmount>, TokenError> {
        if self.unallocated.is_zero() {
            return Ok(HashMap::new());
        }

        let mut allocated = HashMap::new();
        let total = self.unallocated;

        for bucket in TreasuryBucket::all() {
            let bp = *self.allocation_bp.get(bucket).unwrap_or(&0);
            let share = total.proportional(bp as u128, 10_000)?;

            if !share.is_zero() {
                let entry = self.buckets.entry(*bucket).or_insert(TokenAmount::ZERO);
                *entry = entry.checked_add(share)?;
                allocated.insert(*bucket, share);

                self.ledger.push(LedgerEntry {
                    epoch: self.current_epoch,
                    operation: LedgerOperation::Allocate,
                    amount: share,
                    bucket: Some(*bucket),
                    memo: format!("allocation {}bp", bp),
                });
            }
        }

        self.unallocated = TokenAmount::ZERO;
        Ok(allocated)
    }

    /// Disburse funds from a bucket.
    pub fn disburse(
        &mut self,
        bucket: TreasuryBucket,
        amount: TokenAmount,
        _recipient: &Address,
        memo: &str,
    ) -> Result<(), TokenError> {
        let balance = self.buckets.entry(bucket).or_insert(TokenAmount::ZERO);
        if balance.base_units() < amount.base_units() {
            return Err(TokenError::InsufficientBalance {
                have: balance.base_units(),
                need: amount.base_units(),
            });
        }

        *balance = balance.checked_sub(amount)?;

        if bucket == TreasuryBucket::Burn {
            self.total_burned = self.total_burned.checked_add(amount)?;
        }

        self.ledger.push(LedgerEntry {
            epoch: self.current_epoch,
            operation: if bucket == TreasuryBucket::Burn {
                LedgerOperation::Burn
            } else {
                LedgerOperation::Disburse
            },
            amount,
            bucket: Some(bucket),
            memo: memo.to_string(),
        });

        Ok(())
    }

    /// Fund a cabinet-approved project from the capital pool.
    pub fn fund_project(
        &mut self,
        amount: TokenAmount,
        project_name: &str,
    ) -> Result<(), TokenError> {
        let balance = self
            .buckets
            .entry(TreasuryBucket::CapitalPool)
            .or_insert(TokenAmount::ZERO);

        if balance.base_units() < amount.base_units() {
            return Err(TokenError::InsufficientBalance {
                have: balance.base_units(),
                need: amount.base_units(),
            });
        }

        *balance = balance.checked_sub(amount)?;

        self.ledger.push(LedgerEntry {
            epoch: self.current_epoch,
            operation: LedgerOperation::ProjectFund,
            amount,
            bucket: Some(TreasuryBucket::CapitalPool),
            memo: format!("project: {}", project_name),
        });

        Ok(())
    }

    /// Get a bucket's current balance.
    pub fn bucket_balance(&self, bucket: TreasuryBucket) -> TokenAmount {
        *self
            .buckets
            .get(&bucket)
            .unwrap_or(&TokenAmount::ZERO)
    }

    /// Get the staking rewards pool balance (available for distribution).
    pub fn staking_rewards_available(&self) -> TokenAmount {
        self.bucket_balance(TreasuryBucket::StakingRewards)
    }

    /// Get the capital pool balance (available for project funding via voting).
    pub fn capital_pool_available(&self) -> TokenAmount {
        self.bucket_balance(TreasuryBucket::CapitalPool)
    }

    pub fn unallocated(&self) -> TokenAmount {
        self.unallocated
    }

    pub fn total_collected(&self) -> TokenAmount {
        self.total_collected
    }

    pub fn total_burned(&self) -> TokenAmount {
        self.total_burned
    }

    /// Full audit ledger.
    pub fn ledger(&self) -> &[LedgerEntry] {
        &self.ledger
    }

    /// Update allocation ratios (governance action).
    pub fn set_allocation_bp(
        &mut self,
        bucket: TreasuryBucket,
        bp: u32,
    ) -> Result<(), TokenError> {
        let total: u32 = self
            .allocation_bp
            .iter()
            .map(|(k, v)| if *k == bucket { bp } else { *v })
            .sum();

        if total != 10_000 {
            return Err(TokenError::InvalidAmount(format!(
                "allocation ratios must sum to 10000bp, got {}",
                total
            )));
        }

        self.allocation_bp.insert(bucket, bp);
        Ok(())
    }
}

impl Default for HelmTreasury {
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
    fn treasury_collect_revenue() {
        let mut treasury = HelmTreasury::new();
        treasury
            .collect_revenue(TokenAmount::from_tokens(1000), "test")
            .unwrap();
        assert_eq!(treasury.unallocated().whole_tokens(), 1000);
        assert_eq!(treasury.total_collected().whole_tokens(), 1000);
    }

    #[test]
    fn edge_api_revenue_15_percent() {
        let mut treasury = HelmTreasury::new();
        let gross = TokenAmount::from_tokens(10_000);
        let collected = treasury.collect_edge_api_revenue(gross).unwrap();
        // 15% of 10,000 = 1,500
        assert_eq!(collected.whole_tokens(), 1500);
        assert_eq!(treasury.unallocated().whole_tokens(), 1500);
    }

    #[test]
    fn allocate_distributes_to_buckets() {
        let mut treasury = HelmTreasury::new();
        treasury
            .collect_revenue(TokenAmount::from_tokens(10_000), "test")
            .unwrap();

        let alloc = treasury.allocate().unwrap();

        // StakingRewards: 50% = 5000
        assert_eq!(
            alloc[&TreasuryBucket::StakingRewards].whole_tokens(),
            5000
        );
        // Development: 20% = 2000
        assert_eq!(
            alloc[&TreasuryBucket::Development].whole_tokens(),
            2000
        );
        // CapitalPool: 25% = 2500
        assert_eq!(
            alloc[&TreasuryBucket::CapitalPool].whole_tokens(),
            2500
        );
        // Burn: 5% = 500
        assert_eq!(alloc[&TreasuryBucket::Burn].whole_tokens(), 500);

        assert_eq!(treasury.unallocated(), TokenAmount::ZERO);
    }

    #[test]
    fn disburse_from_bucket() {
        let mut treasury = HelmTreasury::new();
        treasury
            .collect_revenue(TokenAmount::from_tokens(1000), "test")
            .unwrap();
        treasury.allocate().unwrap();

        // Development has 200 tokens (20%)
        let dev_balance = treasury.bucket_balance(TreasuryBucket::Development);
        assert_eq!(dev_balance.whole_tokens(), 200);

        treasury
            .disburse(
                TreasuryBucket::Development,
                TokenAmount::from_tokens(100),
                &addr("dev"),
                "dev payment",
            )
            .unwrap();

        assert_eq!(
            treasury
                .bucket_balance(TreasuryBucket::Development)
                .whole_tokens(),
            100
        );
    }

    #[test]
    fn disburse_exceeds_balance_fails() {
        let mut treasury = HelmTreasury::new();
        treasury
            .collect_revenue(TokenAmount::from_tokens(100), "test")
            .unwrap();
        treasury.allocate().unwrap();

        assert!(treasury
            .disburse(
                TreasuryBucket::Development,
                TokenAmount::from_tokens(999),
                &addr("dev"),
                "too much",
            )
            .is_err());
    }

    #[test]
    fn fund_project_from_capital_pool() {
        let mut treasury = HelmTreasury::new();
        treasury
            .collect_revenue(TokenAmount::from_tokens(10_000), "revenue")
            .unwrap();
        treasury.allocate().unwrap();

        // Capital pool: 25% = 2500
        assert_eq!(treasury.capital_pool_available().whole_tokens(), 2500);

        treasury
            .fund_project(TokenAmount::from_tokens(1000), "Security Audit v2")
            .unwrap();

        assert_eq!(treasury.capital_pool_available().whole_tokens(), 1500);
    }

    #[test]
    fn burn_tracks_total() {
        let mut treasury = HelmTreasury::new();
        treasury
            .collect_revenue(TokenAmount::from_tokens(10_000), "test")
            .unwrap();
        treasury.allocate().unwrap();

        // Burn bucket: 5% = 500
        treasury
            .disburse(
                TreasuryBucket::Burn,
                TokenAmount::from_tokens(500),
                &Address::genesis(),
                "quarterly burn",
            )
            .unwrap();

        assert_eq!(treasury.total_burned().whole_tokens(), 500);
    }

    #[test]
    fn ledger_audit_trail() {
        let mut treasury = HelmTreasury::new();
        treasury
            .collect_revenue(TokenAmount::from_tokens(1000), "test")
            .unwrap();
        treasury.allocate().unwrap();

        // 1 collect + 4 allocations
        assert_eq!(treasury.ledger().len(), 5);
        assert!(matches!(
            treasury.ledger()[0].operation,
            LedgerOperation::Collect
        ));
    }

    #[test]
    fn allocation_ratios_must_sum_to_100() {
        let mut treasury = HelmTreasury::new();
        // Try to set Development to 5000bp (50%) — total would be 12500
        assert!(treasury
            .set_allocation_bp(TreasuryBucket::Development, 5000)
            .is_err());
    }

    #[test]
    fn staking_rewards_available() {
        let mut treasury = HelmTreasury::new();
        treasury
            .collect_revenue(TokenAmount::from_tokens(2000), "test")
            .unwrap();
        treasury.allocate().unwrap();

        assert_eq!(treasury.staking_rewards_available().whole_tokens(), 1000);
    }

    #[test]
    fn bucket_default_ratios() {
        assert_eq!(TreasuryBucket::StakingRewards.default_basis_points(), 5000);
        assert_eq!(TreasuryBucket::Development.default_basis_points(), 2000);
        assert_eq!(TreasuryBucket::CapitalPool.default_basis_points(), 2500);
        assert_eq!(TreasuryBucket::Burn.default_basis_points(), 500);
    }
}
