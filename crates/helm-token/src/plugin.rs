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

use helm_core::{Plugin, PluginContext};
use helm_net::protocol::HelmMessage;

use crate::cabinet::Cabinet;
use crate::genesis::{execute_genesis, GenesisConfig};
use crate::pricing::DynamicPricing;
use crate::staking::StakePool;
use crate::token::HelmToken;
use crate::treasury::{HelmTreasury, TreasuryBucket};
use crate::wallet::{Address, WalletStore};

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
    tick_count: u64,
}

impl TokenPlugin {
    pub fn new(config: TokenPluginConfig) -> Self {
        let pricing = DynamicPricing::new(config.base_api_price);
        Self {
            config,
            token: HelmToken::new(),
            wallets: WalletStore::new(),
            stake_pool: StakePool::new(),
            treasury: HelmTreasury::new(),
            cabinet: Cabinet::new(),
            pricing,
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

        // 2. Allocate treasury to buckets
        self.treasury
            .allocate()
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // 3. Distribute staking rewards from treasury bucket
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

        Ok(())
    }

    /// Handle a token request from a network message.
    fn handle_request(&mut self, request: TokenRequest) -> Result<()> {
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

    async fn on_start(&mut self, ctx: &PluginContext) -> Result<()> {
        info!(node = %ctx.node_name, "TokenPlugin starting");
        self.run_genesis()?;
        Ok(())
    }

    async fn on_message(&mut self, _ctx: &PluginContext, msg: &HelmMessage) -> Result<()> {
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

    async fn on_tick(&mut self, _ctx: &PluginContext) -> Result<()> {
        self.tick_count += 1;

        if self.tick_count.is_multiple_of(self.config.ticks_per_epoch) {
            if let Err(e) = self.process_epoch() {
                warn!(error = %e, "Epoch processing failed");
            }
        }

        Ok(())
    }

    async fn on_shutdown(&mut self, _ctx: &PluginContext) -> Result<()> {
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
        let ctx = PluginContext {
            node_name: "genesis-node".to_string(),
        };

        plugin.on_start(&ctx).await.unwrap();

        assert!(plugin.token.is_genesis_done());
        assert_eq!(plugin.token.minted().whole_tokens(), TOTAL_SUPPLY);
    }

    #[tokio::test]
    async fn plugin_non_genesis_skips() {
        let mut plugin = TokenPlugin::new(TokenPluginConfig::default());
        let ctx = PluginContext {
            node_name: "regular-node".to_string(),
        };

        plugin.on_start(&ctx).await.unwrap();
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
        let ctx = PluginContext {
            node_name: "genesis-node".to_string(),
        };

        plugin.on_start(&ctx).await.unwrap();

        // Simulate API revenue
        let caller = Address(format!("{:0>64}", "c1"));
        plugin
            .pricing
            .process_call(&caller, crate::token::TokenAmount::ZERO)
            .unwrap();

        // Run ticks until epoch
        for _ in 0..10 {
            plugin.on_tick(&ctx).await.unwrap();
        }

        // Epoch should have been processed
        assert_eq!(plugin.stake_pool.current_epoch(), 1);
    }

    #[tokio::test]
    async fn plugin_shutdown() {
        let mut plugin = genesis_plugin();
        let ctx = PluginContext {
            node_name: "genesis-node".to_string(),
        };

        plugin.on_start(&ctx).await.unwrap();
        plugin.on_shutdown(&ctx).await.unwrap();
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
}
