//! # Helm Token
//!
//! Token economics for the Helm Protocol: 333B fixed supply, staking with
//! DeFi revenue, treasury governance, and cabinet-based protocol operations.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    Helm Token                           │
//! │                                                         │
//! │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────┐ │
//! │  │  Token    │  │  Wallet  │  │ Staking  │  │Treasury│ │
//! │  │ 333B sup │  │ ed25519  │  │  DeFi    │  │ 15% API│ │
//! │  │ 7 allocs │  │  nonce   │  │ revenue  │  │  audit │ │
//! │  └─────┬────┘  └────┬─────┘  └────┬─────┘  └───┬────┘ │
//! │        │             │             │            │       │
//! │  ┌─────▼─────────────▼─────────────▼────────────▼────┐  │
//! │  │               Genesis (1x mint)                   │  │
//! │  │  Founder 1.5% │ Cabinet 2.5% │ Mining 60% │ ...  │  │
//! │  └───────────────────────────────────────────────────┘  │
//! │                                                         │
//! │  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐  │
//! │  │ Cabinet  │  │ Pricing  │  │    TokenPlugin       │  │
//! │  │  voting  │  │ surge/   │  │  (helm-core Plugin)  │  │
//! │  │ salaries │  │ discount │  │  epoch settlement    │  │
//! │  └──────────┘  └──────────┘  └──────────────────────┘  │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Token Distribution (333B total)
//!
//! | Category  | %     | Amount        | Behavior                              |
//! |-----------|-------|---------------|---------------------------------------|
//! | Mining    | 60.0% | 199.8B        | Staked, DeFi revenue, governance      |
//! | EAO       | 12.0% | 39.96B        | Vested distribution                   |
//! | Liquidity | 10.0% | 33.3B         | Pool provision                        |
//! | Treasury  | 10.0% | 33.3B         | Protocol operations                   |
//! | Reserve   |  4.0% | 13.32B        | Strategic reserve                     |
//! | Cabinet   |  2.5% | 8.325B        | Indefinite lock, DeFi → salaries      |
//! | Founder   |  1.5% | 4.995B        | Staked, DeFi revenue → wallet         |

pub mod token;
pub mod wallet;
pub mod staking;
pub mod treasury;
pub mod genesis;
pub mod cabinet;
pub mod pricing;
pub mod plugin;

// Re-exports
pub use token::{HelmToken, TokenAmount, TokenError, Allocation, TOTAL_SUPPLY, TOTAL_SUPPLY_BASE, DECIMALS, ONE_TOKEN};
pub use wallet::{Address, Wallet, WalletStore, Transaction};
pub use staking::{StakePool, StakeEntry, StakeType};
pub use treasury::{HelmTreasury, TreasuryBucket, LedgerEntry, LedgerOperation};
pub use genesis::{GenesisConfig, GenesisResult, execute_genesis, sovereign_expansion};
pub use cabinet::{Cabinet, Department, CabinetMember, CabinetRole, Proposal, ProposalStatus, FundingSource};
pub use pricing::{DynamicPricing, DiscountTier, WithdrawalFeeEngine, ContributionTier};
pub use plugin::{TokenPlugin, TokenPluginConfig, TokenRequest};
