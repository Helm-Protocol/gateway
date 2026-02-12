//! Dynamic pricing — supply/demand based API pricing with rate limiting and tiers.
//!
//! Pricing model:
//! - Base price adjusts based on network utilization
//! - Surge pricing when demand exceeds capacity
//! - Discount tiers for high-volume / long-term stakers
//! - Rate limiting per agent to prevent abuse

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::token::{TokenAmount, TokenError, ONE_TOKEN};
use crate::wallet::Address;

/// Discount tier for frequent users / stakers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DiscountTier {
    /// No discount (< 100 calls/epoch or no stake).
    None,
    /// Bronze: 5% discount (100+ calls or 10K+ staked).
    Bronze,
    /// Silver: 10% discount (1000+ calls or 100K+ staked).
    Silver,
    /// Gold: 20% discount (10K+ calls or 1M+ staked).
    Gold,
}

impl DiscountTier {
    /// Discount in basis points.
    pub fn discount_bp(&self) -> u32 {
        match self {
            Self::None => 0,
            Self::Bronze => 500,  //  5%
            Self::Silver => 1000, // 10%
            Self::Gold => 2000,   // 20%
        }
    }

    /// Determine tier from call count.
    pub fn from_calls(calls: u64) -> Self {
        if calls >= 10_000 {
            Self::Gold
        } else if calls >= 1_000 {
            Self::Silver
        } else if calls >= 100 {
            Self::Bronze
        } else {
            Self::None
        }
    }

    /// Determine tier from staked amount.
    pub fn from_stake(staked: TokenAmount) -> Self {
        let tokens = staked.whole_tokens();
        if tokens >= 1_000_000 {
            Self::Gold
        } else if tokens >= 100_000 {
            Self::Silver
        } else if tokens >= 10_000 {
            Self::Bronze
        } else {
            Self::None
        }
    }

    /// Best tier between call-based and stake-based.
    pub fn best(calls: u64, staked: TokenAmount) -> Self {
        std::cmp::max(Self::from_calls(calls), Self::from_stake(staked))
    }
}

/// Rate limiter state for an address.
#[derive(Debug, Clone)]
struct RateLimitState {
    calls_this_epoch: u64,
    max_calls_per_epoch: u64,
    total_calls: u64,
}

/// Dynamic pricing engine.
#[derive(Debug)]
pub struct DynamicPricing {
    /// Base price per API call in base token units.
    base_price: u128,
    /// Current utilization (0..10000 basis points, 10000 = 100% capacity).
    utilization_bp: u32,
    /// Surge multiplier threshold (utilization_bp above which surge applies).
    surge_threshold_bp: u32,
    /// Maximum surge multiplier in basis points (e.g., 30000 = 3x).
    max_surge_bp: u32,
    /// Rate limits per address.
    rate_limits: HashMap<Address, RateLimitState>,
    /// Default max calls per epoch.
    pub default_rate_limit: u64,
    /// Total revenue generated.
    total_revenue: TokenAmount,
    /// Current epoch.
    current_epoch: u64,
}

impl DynamicPricing {
    pub fn new(base_price_tokens: u128) -> Self {
        Self {
            base_price: base_price_tokens * ONE_TOKEN,
            utilization_bp: 0,
            surge_threshold_bp: 7000, // 70%
            max_surge_bp: 30_000,     // 3x max
            rate_limits: HashMap::new(),
            default_rate_limit: 10_000,
            total_revenue: TokenAmount::ZERO,
            current_epoch: 0,
        }
    }

    /// Advance epoch — resets per-epoch rate limits.
    pub fn advance_epoch(&mut self) {
        self.current_epoch += 1;
        for state in self.rate_limits.values_mut() {
            state.calls_this_epoch = 0;
        }
    }

    /// Set current network utilization.
    pub fn set_utilization(&mut self, bp: u32) {
        self.utilization_bp = bp.min(10_000);
    }

    /// Calculate the current effective price per API call.
    pub fn effective_price(&self, caller: &Address, staked: TokenAmount) -> TokenAmount {
        let mut price = self.base_price;

        // Apply surge if above threshold
        if self.utilization_bp > self.surge_threshold_bp {
            let over = (self.utilization_bp - self.surge_threshold_bp) as u128;
            let max_over = (10_000 - self.surge_threshold_bp) as u128;
            // Linear surge: 1x at threshold, max_surge at 100%
            let surge_bp = 10_000u128 + (self.max_surge_bp as u128 - 10_000) * over / max_over;
            price = price * surge_bp / 10_000;
        }

        // Apply discount tier
        let calls = self
            .rate_limits
            .get(caller)
            .map(|s| s.total_calls)
            .unwrap_or(0);
        let tier = DiscountTier::best(calls, staked);
        let discount = tier.discount_bp();
        price = price * (10_000 - discount as u128) / 10_000;

        TokenAmount::from_base(price)
    }

    /// Process an API call: check rate limit, calculate price.
    /// Returns the price charged.
    pub fn process_call(
        &mut self,
        caller: &Address,
        staked: TokenAmount,
    ) -> Result<TokenAmount, TokenError> {
        // Ensure rate limit entry exists
        let default_limit = self.default_rate_limit;
        let state = self.rate_limits.entry(caller.clone()).or_insert(RateLimitState {
            calls_this_epoch: 0,
            max_calls_per_epoch: default_limit,
            total_calls: 0,
        });

        if state.calls_this_epoch >= state.max_calls_per_epoch {
            return Err(TokenError::Locked(format!(
                "rate limit exceeded: {}/{} calls this epoch",
                state.calls_this_epoch, state.max_calls_per_epoch
            )));
        }

        // Increment counters before computing price (avoids borrow conflict)
        state.calls_this_epoch += 1;
        state.total_calls += 1;

        let price = self.effective_price(caller, staked);
        self.total_revenue = self.total_revenue.checked_add(price)?;

        Ok(price)
    }

    /// Set a custom rate limit for an address.
    pub fn set_rate_limit(&mut self, address: &Address, max_per_epoch: u64) {
        let state = self.rate_limits.entry(address.clone()).or_insert(RateLimitState {
            calls_this_epoch: 0,
            max_calls_per_epoch: max_per_epoch,
            total_calls: 0,
        });
        state.max_calls_per_epoch = max_per_epoch;
    }

    /// Get calls made by an address this epoch.
    pub fn calls_this_epoch(&self, address: &Address) -> u64 {
        self.rate_limits
            .get(address)
            .map(|s| s.calls_this_epoch)
            .unwrap_or(0)
    }

    /// Get total calls ever made by an address.
    pub fn total_calls(&self, address: &Address) -> u64 {
        self.rate_limits
            .get(address)
            .map(|s| s.total_calls)
            .unwrap_or(0)
    }

    pub fn total_revenue(&self) -> TokenAmount {
        self.total_revenue
    }

    pub fn base_price(&self) -> TokenAmount {
        TokenAmount::from_base(self.base_price)
    }

    pub fn utilization_bp(&self) -> u32 {
        self.utilization_bp
    }
}

impl Default for DynamicPricing {
    fn default() -> Self {
        Self::new(1) // 1 HELM per call default
    }
}

/// Contribution tier — determines withdrawal fee discount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ContributionTier {
    /// New user, minimal contribution: 15% withdrawal fee.
    Newcomer,
    /// Active participant: 10% withdrawal fee.
    Active,
    /// Significant contributor: 5% withdrawal fee.
    Contributor,
    /// Core contributor: 2% withdrawal fee.
    Core,
    /// Veteran/essential: 1% withdrawal fee.
    Veteran,
}

impl ContributionTier {
    /// Withdrawal fee in basis points.
    pub fn withdrawal_fee_bp(&self) -> u32 {
        match self {
            Self::Newcomer => 1500,    // 15%
            Self::Active => 1000,      // 10%
            Self::Contributor => 500,  //  5%
            Self::Core => 200,         //  2%
            Self::Veteran => 100,      //  1%
        }
    }

    /// Determine tier from a contribution score (0-1000).
    pub fn from_score(score: u64) -> Self {
        if score >= 800 {
            Self::Veteran
        } else if score >= 500 {
            Self::Core
        } else if score >= 200 {
            Self::Contributor
        } else if score >= 50 {
            Self::Active
        } else {
            Self::Newcomer
        }
    }
}

/// Withdrawal fee engine — free deposits, 1-15% dynamic withdrawal fee.
///
/// Deposits into the Helm network (IAO, DeFi, Applications) are always FREE.
/// Withdrawals incur a fee based on the user's network contribution score,
/// structurally incentivizing long-term participation and contribution.
#[derive(Debug)]
pub struct WithdrawalFeeEngine {
    /// Contribution scores per address.
    contribution_scores: HashMap<Address, u64>,
    /// Total fees collected.
    total_fees_collected: TokenAmount,
}

impl WithdrawalFeeEngine {
    pub fn new() -> Self {
        Self {
            contribution_scores: HashMap::new(),
            total_fees_collected: TokenAmount::ZERO,
        }
    }

    /// Record a contribution event for an address.
    pub fn add_contribution(&mut self, address: &Address, points: u64) {
        let score = self.contribution_scores.entry(address.clone()).or_insert(0);
        *score = (*score + points).min(1000);
    }

    /// Get contribution score for an address.
    pub fn contribution_score(&self, address: &Address) -> u64 {
        *self.contribution_scores.get(address).unwrap_or(&0)
    }

    /// Get contribution tier for an address.
    pub fn contribution_tier(&self, address: &Address) -> ContributionTier {
        ContributionTier::from_score(self.contribution_score(address))
    }

    /// Calculate withdrawal fee for a given amount.
    /// Returns (fee_amount, net_amount_after_fee).
    pub fn calculate_fee(
        &self,
        address: &Address,
        amount: TokenAmount,
    ) -> (TokenAmount, TokenAmount) {
        let tier = self.contribution_tier(address);
        let fee_bp = tier.withdrawal_fee_bp();
        let fee = TokenAmount::from_base(
            amount.base_units() * fee_bp as u128 / 10_000,
        );
        let net = TokenAmount::from_base(amount.base_units() - fee.base_units());
        (fee, net)
    }

    /// Process a withdrawal, collecting the fee.
    /// Returns (fee_amount, net_amount).
    pub fn process_withdrawal(
        &mut self,
        address: &Address,
        amount: TokenAmount,
    ) -> Result<(TokenAmount, TokenAmount), TokenError> {
        if amount.is_zero() {
            return Err(TokenError::InvalidAmount("zero withdrawal".into()));
        }
        let (fee, net) = self.calculate_fee(address, amount);
        self.total_fees_collected = self.total_fees_collected.checked_add(fee)?;
        Ok((fee, net))
    }

    pub fn total_fees_collected(&self) -> TokenAmount {
        self.total_fees_collected
    }
}

impl Default for WithdrawalFeeEngine {
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
    fn base_price_no_surge() {
        let pricing = DynamicPricing::new(1);
        let price = pricing.effective_price(&addr("aa"), TokenAmount::ZERO);
        assert_eq!(price.whole_tokens(), 1);
    }

    #[test]
    fn surge_pricing_above_threshold() {
        let mut pricing = DynamicPricing::new(1);
        pricing.set_utilization(10_000); // 100% utilization

        let price = pricing.effective_price(&addr("aa"), TokenAmount::ZERO);
        // At 100%, should be 3x base
        assert_eq!(price.whole_tokens(), 3);
    }

    #[test]
    fn surge_pricing_at_threshold() {
        let mut pricing = DynamicPricing::new(1);
        pricing.set_utilization(7000); // exactly at threshold

        let price = pricing.effective_price(&addr("aa"), TokenAmount::ZERO);
        assert_eq!(price.whole_tokens(), 1); // no surge at threshold
    }

    #[test]
    fn discount_tier_from_calls() {
        assert_eq!(DiscountTier::from_calls(0), DiscountTier::None);
        assert_eq!(DiscountTier::from_calls(100), DiscountTier::Bronze);
        assert_eq!(DiscountTier::from_calls(1000), DiscountTier::Silver);
        assert_eq!(DiscountTier::from_calls(10_000), DiscountTier::Gold);
    }

    #[test]
    fn discount_tier_from_stake() {
        assert_eq!(
            DiscountTier::from_stake(TokenAmount::from_tokens(0)),
            DiscountTier::None
        );
        assert_eq!(
            DiscountTier::from_stake(TokenAmount::from_tokens(10_000)),
            DiscountTier::Bronze
        );
        assert_eq!(
            DiscountTier::from_stake(TokenAmount::from_tokens(100_000)),
            DiscountTier::Silver
        );
        assert_eq!(
            DiscountTier::from_stake(TokenAmount::from_tokens(1_000_000)),
            DiscountTier::Gold
        );
    }

    #[test]
    fn discount_best_of_both() {
        // Low calls but high stake
        let tier = DiscountTier::best(50, TokenAmount::from_tokens(1_000_000));
        assert_eq!(tier, DiscountTier::Gold);

        // High calls but no stake
        let tier = DiscountTier::best(10_000, TokenAmount::ZERO);
        assert_eq!(tier, DiscountTier::Gold);
    }

    #[test]
    fn gold_discount_20_percent() {
        let mut pricing = DynamicPricing::new(100);
        let caller = addr("aa");
        // Make enough calls to get Gold tier
        pricing.rate_limits.insert(
            caller.clone(),
            RateLimitState {
                calls_this_epoch: 0,
                max_calls_per_epoch: 100_000,
                total_calls: 10_000,
            },
        );

        let price = pricing.effective_price(&caller, TokenAmount::ZERO);
        // 100 * (1 - 0.20) = 80
        assert_eq!(price.whole_tokens(), 80);
    }

    #[test]
    fn rate_limit_enforced() {
        let mut pricing = DynamicPricing::new(1);
        let caller = addr("aa");
        pricing.set_rate_limit(&caller, 3);

        pricing.process_call(&caller, TokenAmount::ZERO).unwrap();
        pricing.process_call(&caller, TokenAmount::ZERO).unwrap();
        pricing.process_call(&caller, TokenAmount::ZERO).unwrap();

        // 4th call should fail
        assert!(pricing.process_call(&caller, TokenAmount::ZERO).is_err());
    }

    #[test]
    fn rate_limit_resets_on_epoch() {
        let mut pricing = DynamicPricing::new(1);
        let caller = addr("aa");
        pricing.set_rate_limit(&caller, 2);

        pricing.process_call(&caller, TokenAmount::ZERO).unwrap();
        pricing.process_call(&caller, TokenAmount::ZERO).unwrap();
        assert!(pricing.process_call(&caller, TokenAmount::ZERO).is_err());

        pricing.advance_epoch();
        // After epoch reset, calls work again
        pricing.process_call(&caller, TokenAmount::ZERO).unwrap();
        assert_eq!(pricing.calls_this_epoch(&caller), 1);
    }

    #[test]
    fn total_revenue_accumulates() {
        let mut pricing = DynamicPricing::new(10);
        let caller = addr("aa");

        pricing.process_call(&caller, TokenAmount::ZERO).unwrap();
        pricing.process_call(&caller, TokenAmount::ZERO).unwrap();

        assert_eq!(pricing.total_revenue().whole_tokens(), 20);
    }

    #[test]
    fn total_calls_tracks_across_epochs() {
        let mut pricing = DynamicPricing::new(1);
        let caller = addr("aa");

        pricing.process_call(&caller, TokenAmount::ZERO).unwrap();
        pricing.advance_epoch();
        pricing.process_call(&caller, TokenAmount::ZERO).unwrap();

        assert_eq!(pricing.total_calls(&caller), 2);
        assert_eq!(pricing.calls_this_epoch(&caller), 1);
    }

    // --- Withdrawal Fee Tests ---

    #[test]
    fn newcomer_15_percent_fee() {
        let engine = WithdrawalFeeEngine::new();
        let (fee, net) = engine.calculate_fee(&addr("aa"), TokenAmount::from_tokens(1000));
        assert_eq!(fee.whole_tokens(), 150);  // 15%
        assert_eq!(net.whole_tokens(), 850);
    }

    #[test]
    fn veteran_1_percent_fee() {
        let mut engine = WithdrawalFeeEngine::new();
        let user = addr("aa");
        engine.add_contribution(&user, 900); // Veteran tier

        let (fee, net) = engine.calculate_fee(&user, TokenAmount::from_tokens(1000));
        assert_eq!(fee.whole_tokens(), 10);   // 1%
        assert_eq!(net.whole_tokens(), 990);
    }

    #[test]
    fn contribution_tiers_progressive() {
        assert_eq!(ContributionTier::from_score(0), ContributionTier::Newcomer);
        assert_eq!(ContributionTier::from_score(50), ContributionTier::Active);
        assert_eq!(ContributionTier::from_score(200), ContributionTier::Contributor);
        assert_eq!(ContributionTier::from_score(500), ContributionTier::Core);
        assert_eq!(ContributionTier::from_score(800), ContributionTier::Veteran);
    }

    #[test]
    fn contribution_score_capped_at_1000() {
        let mut engine = WithdrawalFeeEngine::new();
        let user = addr("aa");
        engine.add_contribution(&user, 999);
        engine.add_contribution(&user, 999);
        assert_eq!(engine.contribution_score(&user), 1000);
    }

    #[test]
    fn withdrawal_fee_collected() {
        let mut engine = WithdrawalFeeEngine::new();
        let user = addr("aa");

        let (fee, _) = engine
            .process_withdrawal(&user, TokenAmount::from_tokens(1000))
            .unwrap();

        assert_eq!(fee.whole_tokens(), 150);
        assert_eq!(engine.total_fees_collected().whole_tokens(), 150);
    }

    #[test]
    fn zero_withdrawal_fails() {
        let mut engine = WithdrawalFeeEngine::new();
        assert!(engine
            .process_withdrawal(&addr("aa"), TokenAmount::ZERO)
            .is_err());
    }
}
