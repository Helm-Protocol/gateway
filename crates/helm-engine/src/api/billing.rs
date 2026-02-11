//! Billing ledger for Helm Engine API usage.
//!
//! Tracks API calls, computes fees, and ensures 15% of all revenue
//! flows back to the Helm Protocol treasury.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};

/// Revenue split: 15% to Helm treasury, 85% to node operator.
pub const HELM_REVENUE_SHARE: f64 = 0.15;

/// A single API usage record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Caller identity (agent ID, protocol address, etc.)
    pub caller: String,
    /// API endpoint called
    pub endpoint: String,
    /// Tokens/units consumed
    pub units: u64,
    /// Fee charged (in smallest denomination)
    pub fee: u64,
    /// Timestamp (unix millis)
    pub timestamp_ms: u64,
}

/// Billing ledger tracking all API usage and revenue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingLedger {
    /// All usage records
    records: Vec<UsageRecord>,
    /// Per-caller cumulative spend
    balances: HashMap<String, u64>,
    /// Total revenue collected
    total_revenue: u64,
    /// Revenue allocated to Helm treasury (15%)
    helm_treasury: u64,
    /// Revenue allocated to node operator (85%)
    operator_revenue: u64,
    /// Per-endpoint pricing (units → fee)
    pricing: HashMap<String, u64>,
}

impl BillingLedger {
    /// Create a new billing ledger.
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            balances: HashMap::new(),
            total_revenue: 0,
            helm_treasury: 0,
            operator_revenue: 0,
            pricing: HashMap::new(),
        }
    }

    /// Set price for an API endpoint (fee per call).
    pub fn set_price(&mut self, endpoint: &str, fee_per_call: u64) {
        self.pricing.insert(endpoint.to_string(), fee_per_call);
    }

    /// Record an API call and compute fees.
    /// Returns the fee charged.
    pub fn record_call(
        &mut self,
        caller: &str,
        endpoint: &str,
        units: u64,
        timestamp_ms: u64,
    ) -> u64 {
        let base_fee = self.pricing.get(endpoint).copied().unwrap_or(1);
        let fee = base_fee * units;

        let record = UsageRecord {
            caller: caller.to_string(),
            endpoint: endpoint.to_string(),
            units,
            fee,
            timestamp_ms,
        };

        self.records.push(record);

        // Update balances
        *self.balances.entry(caller.to_string()).or_insert(0) += fee;

        // Split revenue: 15% Helm, 85% operator
        let helm_share = (fee as f64 * HELM_REVENUE_SHARE).ceil() as u64;
        let operator_share = fee - helm_share;

        self.total_revenue += fee;
        self.helm_treasury += helm_share;
        self.operator_revenue += operator_share;

        fee
    }

    /// Get total revenue.
    pub fn total_revenue(&self) -> u64 {
        self.total_revenue
    }

    /// Get Helm treasury balance (15% share).
    pub fn helm_treasury(&self) -> u64 {
        self.helm_treasury
    }

    /// Get node operator revenue (85% share).
    pub fn operator_revenue(&self) -> u64 {
        self.operator_revenue
    }

    /// Get a caller's total spend.
    pub fn caller_spend(&self, caller: &str) -> u64 {
        self.balances.get(caller).copied().unwrap_or(0)
    }

    /// Get all usage records.
    pub fn records(&self) -> &[UsageRecord] {
        &self.records
    }

    /// Get number of recorded calls.
    pub fn call_count(&self) -> usize {
        self.records.len()
    }

    /// Summary report.
    pub fn summary(&self) -> BillingSummary {
        BillingSummary {
            total_calls: self.records.len(),
            total_revenue: self.total_revenue,
            helm_treasury: self.helm_treasury,
            operator_revenue: self.operator_revenue,
            unique_callers: self.balances.len(),
        }
    }
}

impl Default for BillingLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of billing activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingSummary {
    pub total_calls: usize,
    pub total_revenue: u64,
    pub helm_treasury: u64,
    pub operator_revenue: u64,
    pub unique_callers: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revenue_split_15_85() {
        let mut ledger = BillingLedger::new();
        ledger.set_price("attention/query", 100);

        ledger.record_call("agent-1", "attention/query", 1, 1000);
        assert_eq!(ledger.total_revenue(), 100);
        assert_eq!(ledger.helm_treasury(), 15); // 15%
        assert_eq!(ledger.operator_revenue(), 85); // 85%
    }

    #[test]
    fn multiple_callers() {
        let mut ledger = BillingLedger::new();
        ledger.set_price("grg/encode", 10);
        ledger.set_price("grg/decode", 5);

        ledger.record_call("agent-1", "grg/encode", 3, 1000);
        ledger.record_call("agent-2", "grg/decode", 2, 2000);
        ledger.record_call("agent-1", "grg/encode", 1, 3000);

        assert_eq!(ledger.caller_spend("agent-1"), 40); // 30 + 10
        assert_eq!(ledger.caller_spend("agent-2"), 10);
        assert_eq!(ledger.call_count(), 3);
    }

    #[test]
    fn summary_report() {
        let mut ledger = BillingLedger::new();
        ledger.set_price("edge/accelerate", 50);

        for i in 0..10 {
            ledger.record_call(&format!("agent-{}", i % 3), "edge/accelerate", 1, i * 1000);
        }

        let summary = ledger.summary();
        assert_eq!(summary.total_calls, 10);
        assert_eq!(summary.total_revenue, 500);
        assert_eq!(summary.unique_callers, 3);
        // 15% of 500 = 75
        assert_eq!(summary.helm_treasury, 80); // ceil(50 * 0.15) = 8 per call, 8 * 10 = 80
    }

    #[test]
    fn default_pricing() {
        let mut ledger = BillingLedger::new();
        // No price set — defaults to 1 per unit
        let fee = ledger.record_call("agent", "unknown/endpoint", 5, 0);
        assert_eq!(fee, 5);
    }
}
