//! Genesis — initial token distribution at network launch.
//!
//! The Genesis node mints the initial allocations:
//! - 1.5% → Founder wallet (immediately staked, DeFi revenue to wallet)
//! - 2.5% → Cabinet pool (indefinite lockup, DeFi revenue for salaries)
//! - 10%  → Treasury
//! - 10%  → Liquidity pool
//! - 4%   → Reserve
//! - 12%  → EAO (vested)
//! - 60%  → Mining pool (staked, governance-directed)

use serde::{Deserialize, Serialize};

use crate::staking::{StakePool, StakeType};
use crate::token::{Allocation, HelmToken, TokenAmount, TokenError};
use crate::treasury::HelmTreasury;
use crate::wallet::{Address, WalletStore};

/// Genesis configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    /// Founder's wallet address (receives 1.5% allocation + DeFi revenue).
    pub founder_address: Address,
    /// Cabinet pool address (receives 2.5% indefinite lockup).
    pub cabinet_address: Address,
    /// Treasury address.
    pub treasury_address: Address,
    /// Liquidity pool address.
    pub liquidity_address: Address,
    /// Reserve address.
    pub reserve_address: Address,
    /// EAO address.
    pub eao_address: Address,
    /// Mining pool address.
    pub mining_address: Address,
}

/// Result of genesis initialization.
#[derive(Debug)]
pub struct GenesisResult {
    pub founder_allocation: TokenAmount,
    pub cabinet_allocation: TokenAmount,
    pub treasury_allocation: TokenAmount,
    pub liquidity_allocation: TokenAmount,
    pub reserve_allocation: TokenAmount,
    pub eao_allocation: TokenAmount,
    pub mining_allocation: TokenAmount,
    pub total_minted: TokenAmount,
}

/// Execute genesis: mint all allocations, stake founder + cabinet + mining.
pub fn execute_genesis(
    config: &GenesisConfig,
    token: &mut HelmToken,
    wallets: &mut WalletStore,
    stake_pool: &mut StakePool,
    treasury: &mut HelmTreasury,
) -> Result<GenesisResult, TokenError> {
    if token.is_genesis_done() {
        return Err(TokenError::GenesisAlreadyDone);
    }

    // 1. Founder: 1.5% → wallet → immediately staked (Founder type)
    let founder_amt = TokenAmount::from_base(Allocation::Founder.amount());
    token.mint(Allocation::Founder, founder_amt)?;
    wallets.deposit(&config.founder_address, founder_amt, "genesis: founder allocation")?;
    stake_pool.stake(
        &config.founder_address,
        founder_amt,
        StakeType::Founder,
        0, // indefinite
    )?;

    // 2. Cabinet: 2.5% → cabinet address → indefinite lockup staking
    let cabinet_amt = TokenAmount::from_base(Allocation::Cabinet.amount());
    token.mint(Allocation::Cabinet, cabinet_amt)?;
    wallets.deposit(&config.cabinet_address, cabinet_amt, "genesis: cabinet allocation")?;
    stake_pool.stake(
        &config.cabinet_address,
        cabinet_amt,
        StakeType::Cabinet,
        0, // indefinite
    )?;

    // 3. Treasury: 10% → treasury address
    let treasury_amt = TokenAmount::from_base(Allocation::Treasury.amount());
    token.mint(Allocation::Treasury, treasury_amt)?;
    wallets.deposit(&config.treasury_address, treasury_amt, "genesis: treasury allocation")?;
    treasury.collect_revenue(treasury_amt, "genesis: initial treasury allocation")?;

    // 4. Liquidity: 10% → liquidity address
    let liquidity_amt = TokenAmount::from_base(Allocation::Liquidity.amount());
    token.mint(Allocation::Liquidity, liquidity_amt)?;
    wallets.deposit(&config.liquidity_address, liquidity_amt, "genesis: liquidity allocation")?;

    // 5. Reserve: 4% → reserve address
    let reserve_amt = TokenAmount::from_base(Allocation::Reserve.amount());
    token.mint(Allocation::Reserve, reserve_amt)?;
    wallets.deposit(&config.reserve_address, reserve_amt, "genesis: reserve allocation")?;

    // 6. EAO: 12% → EAO address (vesting managed externally)
    let eao_amt = TokenAmount::from_base(Allocation::Eao.amount());
    token.mint(Allocation::Eao, eao_amt)?;
    wallets.deposit(&config.eao_address, eao_amt, "genesis: EAO allocation")?;

    // 7. Mining: 60% → mining pool address → staked (Mining type)
    let mining_amt = TokenAmount::from_base(Allocation::Mining.amount());
    token.mint(Allocation::Mining, mining_amt)?;
    wallets.deposit(&config.mining_address, mining_amt, "genesis: mining pool allocation")?;
    stake_pool.stake(
        &config.mining_address,
        mining_amt,
        StakeType::Mining,
        0, // governance-directed
    )?;

    token.mark_genesis_done();

    Ok(GenesisResult {
        founder_allocation: founder_amt,
        cabinet_allocation: cabinet_amt,
        treasury_allocation: treasury_amt,
        liquidity_allocation: liquidity_amt,
        reserve_allocation: reserve_amt,
        eao_allocation: eao_amt,
        mining_allocation: mining_amt,
        total_minted: token.minted(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::{TOTAL_SUPPLY, TOTAL_SUPPLY_BASE, ONE_TOKEN};

    fn test_config() -> GenesisConfig {
        GenesisConfig {
            founder_address: Address(format!("{:0>64}", "f0")),
            cabinet_address: Address(format!("{:0>64}", "ca")),
            treasury_address: Address(format!("{:0>64}", "tr")),
            liquidity_address: Address(format!("{:0>64}", "lq")),
            reserve_address: Address(format!("{:0>64}", "rs")),
            eao_address: Address(format!("{:0>64}", "ea")),
            mining_address: Address(format!("{:0>64}", "mn")),
        }
    }

    #[test]
    fn genesis_mints_full_supply() {
        let config = test_config();
        let mut token = HelmToken::new();
        let mut wallets = WalletStore::new();
        let mut stake_pool = StakePool::new();
        let mut treasury = HelmTreasury::new();

        let result =
            execute_genesis(&config, &mut token, &mut wallets, &mut stake_pool, &mut treasury)
                .unwrap();

        assert_eq!(result.total_minted.base_units(), TOTAL_SUPPLY_BASE);
        assert_eq!(result.total_minted.whole_tokens(), TOTAL_SUPPLY);
    }

    #[test]
    fn genesis_founder_gets_1_5_percent() {
        let config = test_config();
        let mut token = HelmToken::new();
        let mut wallets = WalletStore::new();
        let mut stake_pool = StakePool::new();
        let mut treasury = HelmTreasury::new();

        let result =
            execute_genesis(&config, &mut token, &mut wallets, &mut stake_pool, &mut treasury)
                .unwrap();

        assert_eq!(result.founder_allocation.whole_tokens(), 4_995_000_000);

        // Founder wallet has the balance
        assert_eq!(
            wallets.balance(&config.founder_address).whole_tokens(),
            4_995_000_000
        );

        // Founder tokens are staked
        assert_eq!(
            stake_pool.staked_by(&config.founder_address).whole_tokens(),
            4_995_000_000
        );
    }

    #[test]
    fn genesis_cabinet_gets_2_5_percent_indefinite_lock() {
        let config = test_config();
        let mut token = HelmToken::new();
        let mut wallets = WalletStore::new();
        let mut stake_pool = StakePool::new();
        let mut treasury = HelmTreasury::new();

        execute_genesis(&config, &mut token, &mut wallets, &mut stake_pool, &mut treasury)
            .unwrap();

        assert_eq!(
            wallets.balance(&config.cabinet_address).whole_tokens(),
            8_325_000_000
        );

        // Cabinet stake cannot be unstaked
        assert!(stake_pool.unstake(&config.cabinet_address, 0).is_err());
    }

    #[test]
    fn genesis_mining_pool_staked() {
        let config = test_config();
        let mut token = HelmToken::new();
        let mut wallets = WalletStore::new();
        let mut stake_pool = StakePool::new();
        let mut treasury = HelmTreasury::new();

        execute_genesis(&config, &mut token, &mut wallets, &mut stake_pool, &mut treasury)
            .unwrap();

        assert_eq!(
            stake_pool.staked_by(&config.mining_address).whole_tokens(),
            199_800_000_000
        );
    }

    #[test]
    fn genesis_cannot_run_twice() {
        let config = test_config();
        let mut token = HelmToken::new();
        let mut wallets = WalletStore::new();
        let mut stake_pool = StakePool::new();
        let mut treasury = HelmTreasury::new();

        execute_genesis(&config, &mut token, &mut wallets, &mut stake_pool, &mut treasury)
            .unwrap();

        // Second genesis fails
        assert!(
            execute_genesis(&config, &mut token, &mut wallets, &mut stake_pool, &mut treasury)
                .is_err()
        );
    }

    #[test]
    fn genesis_founder_earns_defi_revenue() {
        let config = test_config();
        let mut token = HelmToken::new();
        let mut wallets = WalletStore::new();
        let mut stake_pool = StakePool::new();
        let mut treasury = HelmTreasury::new();

        execute_genesis(&config, &mut token, &mut wallets, &mut stake_pool, &mut treasury)
            .unwrap();

        // Simulate DeFi revenue
        let revenue = TokenAmount::from_tokens(1_000_000);
        let dist = stake_pool.distribute_revenue(revenue).unwrap();

        // Founder gets proportional share of revenue
        let founder_revenue = dist.get(&config.founder_address).unwrap();
        assert!(founder_revenue.base_units() > 0);

        // Founder can claim and move to wallet
        let claimed = stake_pool.claim_revenue(&config.founder_address).unwrap();
        assert_eq!(claimed.base_units(), founder_revenue.base_units());
    }

    #[test]
    fn genesis_cabinet_defi_for_salaries() {
        let config = test_config();
        let mut token = HelmToken::new();
        let mut wallets = WalletStore::new();
        let mut stake_pool = StakePool::new();
        let mut treasury = HelmTreasury::new();

        execute_genesis(&config, &mut token, &mut wallets, &mut stake_pool, &mut treasury)
            .unwrap();

        // Simulate DeFi revenue
        stake_pool
            .distribute_revenue(TokenAmount::from_tokens(1_000_000))
            .unwrap();

        // Cabinet can claim DeFi revenue for salaries
        let salary_fund = stake_pool.claim_revenue(&config.cabinet_address).unwrap();
        assert!(salary_fund.base_units() > 0);
    }

    #[test]
    fn genesis_treasury_initial_balance() {
        let config = test_config();
        let mut token = HelmToken::new();
        let mut wallets = WalletStore::new();
        let mut stake_pool = StakePool::new();
        let mut treasury = HelmTreasury::new();

        execute_genesis(&config, &mut token, &mut wallets, &mut stake_pool, &mut treasury)
            .unwrap();

        // Treasury got 10% = 33.3B
        assert_eq!(treasury.total_collected().whole_tokens(), 33_300_000_000);
    }

    #[test]
    fn genesis_all_allocations_accounted() {
        let config = test_config();
        let mut token = HelmToken::new();
        let mut wallets = WalletStore::new();
        let mut stake_pool = StakePool::new();
        let mut treasury = HelmTreasury::new();

        let result =
            execute_genesis(&config, &mut token, &mut wallets, &mut stake_pool, &mut treasury)
                .unwrap();

        let sum = result.founder_allocation.base_units()
            + result.cabinet_allocation.base_units()
            + result.treasury_allocation.base_units()
            + result.liquidity_allocation.base_units()
            + result.reserve_allocation.base_units()
            + result.eao_allocation.base_units()
            + result.mining_allocation.base_units();

        assert_eq!(sum, TOTAL_SUPPLY_BASE);
        assert_eq!(sum / ONE_TOKEN, TOTAL_SUPPLY);
    }
}
