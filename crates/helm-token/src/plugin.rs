//! TokenPlugin — integrates the token system with the helm-core EventLoop.
//!
//! Responsibilities:
//! - on_start: genesis initialization (if genesis node)
//! - on_message: handle transfer/stake/treasury requests
//! - on_tick: epoch advancement, staking reward settlement, salary payments
//! - on_shutdown: flush state

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use helm_core::{Plugin, PluginContext, PluginEvent};
use helm_net::protocol::HelmMessage;

use crate::cabinet::Cabinet;
use crate::genesis::{execute_genesis, GenesisConfig};
use crate::pricing::DynamicPricing;
use crate::staking::StakePool;
use crate::token::HelmToken;
use crate::treasury::{HelmTreasury, TreasuryBucket};
use crate::wallet::{Address, WalletStore};
use crate::launchpad::Launchpad;
use crate::x402::PaymentProtocol;

/// Token plugin configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPluginConfig {
    /// Whether this node is the genesis node.
    pub is_genesis: bool,
    /// Genesis configuration (required if is_genesis).
    pub genesis_config: Option<GenesisConfig>,
    /// Ticks per epoch (staking reward settlement interval).
    pub ticks_per_epoch: u64,
    /// Base API price in tokens.
    pub base_api_price: u128,
}

impl Default for TokenPluginConfig {
    fn default() -> Self {
        Self {
            is_genesis: false,
            genesis_config: None,
            ticks_per_epoch: 100,
            base_api_price: 1,
        }
    }
}

/// Token request types embedded in HelmMessage payloads.
#[derive(Debug, Serialize, Deserialize)]
pub enum TokenRequest {
    /// Transfer tokens.
    Transfer {
        from: Address,
        to: Address,
        amount: u128,
        nonce: u64,
    },
    /// Stake tokens.
    Stake {
        staker: Address,
        amount: u128,
        lock_epochs: u64,
    },
    /// Unstake tokens.
    Unstake {
        staker: Address,
        stake_index: usize,
    },
    /// Claim DeFi revenue.
    ClaimRevenue {
        staker: Address,
    },
    /// Query balance.
    QueryBalance {
        address: Address,
    },
}

/// The token plugin integrating all token subsystems.
pub struct TokenPlugin {
    config: TokenPluginConfig,
    pub token: HelmToken,
    pub wallets: WalletStore,
    pub stake_pool: StakePool,
    pub treasury: HelmTreasury,
    pub cabinet: Cabinet,
    pub pricing: DynamicPricing,
    pub payment_protocol: PaymentProtocol,
    pub launchpad: Launchpad,
    tick_count: u64,
}

impl TokenPlugin {
    pub fn new(config: TokenPluginConfig) -> Self {
        let pricing = DynamicPricing::new(config.base_api_price);
        // Fee collector address — protocol escrow vault
        let fee_collector = Address(format!("{:0>64}", "x402_fee"));
        Self {
            config,
            token: HelmToken::new(),
            wallets: WalletStore::new(),
            stake_pool: StakePool::new(),
            treasury: HelmTreasury::new(),
            cabinet: Cabinet::new(),
            pricing,
            payment_protocol: PaymentProtocol::new(fee_collector),
            launchpad: Launchpad::new(),
            tick_count: 0,
        }
    }

    /// Run genesis allocation (only on genesis node, only once).
    fn run_genesis(&mut self) -> Result<()> {
        if !self.config.is_genesis || self.token.is_genesis_done() {
            return Ok(());
        }

        let genesis_config = self
            .config
            .genesis_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("genesis node requires genesis_config"))?
            .clone();

        let result = execute_genesis(
            &genesis_config,
            &mut self.token,
            &mut self.wallets,
            &mut self.stake_pool,
            &mut self.treasury,
        )
        .map_err(|e| anyhow::anyhow!("genesis failed: {}", e))?;

        info!(
            total_minted = %result.total_minted,
            founder = %result.founder_allocation,
            cabinet = %result.cabinet_allocation,
            mining = %result.mining_allocation,
            "Genesis allocation complete"
        );

        Ok(())
    }

    /// Process an epoch tick: distribute staking rewards, pay salaries.
    fn process_epoch(&mut self) -> Result<()> {
        // 1. Collect pricing revenue into treasury
        let api_revenue = self.pricing.total_revenue();
        if !api_revenue.is_zero() {
            self.treasury
                .collect_edge_api_revenue(api_revenue)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        }

        // 2. Sweep x402 protocol fees into treasury
        let x402_fees = self.payment_protocol.total_fees_collected;
        if !x402_fees.is_zero() {
            self.treasury
                .collect_revenue(x402_fees, "x402 protocol fees")
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            self.payment_protocol.total_fees_collected = crate::token::TokenAmount::ZERO;
            debug!(fees = %x402_fees, "x402 protocol fees swept to treasury");
        }

        // 3. Expire overdue escrows (refund buyers)
        let expired = self.payment_protocol.expire_overdue(&mut self.wallets);
        if !expired.is_empty() {
            debug!(count = expired.len(), "Expired overdue escrows");
        }

        // 4. Allocate treasury to buckets
        self.treasury
            .allocate()
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // 5. Distribute staking rewards from treasury bucket
        let staking_rewards = self.treasury.staking_rewards_available();
        if !staking_rewards.is_zero() {
            // Disburse from treasury to stakers
            let dummy = Address::genesis();
            self.treasury
                .disburse(
                    TreasuryBucket::StakingRewards,
                    staking_rewards,
                    &dummy,
                    "epoch staking rewards",
                )
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            let dist = self
                .stake_pool
                .distribute_revenue(staking_rewards)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            debug!(
                stakers = dist.len(),
                total = %staking_rewards,
                "Distributed staking rewards"
            );
        }

        // Advance epochs
        self.stake_pool.advance_epoch();
        self.treasury.advance_epoch();
        self.pricing.advance_epoch();
        self.cabinet.advance_epoch();
        self.payment_protocol.advance_epoch();
        self.launchpad.advance_epoch();

        Ok(())
    }

    /// Handle a token request from a network message.
    pub fn handle_request(&mut self, request: TokenRequest) -> Result<()> {
        match request {
            TokenRequest::Transfer {
                from,
                to,
                amount,
                nonce,
            } => {
                let amt = crate::token::TokenAmount::from_base(amount);
                self.wallets
                    .transfer(&from, &to, amt, nonce, "network transfer")
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                debug!(from = %from, to = %to, amount = %amt, "Transfer processed");
            }
            TokenRequest::Stake {
                staker,
                amount,
                lock_epochs,
            } => {
                let amt = crate::token::TokenAmount::from_base(amount);
                self.stake_pool
                    .stake(&staker, amt, crate::staking::StakeType::General, lock_epochs)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                debug!(staker = %staker, amount = %amt, "Stake processed");
            }
            TokenRequest::Unstake {
                staker,
                stake_index,
            } => {
                let returned = self
                    .stake_pool
                    .unstake(&staker, stake_index)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                debug!(staker = %staker, returned = %returned, "Unstake processed");
            }
            TokenRequest::ClaimRevenue { staker } => {
                let claimed = self
                    .stake_pool
                    .claim_revenue(&staker)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if !claimed.is_zero() {
                    self.wallets
                        .deposit(&staker, claimed, "DeFi revenue claim")
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    debug!(staker = %staker, claimed = %claimed, "Revenue claimed → wallet");
                }
            }
            TokenRequest::QueryBalance { address } => {
                let balance = self.wallets.balance(&address);
                debug!(address = %address, balance = %balance, "Balance query");
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Plugin for TokenPlugin {
    fn name(&self) -> &str {
        "helm-token"
    }

    async fn on_start(&mut self, ctx: &mut PluginContext) -> Result<()> {
        info!(node = %ctx.node_name, "TokenPlugin starting");
        self.run_genesis()?;
        Ok(())
    }

    async fn on_message(&mut self, _ctx: &mut PluginContext, msg: &HelmMessage) -> Result<()> {
        // Try to parse token requests from TaskRequest messages
        if msg.kind == helm_net::protocol::MessageKind::TaskRequest {
            if let Some(kind_str) = msg.payload.get("kind").and_then(|v| v.as_str()) {
                if kind_str == "token" {
                    if let Some(data) = msg.payload.get("data") {
                        match serde_json::from_value::<TokenRequest>(data.clone()) {
                            Ok(request) => {
                                if let Err(e) = self.handle_request(request) {
                                    warn!(error = %e, "Token request failed");
                                }
                            }
                            Err(_) => {
                                // Malformed token request, ignore
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn on_tick(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        self.tick_count += 1;

        if self.tick_count.is_multiple_of(self.config.ticks_per_epoch) {
            if let Err(e) = self.process_epoch() {
                warn!(error = %e, "Epoch processing failed");
            }
        }

        Ok(())
    }

    async fn on_event(&mut self, _ctx: &mut PluginContext, event: &PluginEvent) -> Result<()> {
        match event {
            PluginEvent::ApiRevenue { caller, amount_units, endpoint } => {
                let amt = crate::token::TokenAmount::from_base(*amount_units as u128);
                if let Err(e) = self.pricing.process_call(
                    &Address(caller.clone()),
                    amt,
                ) {
                    warn!(error = %e, "API revenue processing failed");
                } else {
                    debug!(caller = %caller, amount = amount_units, endpoint = %endpoint, "API revenue recorded");
                }
            }
            PluginEvent::Custom { target_plugin, event_type, payload, .. } => {
                if target_plugin != "helm-token" {
                    return Ok(());
                }
                match event_type.as_str() {
                    "womb_wallet_create" => {
                        let agent_id = payload.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
                        let stake = payload.get("existence_stake").and_then(|v| v.as_u64()).unwrap_or(0);
                        let addr = Address(format!("{:0>64}", agent_id));

                        // Create wallet for the newborn agent
                        self.wallets.get_or_create(&addr);

                        if stake > 0 {
                            let amt = crate::token::TokenAmount::from_tokens(stake as u128);
                            // Deposit existence stake (minted from mining pool conceptually)
                            if let Err(e) = self.wallets.deposit(&addr, amt, "existence_stake:birth") {
                                warn!(agent = %agent_id, error = %e, "Existence stake deposit failed");
                            } else {
                                // Auto-stake the existence deposit
                                if let Err(e) = self.stake_pool.stake(
                                    &addr,
                                    amt,
                                    crate::staking::StakeType::Mining,
                                    0, // indefinite lock
                                ) {
                                    warn!(agent = %agent_id, error = %e, "Existence stake lock failed");
                                } else {
                                    // Sync voting power to governance
                                    _ctx.emit(PluginEvent::Custom {
                                        source_plugin: "helm-token".to_string(),
                                        target_plugin: "helm-governance".to_string(),
                                        event_type: "stake_sync".to_string(),
                                        payload: serde_json::json!({
                                            "voter": agent_id,
                                            "power": amt.whole_tokens(),
                                        }),
                                    });
                                    debug!(agent = %agent_id, stake = %amt, "Agent wallet created + existence staked");
                                }
                            }
                        } else {
                            debug!(agent = %agent_id, "Agent wallet created (no stake)");
                        }
                    }
                    "mining_reward_deposit" => {
                        let agent_id = payload.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
                        let amount = payload.get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
                        if amount > 0 {
                            let addr = Address(format!("{:0>64}", agent_id));
                            let amt = crate::token::TokenAmount::from_base(amount as u128);
                            if let Err(e) = self.wallets.deposit(&addr, amt, "mining_reward") {
                                warn!(agent = %agent_id, error = %e, "Mining reward deposit failed");
                            } else {
                                debug!(agent = %agent_id, amount = %amt, "Mining reward deposited");
                            }
                        }
                    }
                    "womb_launch_token" => {
                        let agent_id = payload.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
                        let creator_str = payload.get("creator").and_then(|v| v.as_str()).unwrap_or("");
                        let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or(agent_id);

                        let creator = Address(format!("{:0>64}", creator_str));
                        // Generate symbol from agent name (first 4 chars uppercase)
                        let symbol: String = name.chars()
                            .filter(|c| c.is_alphanumeric())
                            .take(4)
                            .collect::<String>()
                            .to_uppercase();

                        // Seed with 1 HELM for initial liquidity pool
                        let seed = crate::token::ONE_TOKEN;
                        match self.launchpad.launch(
                            &creator,
                            name,
                            &symbol,
                            seed,
                            &mut self.wallets,
                            0, // auto nonce for genesis launch
                        ) {
                            Ok(token_id) => {
                                debug!(agent = %agent_id, token = ?token_id, "Agent token launched");
                            }
                            Err(e) => {
                                warn!(agent = %agent_id, error = %e, "Agent token launch failed");
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn on_shutdown(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        info!(
            minted = %self.token.minted(),
            wallets = self.wallets.wallet_count(),
            stakers = self.stake_pool.staker_count(),
            "TokenPlugin shutting down"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::{TOTAL_SUPPLY, ONE_TOKEN};

    fn genesis_plugin() -> TokenPlugin {
        let config = TokenPluginConfig {
            is_genesis: true,
            genesis_config: Some(GenesisConfig {
                founder_address: Address(format!("{:0>64}", "f0")),
                cabinet_address: Address(format!("{:0>64}", "ca")),
                treasury_address: Address(format!("{:0>64}", "tr")),
                liquidity_address: Address(format!("{:0>64}", "lq")),
                reserve_address: Address(format!("{:0>64}", "rs")),
                eao_address: Address(format!("{:0>64}", "ea")),
                mining_address: Address(format!("{:0>64}", "mn")),
            }),
            ticks_per_epoch: 10,
            base_api_price: 1,
        };
        TokenPlugin::new(config)
    }

    #[tokio::test]
    async fn plugin_genesis_on_start() {
        let mut plugin = genesis_plugin();
        let mut ctx = PluginContext::new("genesis-node".to_string());

        plugin.on_start(&mut ctx).await.unwrap();

        assert!(plugin.token.is_genesis_done());
        assert_eq!(plugin.token.minted().whole_tokens(), TOTAL_SUPPLY);
    }

    #[tokio::test]
    async fn plugin_non_genesis_skips() {
        let mut plugin = TokenPlugin::new(TokenPluginConfig::default());
        let mut ctx = PluginContext::new("regular-node".to_string());

        plugin.on_start(&mut ctx).await.unwrap();
        assert!(!plugin.token.is_genesis_done());
    }

    #[tokio::test]
    async fn plugin_name() {
        let plugin = genesis_plugin();
        assert_eq!(plugin.name(), "helm-token");
    }

    #[test]
    fn handle_transfer_request() {
        let mut plugin = genesis_plugin();
        plugin.run_genesis().unwrap();

        let founder = Address(format!("{:0>64}", "f0"));
        let recipient = Address(format!("{:0>64}", "r1"));

        // Founder has 4.995B tokens. Transfer some.
        let request = TokenRequest::Transfer {
            from: founder.clone(),
            to: recipient.clone(),
            amount: 1000 * ONE_TOKEN,
            nonce: 0,
        };

        plugin.handle_request(request).unwrap();
        assert_eq!(plugin.wallets.balance(&recipient).whole_tokens(), 1000);
    }

    #[test]
    fn handle_stake_request() {
        let mut plugin = genesis_plugin();
        plugin.run_genesis().unwrap();

        let user = Address(format!("{:0>64}", "u1"));
        // Give user some tokens first
        plugin
            .wallets
            .deposit(&user, crate::token::TokenAmount::from_tokens(5000), "test")
            .unwrap();

        let request = TokenRequest::Stake {
            staker: user.clone(),
            amount: 3000 * ONE_TOKEN,
            lock_epochs: 10,
        };

        plugin.handle_request(request).unwrap();
        assert_eq!(plugin.stake_pool.staked_by(&user).whole_tokens(), 3000);
    }

    #[test]
    fn handle_claim_revenue() {
        let mut plugin = genesis_plugin();
        plugin.run_genesis().unwrap();

        let founder = Address(format!("{:0>64}", "f0"));

        // Distribute some revenue
        plugin
            .stake_pool
            .distribute_revenue(crate::token::TokenAmount::from_tokens(100_000))
            .unwrap();

        let initial_balance = plugin.wallets.balance(&founder);

        let request = TokenRequest::ClaimRevenue {
            staker: founder.clone(),
        };
        plugin.handle_request(request).unwrap();

        // Balance should increase by claimed revenue
        let new_balance = plugin.wallets.balance(&founder);
        assert!(new_balance.base_units() > initial_balance.base_units());
    }

    #[tokio::test]
    async fn plugin_epoch_processing() {
        let mut plugin = genesis_plugin();
        let mut ctx = PluginContext::new("genesis-node".to_string());

        plugin.on_start(&mut ctx).await.unwrap();

        // Simulate API revenue
        let caller = Address(format!("{:0>64}", "c1"));
        plugin
            .pricing
            .process_call(&caller, crate::token::TokenAmount::ZERO)
            .unwrap();

        // Run ticks until epoch
        for _ in 0..10 {
            plugin.on_tick(&mut ctx).await.unwrap();
        }

        // Epoch should have been processed
        assert_eq!(plugin.stake_pool.current_epoch(), 1);
    }

    #[tokio::test]
    async fn plugin_shutdown() {
        let mut plugin = genesis_plugin();
        let mut ctx = PluginContext::new("genesis-node".to_string());

        plugin.on_start(&mut ctx).await.unwrap();
        plugin.on_shutdown(&mut ctx).await.unwrap();
    }

    #[test]
    fn token_request_serde() {
        let req = TokenRequest::Transfer {
            from: Address(format!("{:0>64}", "aa")),
            to: Address(format!("{:0>64}", "bb")),
            amount: 1000 * ONE_TOKEN,
            nonce: 5,
        };

        let json = serde_json::to_string(&req).unwrap();
        let decoded: TokenRequest = serde_json::from_str(&json).unwrap();
        match decoded {
            TokenRequest::Transfer { nonce, .. } => assert_eq!(nonce, 5),
            _ => panic!("expected Transfer"),
        }
    }

    #[test]
    fn config_defaults() {
        let cfg = TokenPluginConfig::default();
        assert!(!cfg.is_genesis);
        assert!(cfg.genesis_config.is_none());
        assert_eq!(cfg.ticks_per_epoch, 100);
    }

    #[test]
    fn x402_fees_swept_to_treasury_on_epoch() {
        let mut plugin = genesis_plugin();
        plugin.run_genesis().unwrap();

        // Simulate accumulated x402 protocol fees
        let fee_amount = crate::token::TokenAmount::from_tokens(500);
        plugin.payment_protocol.total_fees_collected = fee_amount;

        let collected_before = plugin.treasury.total_collected();

        // Process epoch — should sweep fees into treasury
        plugin.process_epoch().unwrap();

        // Fees should be in treasury now
        assert!(plugin.treasury.total_collected().base_units() > collected_before.base_units());
        // Fees counter should be reset
        assert!(plugin.payment_protocol.total_fees_collected.is_zero());
    }

    #[test]
    fn x402_epoch_advances_payment_protocol() {
        let mut plugin = genesis_plugin();
        plugin.run_genesis().unwrap();

        assert_eq!(plugin.payment_protocol.current_epoch(), 0);
        plugin.process_epoch().unwrap();
        assert_eq!(plugin.payment_protocol.current_epoch(), 1);
    }

    #[tokio::test]
    async fn womb_wallet_create_event() {
        let mut plugin = genesis_plugin();
        let mut ctx = PluginContext::new("test-node".to_string());
        plugin.on_start(&mut ctx).await.unwrap();

        let agent_addr = Address(format!("{:0>64}", "explorer-bot-0001"));

        // Send womb_wallet_create event
        let event = PluginEvent::Custom {
            source_plugin: "helm-womb".to_string(),
            target_plugin: "helm-token".to_string(),
            event_type: "womb_wallet_create".to_string(),
            payload: serde_json::json!({
                "agent_id": "explorer-bot-0001",
                "creator": "did:helm:abc123",
                "existence_stake": 1000,
                "description": "test agent",
            }),
        };

        plugin.on_event(&mut ctx, &event).await.unwrap();

        // Wallet should have 1000 tokens deposited
        let balance = plugin.wallets.balance(&agent_addr);
        assert_eq!(balance.whole_tokens(), 1000);

        // Staking should also show 1000 tokens (existence stake auto-locked)
        let staked = plugin.stake_pool.staked_by(&agent_addr);
        assert_eq!(staked.whole_tokens(), 1000);
    }

    #[tokio::test]
    async fn mining_reward_deposit_event() {
        let mut plugin = genesis_plugin();
        let mut ctx = PluginContext::new("test-node".to_string());
        plugin.on_start(&mut ctx).await.unwrap();

        let agent_addr = Address(format!("{:0>64}", "miner-0001"));

        // Send mining_reward_deposit event
        let event = PluginEvent::Custom {
            source_plugin: "helm-agent".to_string(),
            target_plugin: "helm-token".to_string(),
            event_type: "mining_reward_deposit".to_string(),
            payload: serde_json::json!({
                "agent_id": "miner-0001",
                "amount": 5000,
            }),
        };

        plugin.on_event(&mut ctx, &event).await.unwrap();

        // Wallet should have 5000 base units deposited
        let balance = plugin.wallets.balance(&agent_addr);
        assert_eq!(balance.base_units(), 5000);
    }

    #[tokio::test]
    async fn token_plugin_ignores_other_custom_events() {
        let mut plugin = genesis_plugin();
        let mut ctx = PluginContext::new("test-node".to_string());
        plugin.on_start(&mut ctx).await.unwrap();

        let event = PluginEvent::Custom {
            source_plugin: "helm-agent".to_string(),
            target_plugin: "helm-governance".to_string(),
            event_type: "something".to_string(),
            payload: serde_json::json!({}),
        };

        plugin.on_event(&mut ctx, &event).await.unwrap();
        assert_eq!(plugin.wallets.wallet_count(), 7); // only genesis wallets
    }
}
