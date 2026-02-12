//! Helm Token — core token model with 333B fixed supply and allocation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Total supply: 333 billion (q^5).
pub const TOTAL_SUPPLY: u128 = 333_000_000_000;

/// Decimal precision (18 decimals, EVM-compatible).
pub const DECIMALS: u8 = 18;

/// One full token in base units.
pub const ONE_TOKEN: u128 = 1_000_000_000_000_000_000; // 10^18

/// Total supply in base units.
pub const TOTAL_SUPPLY_BASE: u128 = TOTAL_SUPPLY * ONE_TOKEN;

/// Token allocation categories with percentage shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Allocation {
    /// 60% — Agent mining + Helm Womb staking.
    Mining,
    /// 12% — Early Adopter Organization.
    Eao,
    /// 10% — Liquidity provision.
    Liquidity,
    /// 10% — Protocol treasury.
    Treasury,
    /// 1.5% — Genesis node founder (immediately allocated, staked, DeFi revenue to wallet).
    Founder,
    /// 2.5% — Cabinet (indefinite lockup, DeFi revenue for salaries/operations).
    Cabinet,
    /// 4% — Strategic reserve.
    Reserve,
}

impl Allocation {
    /// Basis points (1 bp = 0.01%) for this allocation.
    pub fn basis_points(&self) -> u32 {
        match self {
            Self::Mining => 6000,    // 60.0%
            Self::Eao => 1200,       // 12.0%
            Self::Liquidity => 1000, // 10.0%
            Self::Treasury => 1000,  // 10.0%
            Self::Founder => 150,    //  1.5%
            Self::Cabinet => 250,    //  2.5%
            Self::Reserve => 400,    //  4.0%
        }
    }

    /// Amount in base units for this allocation.
    pub fn amount(&self) -> u128 {
        TOTAL_SUPPLY_BASE / 10_000 * self.basis_points() as u128
    }

    /// All allocation variants.
    pub fn all() -> &'static [Allocation] {
        &[
            Self::Mining,
            Self::Eao,
            Self::Liquidity,
            Self::Treasury,
            Self::Founder,
            Self::Cabinet,
            Self::Reserve,
        ]
    }
}

impl std::fmt::Display for Allocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mining => write!(f, "Mining (60%)"),
            Self::Eao => write!(f, "EAO (12%)"),
            Self::Liquidity => write!(f, "Liquidity (10%)"),
            Self::Treasury => write!(f, "Treasury (10%)"),
            Self::Founder => write!(f, "Founder (1.5%)"),
            Self::Cabinet => write!(f, "Cabinet (2.5%)"),
            Self::Reserve => write!(f, "Reserve (4%)"),
        }
    }
}

/// Token-related errors.
#[derive(Debug, Error)]
pub enum TokenError {
    #[error("insufficient balance: have {have}, need {need}")]
    InsufficientBalance { have: u128, need: u128 },

    #[error("arithmetic overflow")]
    Overflow,

    #[error("allocation exceeded: {allocation} max={max}, already={already}, requested={requested}")]
    AllocationExceeded {
        allocation: String,
        max: u128,
        already: u128,
        requested: u128,
    },

    #[error("total supply exceeded: minted={minted}, requested={requested}, cap={cap}")]
    SupplyExceeded {
        minted: u128,
        requested: u128,
        cap: u128,
    },

    #[error("invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },

    #[error("genesis already performed")]
    GenesisAlreadyDone,

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("locked: {0}")]
    Locked(String),

    #[error("invalid amount: {0}")]
    InvalidAmount(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("vote error: {0}")]
    VoteError(String),
}

/// A token amount with safe arithmetic and display formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TokenAmount(pub u128);

impl TokenAmount {
    pub const ZERO: Self = Self(0);

    /// Create from whole tokens (multiplied by 10^18).
    pub fn from_tokens(tokens: u128) -> Self {
        Self(tokens * ONE_TOKEN)
    }

    /// Create from base units directly.
    pub fn from_base(base: u128) -> Self {
        Self(base)
    }

    pub fn base_units(&self) -> u128 {
        self.0
    }

    pub fn whole_tokens(&self) -> u128 {
        self.0 / ONE_TOKEN
    }

    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    pub fn checked_add(&self, other: Self) -> Result<Self, TokenError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(TokenError::Overflow)
    }

    pub fn checked_sub(&self, other: Self) -> Result<Self, TokenError> {
        if other.0 > self.0 {
            return Err(TokenError::InsufficientBalance {
                have: self.0,
                need: other.0,
            });
        }
        Ok(Self(self.0 - other.0))
    }

    pub fn checked_mul(&self, factor: u128) -> Result<Self, TokenError> {
        self.0
            .checked_mul(factor)
            .map(Self)
            .ok_or(TokenError::Overflow)
    }

    /// Proportional share: self * numerator / denominator.
    /// Uses GCD reduction to prevent u128 overflow with large token base units.
    pub fn proportional(&self, numerator: u128, denominator: u128) -> Result<Self, TokenError> {
        if denominator == 0 {
            return Err(TokenError::InvalidAmount("division by zero".into()));
        }
        if self.0 == 0 || numerator == 0 {
            return Ok(Self(0));
        }

        // Reduce numerator/denominator by GCD
        let g1 = gcd(numerator, denominator);
        let num = numerator / g1;
        let den = denominator / g1;

        // Reduce self.0 and den by their GCD
        let g2 = gcd(self.0, den);
        let val = self.0 / g2;
        let den = den / g2;

        // Now compute val * num / den (much less likely to overflow)
        match val.checked_mul(num) {
            Some(product) => Ok(Self(product / den)),
            None => {
                // Fallback: scale down, compute, scale back
                let scale = 1_000_000_000u128;
                let val_s = val / scale;
                let rem = val % scale;
                let main = val_s.checked_mul(num).ok_or(TokenError::Overflow)? / den * scale;
                let extra = rem * num / den;
                Ok(Self(main + extra))
            }
        }
    }
}

impl std::fmt::Display for TokenAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let whole = self.0 / ONE_TOKEN;
        let frac = self.0 % ONE_TOKEN;
        if frac == 0 {
            write!(f, "{} HELM", whole)
        } else {
            let frac_str = format!("{:018}", frac);
            let trimmed = frac_str.trim_end_matches('0');
            let display = if trimmed.len() > 8 {
                &trimmed[..8]
            } else {
                trimmed
            };
            write!(f, "{}.{} HELM", whole, display)
        }
    }
}

impl std::ops::Add for TokenAmount {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::Sub for TokenAmount {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

/// Euclidean GCD for u128 (overflow-safe proportional math).
fn gcd(a: u128, b: u128) -> u128 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

/// Helm token ledger — tracks minted supply per allocation category.
#[derive(Debug)]
pub struct HelmToken {
    minted: u128,
    allocated: HashMap<Allocation, u128>,
    genesis_done: bool,
}

impl HelmToken {
    pub fn new() -> Self {
        Self {
            minted: 0,
            allocated: HashMap::new(),
            genesis_done: false,
        }
    }

    /// Mint tokens for a specific allocation category.
    pub fn mint(
        &mut self,
        allocation: Allocation,
        amount: TokenAmount,
    ) -> Result<TokenAmount, TokenError> {
        let max = allocation.amount();
        let already = *self.allocated.get(&allocation).unwrap_or(&0);

        if already + amount.0 > max {
            return Err(TokenError::AllocationExceeded {
                allocation: format!("{}", allocation),
                max,
                already,
                requested: amount.0,
            });
        }

        if self.minted + amount.0 > TOTAL_SUPPLY_BASE {
            return Err(TokenError::SupplyExceeded {
                minted: self.minted,
                requested: amount.0,
                cap: TOTAL_SUPPLY_BASE,
            });
        }

        self.minted += amount.0;
        *self.allocated.entry(allocation).or_insert(0) += amount.0;
        Ok(amount)
    }

    pub fn minted(&self) -> TokenAmount {
        TokenAmount(self.minted)
    }

    pub fn remaining(&self) -> TokenAmount {
        TokenAmount(TOTAL_SUPPLY_BASE - self.minted)
    }

    pub fn allocated(&self, allocation: Allocation) -> TokenAmount {
        TokenAmount(*self.allocated.get(&allocation).unwrap_or(&0))
    }

    pub fn allocation_remaining(&self, allocation: Allocation) -> TokenAmount {
        let max = allocation.amount();
        let used = *self.allocated.get(&allocation).unwrap_or(&0);
        TokenAmount(max - used)
    }

    pub fn is_genesis_done(&self) -> bool {
        self.genesis_done
    }

    pub fn mark_genesis_done(&mut self) {
        self.genesis_done = true;
    }
}

impl Default for HelmToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_supply_constant() {
        assert_eq!(TOTAL_SUPPLY, 333_000_000_000);
        assert_eq!(TOTAL_SUPPLY_BASE, 333_000_000_000 * ONE_TOKEN);
    }

    #[test]
    fn allocation_basis_points_sum_to_100_percent() {
        let total: u32 = Allocation::all().iter().map(|a| a.basis_points()).sum();
        assert_eq!(total, 10_000);
    }

    #[test]
    fn allocation_amounts_sum_to_total_supply() {
        let total: u128 = Allocation::all().iter().map(|a| a.amount()).sum();
        assert_eq!(total, TOTAL_SUPPLY_BASE);
    }

    #[test]
    fn founder_allocation_is_1_5_percent() {
        let founder = Allocation::Founder.amount();
        let expected = TOTAL_SUPPLY_BASE / 10_000 * 150;
        assert_eq!(founder, expected);
        // 333B * 1.5% = 4.995B tokens
        assert_eq!(founder / ONE_TOKEN, 4_995_000_000);
    }

    #[test]
    fn cabinet_allocation_is_2_5_percent() {
        let cabinet = Allocation::Cabinet.amount();
        assert_eq!(cabinet / ONE_TOKEN, 8_325_000_000);
    }

    #[test]
    fn mining_allocation_is_60_percent() {
        let mining = Allocation::Mining.amount();
        assert_eq!(mining / ONE_TOKEN, 199_800_000_000);
    }

    #[test]
    fn token_amount_from_tokens() {
        let amt = TokenAmount::from_tokens(100);
        assert_eq!(amt.whole_tokens(), 100);
        assert_eq!(amt.base_units(), 100 * ONE_TOKEN);
    }

    #[test]
    fn token_amount_display() {
        let whole = TokenAmount::from_tokens(42);
        assert_eq!(whole.to_string(), "42 HELM");

        let frac = TokenAmount::from_base(ONE_TOKEN + ONE_TOKEN / 2);
        assert_eq!(frac.to_string(), "1.5 HELM");
    }

    #[test]
    fn token_amount_checked_arithmetic() {
        let a = TokenAmount::from_tokens(100);
        let b = TokenAmount::from_tokens(50);

        let sum = a.checked_add(b).unwrap();
        assert_eq!(sum.whole_tokens(), 150);

        let diff = a.checked_sub(b).unwrap();
        assert_eq!(diff.whole_tokens(), 50);

        assert!(b.checked_sub(a).is_err());
    }

    #[test]
    fn token_amount_proportional() {
        let total = TokenAmount::from_tokens(1000);
        let share = total.proportional(30, 100).unwrap();
        assert_eq!(share.whole_tokens(), 300);
    }

    #[test]
    fn helm_token_mint_within_allocation() {
        let mut token = HelmToken::new();
        let amt = TokenAmount::from_tokens(1_000_000);
        let result = token.mint(Allocation::Mining, amt);
        assert!(result.is_ok());
        assert_eq!(token.minted().whole_tokens(), 1_000_000);
        assert_eq!(token.allocated(Allocation::Mining).whole_tokens(), 1_000_000);
    }

    #[test]
    fn helm_token_mint_exceeds_allocation() {
        let mut token = HelmToken::new();
        // Founder allocation = 4.995B, try to mint 5B
        let amt = TokenAmount::from_tokens(5_000_000_000);
        let result = token.mint(Allocation::Founder, amt);
        assert!(result.is_err());
    }

    #[test]
    fn helm_token_remaining_tracks_correctly() {
        let mut token = HelmToken::new();
        let initial = token.remaining();
        assert_eq!(initial, TokenAmount::from_base(TOTAL_SUPPLY_BASE));

        token
            .mint(Allocation::Treasury, TokenAmount::from_tokens(1000))
            .unwrap();
        let after = token.remaining();
        assert_eq!(
            after.whole_tokens(),
            initial.whole_tokens() - 1000
        );
    }

    #[test]
    fn allocation_display() {
        assert_eq!(Allocation::Founder.to_string(), "Founder (1.5%)");
        assert_eq!(Allocation::Cabinet.to_string(), "Cabinet (2.5%)");
        assert_eq!(Allocation::Mining.to_string(), "Mining (60%)");
    }

    #[test]
    fn token_amount_serde_roundtrip() {
        let amt = TokenAmount::from_tokens(42);
        let json = serde_json::to_string(&amt).unwrap();
        let decoded: TokenAmount = serde_json::from_str(&json).unwrap();
        assert_eq!(amt, decoded);
    }

    #[test]
    fn allocation_serde_roundtrip() {
        let alloc = Allocation::Founder;
        let json = serde_json::to_string(&alloc).unwrap();
        let decoded: Allocation = serde_json::from_str(&json).unwrap();
        assert_eq!(alloc, decoded);
    }
}
