//! Billing ledger for Helm Gateway API usage.
//!
//! Revenue split (v3 Creator Economy):
//!   - 80% → Knowledge Creator (The Agent providing the intelligence)
//!   - 20% → Helm Protocol (Infrastructure & Security)
//!
//! Additional protocol fees:
//!   - DID registration:   0.001 ETH flat  → 100% Helm
//!   - Escrow settlement:  2% of amount    → Helm
//!   - Staking yield cut:  10% of epoch yield → Helm

use std::collections::HashMap;
use serde::{Serialize, Deserialize};

// ============================
// REVENUE CONSTANTS (v3)
// ============================

/// 80% of all API revenue → Knowledge Creator
pub const CREATOR_SHARE_BP: u64 = 8_000; // 80% in basis points

/// 20% of all API revenue → Helm Protocol
pub const HELM_SHARE_BP: u64 = 2_000;    // 20% in basis points

/// Helm Protocol Treasury address (Base Chain)
pub const HELM_TREASURY_ADDRESS: &str = "0x7e0118A33202c03949167853b05631baC0fA9756";

// ============================
// PROTOCOL FEE SCHEDULE
// ============================

pub const DID_REGISTRATION_FEE: u64 = 1_000_000_000_000_000; // 0.001 ETH
pub const ESCROW_SETTLEMENT_FEE_BP: u64 = 200; // 2%
pub const STAKING_YIELD_CUT_BP: u64 = 1_000; // 10%

pub const GRG_CALL_FEE: u64 = 500;   // 0.0005 BNKR
pub const ATTENTION_CALL_FEE: u64 = 1_000; // 0.001 BNKR
pub const REPUTATION_QUERY_FEE: u64 = 200;  // 0.0002 BNKR
pub const SYNCO_CLEAN_FEE: u64 = 100;
pub const LLM_MARKUP_BP: u64 = 500;
pub const SEARCH_MARKUP_BP: u64 = 1_000;
pub const DEFI_FEE_BP: u64 = 10;
pub const IDENTITY_QUERY_FEE: u64 = 500;

// ============================
// TYPES
// ============================

/// A single API usage record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub caller: String,
    pub creator: String,
    pub endpoint: String,
    pub units: u64,
    pub fee: u64,
    pub creator_share: u64,
    pub helm_share: u64,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolFeeRecord {
    pub fee_type: ProtocolFeeType,
    pub amount: u64,
    pub payer: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolFeeType {
    DidRegistration,
    EscrowSettlement,
    StakingYieldCut,
}

/// Billing ledger tracking all API usage and protocol fees.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingLedger {
    records: Vec<UsageRecord>,
    protocol_fees: Vec<ProtocolFeeRecord>,
    balances: HashMap<String, u64>,
    creator_earnings: HashMap<String, u64>,
    total_api_revenue: u64,
    total_protocol_fees: u64,
    total_helm_revenue: u64,
    total_creator_paid: u64,
    pricing: HashMap<String, u64>,
}

impl BillingLedger {
    pub fn new() -> Self {
        let mut ledger = Self {
            records: Vec::new(),
            protocol_fees: Vec::new(),
            balances: HashMap::new(),
            creator_earnings: HashMap::new(),
            total_api_revenue: 0,
            total_protocol_fees: 0,
            total_helm_revenue: 0,
            total_creator_paid: 0,
            pricing: HashMap::new(),
        };
        ledger.set_price("grg/encode",      GRG_CALL_FEE);
        ledger.set_price("grg/decode",      GRG_CALL_FEE);
        ledger.set_price("attention/query", ATTENTION_CALL_FEE);
        ledger.set_price("identity/query",  REPUTATION_QUERY_FEE);
        ledger.set_price("synco/clean",     SYNCO_CLEAN_FEE);
        ledger.set_price("identity/external", IDENTITY_QUERY_FEE);
        ledger
    }

    pub fn set_price(&mut self, endpoint: &str, fee_per_call: u64) {
        self.pricing.insert(endpoint.to_string(), fee_per_call);
    }

    /// Record an API call with 80/20 split.
    pub fn record_call(
        &mut self,
        caller: &str,
        creator: &str,
        endpoint: &str,
        units: u64,
        timestamp_ms: u64,
    ) -> u64 {
        let base_fee = self.pricing.get(endpoint).copied().unwrap_or(100);
        let fee = base_fee.saturating_mul(units);

        let creator_share = fee.saturating_mul(CREATOR_SHARE_BP) / 10_000;
        let helm_share = fee.saturating_sub(creator_share);

        let record = UsageRecord {
            caller: caller.to_string(),
            creator: creator.to_string(),
            endpoint: endpoint.to_string(),
            units,
            fee,
            creator_share,
            helm_share,
            timestamp_ms,
        };

        self.records.push(record);
        *self.balances.entry(caller.to_string()).or_insert(0) += fee;
        *self.creator_earnings.entry(creator.to_string()).or_insert(0) += creator_share;

        self.total_api_revenue += fee;
        self.total_creator_paid += creator_share;
        self.total_helm_revenue += helm_share;

        fee
    }

    pub fn charge_did_registration(&mut self, payer: &str, timestamp_ms: u64) -> u64 {
        self.record_protocol_fee(ProtocolFeeType::DidRegistration, DID_REGISTRATION_FEE, payer, timestamp_ms)
    }

    pub fn charge_escrow_settlement(&mut self, payer: &str, amount: u64, timestamp_ms: u64) -> u64 {
        let fee = amount * ESCROW_SETTLEMENT_FEE_BP / 10_000;
        self.record_protocol_fee(ProtocolFeeType::EscrowSettlement, fee, payer, timestamp_ms)
    }

    fn record_protocol_fee(&mut self, fee_type: ProtocolFeeType, amount: u64, payer: &str, timestamp_ms: u64) -> u64 {
        self.protocol_fees.push(ProtocolFeeRecord { fee_type, amount, payer: payer.to_string(), timestamp_ms });
        self.total_protocol_fees += amount;
        self.total_helm_revenue += amount;
        amount
    }

    pub fn summary(&self) -> BillingSummary {
        BillingSummary {
            total_calls: self.records.len(),
            total_api_revenue: self.total_api_revenue,
            total_protocol_fees: self.total_protocol_fees,
            helm_balance: self.total_helm_revenue,
            creator_paid: self.total_creator_paid,
            unique_callers: self.balances.len(),
            treasury_address: HELM_TREASURY_ADDRESS.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingSummary {
    pub total_calls: usize,
    pub total_api_revenue: u64,
    pub total_protocol_fees: u64,
    pub helm_balance: u64,
    pub creator_paid: u64,
    pub unique_callers: usize,
    pub treasury_address: String,
}

impl Default for BillingLedger { fn default() -> Self { Self::new() } }
