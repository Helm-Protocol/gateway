//! Billing ledger for Helm Gateway API usage.
//!
//! Revenue split (corrected):
//!   - 85% → Helm Protocol Treasury (Jay's wallet)
//!   - 15% → Referring agent (the agent that brought the caller)
//!
//! Additional protocol fees:
//!   - DID registration:   0.001 ETH flat  → 100% treasury
//!   - Escrow settlement:  2% of amount    → treasury
//!   - Staking yield cut:  10% of epoch yield → treasury
//!   - GRG encode/decode:  0.0005 ETH/call → 85/15 split
//!   - QKV-G attention:    0.001 ETH/call  → 85/15 split
//!   - Reputation query:   0.0002 ETH/call → 85/15 split

use std::collections::HashMap;
use serde::{Serialize, Deserialize};

// ============================
// REVENUE CONSTANTS
// ============================

/// 85% of all API revenue → Helm Protocol Treasury (Jay's wallet)
pub const TREASURY_SHARE: f64 = 0.85;

/// 15% of all API revenue → referring agent (the one that onboarded the caller)
pub const REFERRER_SHARE: f64 = 0.15;

/// Helm Protocol Treasury address (Base Chain)
pub const HELM_TREASURY_ADDRESS: &str = "0x7e0118A33202c03949167853b05631baC0fA9756";

// ============================
// PROTOCOL FEE SCHEDULE
// ============================

/// DID registration: flat fee in BNKR base units (0.001 ETH equivalent)
pub const DID_REGISTRATION_FEE: u64 = 1_000_000_000_000_000; // 0.001 ETH in wei

/// Escrow settlement fee: 2% of total settled amount
pub const ESCROW_SETTLEMENT_FEE_BP: u64 = 200; // basis points (2%)

/// Staking yield protocol cut: 10% of epoch yield
pub const STAKING_YIELD_CUT_BP: u64 = 1_000; // basis points (10%)

/// GRG encode/decode fee per call (in BNKR micro-units)
pub const GRG_CALL_FEE: u64 = 500;   // 0.0005 BNKR

/// QKV-G attention query fee per call
pub const ATTENTION_CALL_FEE: u64 = 1_000; // 0.001 BNKR

/// Reputation query fee per call
pub const REPUTATION_QUERY_FEE: u64 = 200;  // 0.0002 BNKR

/// Sync-O stream clean fee per 1000 items
pub const SYNCO_CLEAN_FEE: u64 = 100;  // 0.0001 BNKR per 1000 items

/// A-Front (LLM proxy) markup over cost: 5%
pub const LLM_MARKUP_BP: u64 = 500;

/// B-Front (Search proxy) markup over cost: 10%
pub const SEARCH_MARKUP_BP: u64 = 1_000;

/// C-Front (DeFi oracle proxy) fee: 0.1% of swap value
pub const DEFI_FEE_BP: u64 = 10;

/// D-Front (Identity/Reputation) fee per external query
pub const IDENTITY_QUERY_FEE: u64 = 500;  // 0.0005 BNKR

// ============================
// TYPES
// ============================

/// A single API usage record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Caller identity (agent DID)
    pub caller: String,
    /// Referring agent DID (earns 15%)
    pub referrer: Option<String>,
    /// API endpoint called
    pub endpoint: String,
    /// Units consumed
    pub units: u64,
    /// Fee charged (in BNKR micro-units)
    pub fee: u64,
    /// Treasury share (85%)
    pub treasury_share: u64,
    /// Referrer share (15%)
    pub referrer_share: u64,
    /// Timestamp (unix millis)
    pub timestamp_ms: u64,
}

/// Protocol fee record (DID, escrow, staking — 100% to treasury).
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
    /// Per-caller cumulative spend
    balances: HashMap<String, u64>,
    /// Per-referrer cumulative earnings
    referrer_earnings: HashMap<String, u64>,
    /// Total API revenue collected
    total_api_revenue: u64,
    /// Total protocol fees collected
    total_protocol_fees: u64,
    /// Total flowing to treasury (85% API + 100% protocol fees)
    total_treasury: u64,
    /// Total paid to referrers (15% API)
    total_referrer_paid: u64,
    /// Per-endpoint pricing (units → fee)
    pricing: HashMap<String, u64>,
}

impl BillingLedger {
    pub fn new() -> Self {
        let mut ledger = Self {
            records: Vec::new(),
            protocol_fees: Vec::new(),
            balances: HashMap::new(),
            referrer_earnings: HashMap::new(),
            total_api_revenue: 0,
            total_protocol_fees: 0,
            total_treasury: 0,
            total_referrer_paid: 0,
            pricing: HashMap::new(),
        };
        // Pre-populate standard fees
        ledger.set_price("grg/encode",      GRG_CALL_FEE);
        ledger.set_price("grg/decode",      GRG_CALL_FEE);
        ledger.set_price("attention/query", ATTENTION_CALL_FEE);
        ledger.set_price("identity/query",  REPUTATION_QUERY_FEE);
        ledger.set_price("synco/clean",     SYNCO_CLEAN_FEE);
        ledger.set_price("identity/external", IDENTITY_QUERY_FEE);
        ledger
    }

    /// Set price for an endpoint (fee per call).
    pub fn set_price(&mut self, endpoint: &str, fee_per_call: u64) {
        self.pricing.insert(endpoint.to_string(), fee_per_call);
    }

    // ============================
    // API CALL BILLING (85/15 split)
    // ============================

    /// Record an API call.
    /// - 85% flows to treasury
    /// - 15% flows to referrer (if any)
    pub fn record_call(
        &mut self,
        caller: &str,
        referrer: Option<&str>,
        endpoint: &str,
        units: u64,
        timestamp_ms: u64,
    ) -> u64 {
        let base_fee = self.pricing.get(endpoint).copied().unwrap_or(100);
        let fee = base_fee.saturating_mul(units);

        // 85% treasury, 15% referrer
        let referrer_share = (fee as f64 * REFERRER_SHARE).floor() as u64;
        let treasury_share = fee - referrer_share;

        let record = UsageRecord {
            caller: caller.to_string(),
            referrer: referrer.map(|r| r.to_string()),
            endpoint: endpoint.to_string(),
            units,
            fee,
            treasury_share,
            referrer_share,
            timestamp_ms,
        };

        self.records.push(record);
        *self.balances.entry(caller.to_string()).or_insert(0) += fee;

        if let Some(ref_did) = referrer {
            *self.referrer_earnings.entry(ref_did.to_string()).or_insert(0) += referrer_share;
            self.total_referrer_paid += referrer_share;
        } else {
            // No referrer → 100% to treasury
            self.total_treasury += referrer_share;
        }

        self.total_api_revenue += fee;
        self.total_treasury += treasury_share;

        fee
    }

    // ============================
    // PROTOCOL FEES (100% treasury)
    // ============================

    /// Charge DID registration fee.
    pub fn charge_did_registration(&mut self, payer: &str, timestamp_ms: u64) -> u64 {
        self.record_protocol_fee(
            ProtocolFeeType::DidRegistration,
            DID_REGISTRATION_FEE,
            payer,
            timestamp_ms,
        )
    }

    /// Charge escrow settlement fee (2% of settled amount).
    pub fn charge_escrow_settlement(&mut self, payer: &str, amount: u64, timestamp_ms: u64) -> u64 {
        let fee = amount * ESCROW_SETTLEMENT_FEE_BP / 10_000;
        self.record_protocol_fee(
            ProtocolFeeType::EscrowSettlement,
            fee,
            payer,
            timestamp_ms,
        )
    }

    /// Charge staking yield cut (10% of epoch yield).
    pub fn charge_staking_yield_cut(&mut self, payer: &str, yield_amount: u64, timestamp_ms: u64) -> u64 {
        let fee = yield_amount * STAKING_YIELD_CUT_BP / 10_000;
        self.record_protocol_fee(
            ProtocolFeeType::StakingYieldCut,
            fee,
            payer,
            timestamp_ms,
        )
    }

    fn record_protocol_fee(
        &mut self,
        fee_type: ProtocolFeeType,
        amount: u64,
        payer: &str,
        timestamp_ms: u64,
    ) -> u64 {
        self.protocol_fees.push(ProtocolFeeRecord {
            fee_type,
            amount,
            payer: payer.to_string(),
            timestamp_ms,
        });
        // 100% protocol fees → treasury
        self.total_protocol_fees += amount;
        self.total_treasury += amount;
        amount
    }

    // ============================
    // QUERIES
    // ============================

    /// Total revenue flowing to treasury (85% API + 100% protocol fees).
    pub fn treasury_balance(&self) -> u64 { self.total_treasury }

    /// Total paid to referrers.
    pub fn total_referrer_paid(&self) -> u64 { self.total_referrer_paid }

    /// A referrer's total earnings.
    pub fn referrer_earnings(&self, did: &str) -> u64 {
        self.referrer_earnings.get(did).copied().unwrap_or(0)
    }

    pub fn total_api_revenue(&self) -> u64 { self.total_api_revenue }
    pub fn total_protocol_fees(&self) -> u64 { self.total_protocol_fees }
    pub fn caller_spend(&self, caller: &str) -> u64 {
        self.balances.get(caller).copied().unwrap_or(0)
    }
    pub fn call_count(&self) -> usize { self.records.len() }
    pub fn records(&self) -> &[UsageRecord] { &self.records }
    pub fn treasury_address(&self) -> &str { HELM_TREASURY_ADDRESS }

    pub fn summary(&self) -> BillingSummary {
        BillingSummary {
            total_calls: self.records.len(),
            total_api_revenue: self.total_api_revenue,
            total_protocol_fees: self.total_protocol_fees,
            treasury_balance: self.total_treasury,
            referrer_paid: self.total_referrer_paid,
            unique_callers: self.balances.len(),
            treasury_address: HELM_TREASURY_ADDRESS.to_string(),
        }
    }
}

impl Default for BillingLedger {
    fn default() -> Self { Self::new() }
}

/// Summary of billing activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingSummary {
    pub total_calls: usize,
    pub total_api_revenue: u64,
    pub total_protocol_fees: u64,
    /// 85% API + 100% protocol fees → treasury
    pub treasury_balance: u64,
    /// 15% API → referrer agents
    pub referrer_paid: u64,
    pub unique_callers: usize,
    pub treasury_address: String,
}

// ============================
// TESTS
// ============================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_revenue_split_85_15() {
        let mut ledger = BillingLedger::new();
        // GRG encode: 500 micro-BNKR per call
        let fee = ledger.record_call("agent-1", Some("referrer-did"), "grg/encode", 1, 1000);
        assert_eq!(fee, 500);
        // 85% treasury = 425, 15% referrer = 75
        assert_eq!(ledger.treasury_balance(), 425);
        assert_eq!(ledger.referrer_earnings("referrer-did"), 75);
        assert_eq!(ledger.total_referrer_paid(), 75);
    }

    #[test]
    fn no_referrer_100_percent_treasury() {
        let mut ledger = BillingLedger::new();
        ledger.record_call("agent-1", None, "attention/query", 1, 1000);
        // No referrer → full 1000 to treasury
        assert_eq!(ledger.treasury_balance(), 1000);
        assert_eq!(ledger.total_referrer_paid(), 0);
    }

    #[test]
    fn did_registration_fee_to_treasury() {
        let mut ledger = BillingLedger::new();
        let fee = ledger.charge_did_registration("new-agent", 1000);
        assert_eq!(fee, DID_REGISTRATION_FEE);
        assert_eq!(ledger.total_protocol_fees(), DID_REGISTRATION_FEE);
        assert_eq!(ledger.treasury_balance(), DID_REGISTRATION_FEE);
    }

    #[test]
    fn escrow_settlement_2_percent() {
        let mut ledger = BillingLedger::new();
        let fee = ledger.charge_escrow_settlement("agent-1", 100_000, 1000);
        assert_eq!(fee, 2_000); // 2% of 100_000
        assert_eq!(ledger.treasury_balance(), 2_000);
    }

    #[test]
    fn staking_yield_cut_10_percent() {
        let mut ledger = BillingLedger::new();
        let fee = ledger.charge_staking_yield_cut("staker", 50_000, 1000);
        assert_eq!(fee, 5_000); // 10% of 50_000
        assert_eq!(ledger.treasury_balance(), 5_000);
    }

    #[test]
    fn referrer_accumulates_earnings() {
        let mut ledger = BillingLedger::new();
        for _ in 0..10 {
            ledger.record_call("agent-X", Some("big-referrer"), "grg/encode", 1, 0);
        }
        // 10 calls × 500 fee × 15% = 750
        assert_eq!(ledger.referrer_earnings("big-referrer"), 750);
    }

    #[test]
    fn treasury_address_correct() {
        let ledger = BillingLedger::new();
        assert_eq!(ledger.treasury_address(), "0x7e0118A33202c03949167853b05631baC0fA9756");
    }

    #[test]
    fn summary_totals() {
        let mut ledger = BillingLedger::new();
        ledger.record_call("a1", Some("ref1"), "grg/encode", 2, 0);   // 1000 total
        ledger.charge_did_registration("a1", 0);
        ledger.charge_escrow_settlement("a1", 10_000, 0);              // 200 fee

        let s = ledger.summary();
        assert_eq!(s.total_calls, 1);
        // treasury: 850 (85% of 1000) + DID_FEE + 200
        assert_eq!(s.referrer_paid, 150); // 15% of 1000
    }
}
