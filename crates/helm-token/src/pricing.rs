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
}
