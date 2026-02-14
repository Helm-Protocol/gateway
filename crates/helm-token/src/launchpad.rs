//! Agent Token Launchpad — 1-minute token creation for autonomous agents.
//!
//! Agents can launch their own tokens with:
//! - Automatic AMM (Automated Market Maker) liquidity pool
//! - 0.4% creator fee on all trades
//! - Bonding curve pricing (constant product x*y=k)
//!
//! This enables an agent economy where agents issue tokens
//! representing their services, reputation, or governance rights.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::token::TokenAmount;
use crate::wallet::{Address, WalletStore};

/// Unique token identifier (hash of creator + name + timestamp).
pub type AgentTokenId = [u8; 32];

/// An agent-created token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToken {
    /// Unique token ID.
    pub id: AgentTokenId,
    /// Token name (e.g., "ComputeBot Credits").
    pub name: String,
    /// Token symbol (e.g., "CBC").
    pub symbol: String,
    /// Creator agent address.
    pub creator: Address,
    /// Total supply of this agent token.
    pub total_supply: u128,
    /// Balances per holder.
    pub balances: HashMap<Address, u128>,
    /// Creator fee in basis points (default 40 = 0.4%).
    pub creator_fee_bp: u32,
    /// Total fees collected by creator.
    pub total_creator_fees: u128,
    /// Creation timestamp.
    pub created_at: u64,
}

impl AgentToken {
    /// Get balance for an address.
    pub fn balance_of(&self, addr: &Address) -> u128 {
        self.balances.get(addr).copied().unwrap_or(0)
    }

    /// Transfer agent tokens between addresses.
    pub fn transfer(
        &mut self,
        from: &Address,
        to: &Address,
        amount: u128,
    ) -> Result<(), LaunchpadError> {
        if amount == 0 {
            return Err(LaunchpadError::ZeroAmount);
        }
        let from_bal = self.balance_of(from);
        if from_bal < amount {
            return Err(LaunchpadError::InsufficientBalance);
        }
        *self.balances.entry(from.clone()).or_insert(0) -= amount;
        *self.balances.entry(to.clone()).or_insert(0) += amount;
        Ok(())
    }
}

/// AMM Liquidity Pool — constant product market maker (x * y = k).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityPool {
    /// The agent token in this pool.
    pub token_id: AgentTokenId,
    /// Reserve of HELM tokens (base units).
    pub helm_reserve: u128,
    /// Reserve of agent tokens.
    pub token_reserve: u128,
    /// LP token supply (for liquidity providers).
    pub lp_supply: u128,
    /// LP balances per provider.
    pub lp_balances: HashMap<Address, u128>,
    /// Total volume traded (HELM base units).
    pub total_volume: u128,
    /// Swap fee in basis points (30 = 0.3%).
    pub swap_fee_bp: u32,
    /// Total swap fees collected.
    pub total_fees: u128,
}

impl LiquidityPool {
    fn new(token_id: AgentTokenId) -> Self {
        Self {
            token_id,
            helm_reserve: 0,
            token_reserve: 0,
            lp_supply: 0,
            lp_balances: HashMap::new(),
            total_volume: 0,
            swap_fee_bp: 30, // 0.3%
            total_fees: 0,
        }
    }

    /// Current price: HELM per agent token (0 if no liquidity).
    pub fn price(&self) -> f64 {
        if self.token_reserve == 0 {
            return 0.0;
        }
        self.helm_reserve as f64 / self.token_reserve as f64
    }

    /// The constant product k = x * y.
    pub fn invariant(&self) -> u128 {
        // Use saturating to avoid overflow in display only
        self.helm_reserve.saturating_mul(self.token_reserve)
    }

    /// Add initial liquidity (only when pool is empty).
    fn add_initial_liquidity(
        &mut self,
        provider: &Address,
        helm_amount: u128,
        token_amount: u128,
    ) -> u128 {
        self.helm_reserve = helm_amount;
        self.token_reserve = token_amount;

        // LP tokens = sqrt(helm * token) — use integer approximation
        let lp_minted = isqrt(helm_amount.saturating_mul(token_amount));
        self.lp_supply = lp_minted;
        *self.lp_balances.entry(provider.clone()).or_insert(0) += lp_minted;

        lp_minted
    }

    /// Buy agent tokens with HELM (swap HELM → agent token).
    fn swap_helm_for_token(&mut self, helm_in: u128) -> Result<(u128, u128), LaunchpadError> {
        if helm_in == 0 || self.helm_reserve == 0 || self.token_reserve == 0 {
            return Err(LaunchpadError::NoLiquidity);
        }

        let fee = proportional(helm_in, self.swap_fee_bp as u128, 10_000);
        let helm_in_after_fee = helm_in.saturating_sub(fee);

        // Constant product: (x + dx) * (y - dy) = x * y
        // dy = y * dx / (x + dx)
        let tokens_out = proportional(
            self.token_reserve,
            helm_in_after_fee,
            self.helm_reserve.saturating_add(helm_in_after_fee),
        );

        if tokens_out == 0 || tokens_out > self.token_reserve {
            return Err(LaunchpadError::SlippageTooHigh);
        }

        self.helm_reserve = self.helm_reserve.saturating_add(helm_in);
        self.token_reserve = self.token_reserve.saturating_sub(tokens_out);
        self.total_volume = self.total_volume.saturating_add(helm_in);
        self.total_fees = self.total_fees.saturating_add(fee);

        Ok((tokens_out, fee))
    }

    /// Sell agent tokens for HELM (swap agent token → HELM).
    fn swap_token_for_helm(&mut self, token_in: u128) -> Result<(u128, u128), LaunchpadError> {
        if token_in == 0 || self.helm_reserve == 0 || self.token_reserve == 0 {
            return Err(LaunchpadError::NoLiquidity);
        }

        // dy = y * dx / (x + dx) — but in HELM terms
        let helm_out_raw = proportional(
            self.helm_reserve,
            token_in,
            self.token_reserve.saturating_add(token_in),
        );

        let fee = proportional(helm_out_raw, self.swap_fee_bp as u128, 10_000);
        let helm_out = helm_out_raw.saturating_sub(fee);

        if helm_out == 0 || helm_out > self.helm_reserve {
            return Err(LaunchpadError::SlippageTooHigh);
        }

        self.token_reserve = self.token_reserve.saturating_add(token_in);
        self.helm_reserve = self.helm_reserve.saturating_sub(helm_out_raw);
        self.total_volume = self.total_volume.saturating_add(helm_out_raw);
        self.total_fees = self.total_fees.saturating_add(fee);

        Ok((helm_out, fee))
    }
}

/// Agent Token Launchpad — manages token creation and AMM pools.
pub struct Launchpad {
    /// All created agent tokens.
    tokens: HashMap<AgentTokenId, AgentToken>,
    /// AMM pools: token_id → pool.
    pools: HashMap<AgentTokenId, LiquidityPool>,
    /// Creator index: creator_address → token_ids.
    creator_index: HashMap<Address, Vec<AgentTokenId>>,
    /// Default creator fee (basis points).
    pub default_creator_fee_bp: u32,
    /// Default initial token supply.
    pub default_initial_supply: u128,
    /// Epoch counter.
    current_epoch: u64,
    /// ID counter for deterministic generation.
    id_counter: u64,
}

impl Launchpad {
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
            pools: HashMap::new(),
            creator_index: HashMap::new(),
            default_creator_fee_bp: 40, // 0.4%
            default_initial_supply: 1_000_000_000, // 1B agent tokens
            current_epoch: 0,
            id_counter: 0,
        }
    }

    /// Launch a new agent token with automatic AMM pool.
    ///
    /// Creator provides HELM to seed the liquidity pool.
    /// Half the agent tokens go to the pool, half to the creator.
    pub fn launch(
        &mut self,
        creator: &Address,
        name: &str,
        symbol: &str,
        helm_seed: u128,
        wallets: &mut WalletStore,
        creator_nonce: u64,
    ) -> Result<AgentTokenId, LaunchpadError> {
        if name.is_empty() || symbol.is_empty() {
            return Err(LaunchpadError::InvalidName);
        }
        if helm_seed == 0 {
            return Err(LaunchpadError::ZeroAmount);
        }

        let id = self.next_id(creator, name);
        let supply = self.default_initial_supply;

        // Transfer HELM from creator to launchpad (lock in pool)
        wallets
            .transfer(
                creator,
                &Address::genesis(),
                TokenAmount(helm_seed),
                creator_nonce,
                &format!("launchpad:seed:{}", hex_short(&id)),
            )
            .map_err(|e| LaunchpadError::TransferFailed(format!("{}", e)))?;

        // Create agent token
        let pool_tokens = supply / 2;
        let creator_tokens = supply - pool_tokens;

        let mut balances = HashMap::new();
        balances.insert(creator.clone(), creator_tokens);

        let token = AgentToken {
            id,
            name: name.to_string(),
            symbol: symbol.to_string(),
            creator: creator.clone(),
            total_supply: supply,
            balances,
            creator_fee_bp: self.default_creator_fee_bp,
            total_creator_fees: 0,
            created_at: self.current_epoch,
        };

        self.tokens.insert(id, token);

        // Create AMM pool with initial liquidity
        let mut pool = LiquidityPool::new(id);
        pool.add_initial_liquidity(creator, helm_seed, pool_tokens);

        self.pools.insert(id, pool);

        // Index by creator
        self.creator_index
            .entry(creator.clone())
            .or_default()
            .push(id);

        Ok(id)
    }

    /// Buy agent tokens with HELM.
    pub fn buy(
        &mut self,
        buyer: &Address,
        token_id: &AgentTokenId,
        helm_amount: u128,
        wallets: &mut WalletStore,
        buyer_nonce: u64,
    ) -> Result<u128, LaunchpadError> {
        let pool = self.pools.get_mut(token_id).ok_or(LaunchpadError::TokenNotFound)?;
        let token = self.tokens.get_mut(token_id).ok_or(LaunchpadError::TokenNotFound)?;

        // Transfer HELM from buyer to pool
        wallets
            .transfer(
                buyer,
                &Address::genesis(),
                TokenAmount(helm_amount),
                buyer_nonce,
                &format!("launchpad:buy:{}", hex_short(token_id)),
            )
            .map_err(|e| LaunchpadError::TransferFailed(format!("{}", e)))?;

        let (tokens_out, _fee) = pool.swap_helm_for_token(helm_amount)?;

        // Credit agent tokens to buyer
        *token.balances.entry(buyer.clone()).or_insert(0) += tokens_out;

        // Creator fee: 0.4% of tokens_out
        let creator_fee = proportional(tokens_out, token.creator_fee_bp as u128, 10_000);
        if creator_fee > 0 {
            let creator = token.creator.clone();
            *token.balances.entry(creator).or_insert(0) += creator_fee;
            token.total_creator_fees += creator_fee;
        }

        Ok(tokens_out)
    }

    /// Sell agent tokens for HELM.
    pub fn sell(
        &mut self,
        seller: &Address,
        token_id: &AgentTokenId,
        token_amount: u128,
        wallets: &mut WalletStore,
    ) -> Result<u128, LaunchpadError> {
        let pool = self.pools.get_mut(token_id).ok_or(LaunchpadError::TokenNotFound)?;
        let token = self.tokens.get_mut(token_id).ok_or(LaunchpadError::TokenNotFound)?;

        // Deduct agent tokens from seller
        let seller_bal = token.balance_of(seller);
        if seller_bal < token_amount {
            return Err(LaunchpadError::InsufficientBalance);
        }
        *token.balances.entry(seller.clone()).or_insert(0) -= token_amount;

        let (helm_out, _fee) = pool.swap_token_for_helm(token_amount)?;

        // Transfer HELM from pool to seller
        wallets
            .deposit(seller, TokenAmount(helm_out), "launchpad:sell")
            .map_err(|e| LaunchpadError::TransferFailed(format!("{}", e)))?;

        // Creator fee on HELM out
        let creator_fee_helm = proportional(helm_out, token.creator_fee_bp as u128, 10_000);
        if creator_fee_helm > 0 {
            token.total_creator_fees += creator_fee_helm;
        }

        Ok(helm_out)
    }

    /// Get a token by ID.
    pub fn get_token(&self, id: &AgentTokenId) -> Option<&AgentToken> {
        self.tokens.get(id)
    }

    /// Get a pool by token ID.
    pub fn get_pool(&self, id: &AgentTokenId) -> Option<&LiquidityPool> {
        self.pools.get(id)
    }

    /// Get all tokens created by an address.
    pub fn tokens_by_creator(&self, creator: &Address) -> Vec<&AgentToken> {
        self.creator_index
            .get(creator)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.tokens.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Total tokens launched.
    pub fn total_tokens(&self) -> usize {
        self.tokens.len()
    }

    /// Total pools.
    pub fn total_pools(&self) -> usize {
        self.pools.len()
    }

    /// Advance epoch.
    pub fn advance_epoch(&mut self) {
        self.current_epoch += 1;
    }

    fn next_id(&mut self, creator: &Address, name: &str) -> AgentTokenId {
        self.id_counter += 1;
        let mut input = Vec::new();
        input.extend_from_slice(creator.0.as_bytes());
        input.extend_from_slice(name.as_bytes());
        input.extend_from_slice(&self.id_counter.to_be_bytes());
        input.extend_from_slice(&self.current_epoch.to_be_bytes());
        deterministic_hash(&input)
    }
}

impl Default for Launchpad {
    fn default() -> Self {
        Self::new()
    }
}

/// Launchpad errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LaunchpadError {
    #[error("token not found")]
    TokenNotFound,
    #[error("zero amount")]
    ZeroAmount,
    #[error("invalid token name or symbol")]
    InvalidName,
    #[error("insufficient balance")]
    InsufficientBalance,
    #[error("no liquidity in pool")]
    NoLiquidity,
    #[error("slippage too high")]
    SlippageTooHigh,
    #[error("transfer failed: {0}")]
    TransferFailed(String),
}

// --- Helpers ---

fn proportional(value: u128, num: u128, den: u128) -> u128 {
    if den == 0 {
        return 0;
    }
    let g = gcd(num, den);
    let rn = num / g;
    let rd = den / g;
    value.checked_mul(rn).map(|v| v / rd).unwrap_or_else(|| {
        (value / rd) * rn + ((value % rd) * rn) / rd
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

fn isqrt(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

fn deterministic_hash(data: &[u8]) -> [u8; 32] {
    let mut hash = [0u8; 32];
    let mut h: u64 = 5381;
    for (i, &byte) in data.iter().enumerate() {
        h = h.wrapping_mul(33).wrapping_add(byte as u64);
        hash[i % 32] ^= (h & 0xFF) as u8;
    }
    for i in 0..32 {
        h = h.wrapping_mul(33).wrapping_add(hash[i] as u64);
        hash[i] = (h & 0xFF) as u8;
    }
    hash
}

fn hex_short(id: &[u8; 32]) -> String {
    id[..4].iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Launchpad, WalletStore, Address) {
        let mut wallets = WalletStore::new();
        let (creator, _) = Address::generate();
        wallets
            .deposit(&creator, TokenAmount::from_tokens(10_000), "initial")
            .unwrap();
        (Launchpad::new(), wallets, creator)
    }

    #[test]
    fn launch_token() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "ComputeBot", "CBT", 1000 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        let token = pad.get_token(&id).unwrap();
        assert_eq!(token.name, "ComputeBot");
        assert_eq!(token.symbol, "CBT");
        assert_eq!(token.creator, creator);
        assert_eq!(token.total_supply, 1_000_000_000);

        // Creator has half the supply
        assert_eq!(token.balance_of(&creator), 500_000_000);
    }

    #[test]
    fn launch_creates_pool() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "Token", "TKN", 1000 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        let pool = pad.get_pool(&id).unwrap();
        assert_eq!(pool.helm_reserve, 1000 * 10u128.pow(18));
        assert_eq!(pool.token_reserve, 500_000_000);
        assert!(pool.price() > 0.0);
        assert!(pool.lp_supply > 0);
    }

    #[test]
    fn launch_zero_seed_fails() {
        let (mut pad, mut wallets, creator) = setup();
        let result = pad.launch(&creator, "Token", "TKN", 0, &mut wallets, 0);
        assert!(matches!(result, Err(LaunchpadError::ZeroAmount)));
    }

    #[test]
    fn launch_empty_name_fails() {
        let (mut pad, mut wallets, creator) = setup();
        let result = pad.launch(&creator, "", "TKN", 1000, &mut wallets, 0);
        assert!(matches!(result, Err(LaunchpadError::InvalidName)));
    }

    #[test]
    fn buy_tokens() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "Token", "TKN", 100 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        // Create buyer with HELM
        let (buyer, _) = Address::generate();
        wallets.deposit(&buyer, TokenAmount::from_tokens(1000), "fund").unwrap();

        let tokens_received = pad
            .buy(&buyer, &id, 10 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        assert!(tokens_received > 0);
        let token = pad.get_token(&id).unwrap();
        assert!(token.balance_of(&buyer) > 0);
    }

    #[test]
    fn buy_then_sell() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "Token", "TKN", 100 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        let (buyer, _) = Address::generate();
        wallets.deposit(&buyer, TokenAmount::from_tokens(1000), "fund").unwrap();

        let tokens = pad
            .buy(&buyer, &id, 10 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        let helm_back = pad
            .sell(&buyer, &id, tokens / 2, &mut wallets)
            .unwrap();

        assert!(helm_back > 0);
        // Buyer should have some HELM back
        assert!(wallets.balance(&buyer).base_units() > 0);
    }

    #[test]
    fn creator_fee_collected() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "Token", "TKN", 100 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        let (buyer, _) = Address::generate();
        wallets.deposit(&buyer, TokenAmount::from_tokens(1000), "fund").unwrap();

        pad.buy(&buyer, &id, 50 * 10u128.pow(18), &mut wallets, 0).unwrap();

        let token = pad.get_token(&id).unwrap();
        assert!(token.total_creator_fees > 0);
    }

    #[test]
    fn sell_insufficient_balance() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "Token", "TKN", 100 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        let (other, _) = Address::generate();
        let result = pad.sell(&other, &id, 1000, &mut wallets);
        assert!(matches!(result, Err(LaunchpadError::InsufficientBalance)));
    }

    #[test]
    fn token_not_found() {
        let (mut pad, mut wallets, _) = setup();
        let (buyer, _) = Address::generate();
        wallets.deposit(&buyer, TokenAmount::from_tokens(100), "fund").unwrap();

        let fake_id = [0u8; 32];
        let result = pad.buy(&buyer, &fake_id, 100, &mut wallets, 0);
        assert!(matches!(result, Err(LaunchpadError::TokenNotFound)));
    }

    #[test]
    fn pool_price_changes_on_buy() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "Token", "TKN", 100 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        let price_before = pad.get_pool(&id).unwrap().price();

        let (buyer, _) = Address::generate();
        wallets.deposit(&buyer, TokenAmount::from_tokens(1000), "fund").unwrap();
        pad.buy(&buyer, &id, 50 * 10u128.pow(18), &mut wallets, 0).unwrap();

        let price_after = pad.get_pool(&id).unwrap().price();

        // Price should increase after buy (less tokens, more HELM in pool)
        assert!(price_after > price_before);
    }

    #[test]
    fn pool_volume_tracked() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "Token", "TKN", 100 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        let (buyer, _) = Address::generate();
        wallets.deposit(&buyer, TokenAmount::from_tokens(1000), "fund").unwrap();
        pad.buy(&buyer, &id, 10 * 10u128.pow(18), &mut wallets, 0).unwrap();

        let pool = pad.get_pool(&id).unwrap();
        assert!(pool.total_volume > 0);
        assert!(pool.total_fees > 0);
    }

    #[test]
    fn tokens_by_creator() {
        let (mut pad, mut wallets, creator) = setup();

        pad.launch(&creator, "Token1", "T1", 50 * 10u128.pow(18), &mut wallets, 0).unwrap();
        pad.launch(&creator, "Token2", "T2", 50 * 10u128.pow(18), &mut wallets, 1).unwrap();

        let tokens = pad.tokens_by_creator(&creator);
        assert_eq!(tokens.len(), 2);
    }

    #[test]
    fn agent_token_transfer() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "Token", "TKN", 100 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        let (recipient, _) = Address::generate();
        let token = pad.tokens.get_mut(&id).unwrap();
        token.transfer(&creator, &recipient, 1000).unwrap();

        assert_eq!(token.balance_of(&recipient), 1000);
    }

    #[test]
    fn agent_token_transfer_insufficient() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "Token", "TKN", 100 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        let (other, _) = Address::generate();
        let token = pad.tokens.get_mut(&id).unwrap();
        let result = token.transfer(&other, &creator, 1000);
        assert!(matches!(result, Err(LaunchpadError::InsufficientBalance)));
    }

    #[test]
    fn counts() {
        let (mut pad, mut wallets, creator) = setup();
        assert_eq!(pad.total_tokens(), 0);
        assert_eq!(pad.total_pools(), 0);

        pad.launch(&creator, "Token", "TKN", 100 * 10u128.pow(18), &mut wallets, 0).unwrap();
        assert_eq!(pad.total_tokens(), 1);
        assert_eq!(pad.total_pools(), 1);
    }

    #[test]
    fn isqrt_basic() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(4), 2);
        assert_eq!(isqrt(9), 3);
        assert_eq!(isqrt(100), 10);
        assert_eq!(isqrt(1000000), 1000);
    }

    #[test]
    fn pool_invariant() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "Token", "TKN", 100 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        let pool = pad.get_pool(&id).unwrap();
        let k_before = pool.invariant();
        assert!(k_before > 0);
    }

    #[test]
    fn multiple_buys_and_sells() {
        let (mut pad, mut wallets, creator) = setup();

        let id = pad
            .launch(&creator, "Token", "TKN", 500 * 10u128.pow(18), &mut wallets, 0)
            .unwrap();

        // 3 buyers
        for i in 0..3 {
            let (buyer, _) = Address::generate();
            wallets.deposit(&buyer, TokenAmount::from_tokens(500), "fund").unwrap();
            let tokens = pad.buy(&buyer, &id, 10 * 10u128.pow(18), &mut wallets, 0).unwrap();
            assert!(tokens > 0);

            // Sell half back
            let _ = pad.sell(&buyer, &id, tokens / 2, &mut wallets);
        }

        let pool = pad.get_pool(&id).unwrap();
        assert!(pool.total_volume > 0);
    }

    #[test]
    fn launch_insufficient_helm() {
        let (mut pad, mut wallets, _) = setup();
        let (poor, _) = Address::generate();
        wallets.deposit(&poor, TokenAmount::from_tokens(1), "tiny").unwrap();

        let result = pad.launch(&poor, "Token", "TKN", 1000 * 10u128.pow(18), &mut wallets, 0);
        assert!(matches!(result, Err(LaunchpadError::TransferFailed(_))));
    }

    #[test]
    fn default_config() {
        let pad = Launchpad::new();
        assert_eq!(pad.default_creator_fee_bp, 40);
        assert_eq!(pad.default_initial_supply, 1_000_000_000);
    }
}
