//! Multi-token pricing engine for Helm Sense API.
//!
//! ## Token hierarchy (Feb 2026 prices)
//!
//! | Token   | USD price | Use case                    |
//! |---------|-----------|----------------------------|
//! | VIRTUAL | $0.65     | Package contracts, flagship |
//! | BNKR    | $0.00055  | Micro-calls, dev usage      |
//! | CLANKER | $32.00    | B2B protocol contracts      |
//! | USDC    | $1.00     | Enterprise, OpenAI/Anthropic|
//!
//! All internal accounting is in VIRTUAL micro-units (1 VIRTUAL = 1_000_000 μV).
//! Billing ledger converts to BNKR micro-units for protocol fees.
//!
//! ## What the strategy doc missed
//!
//! The doc listed prices in VIRTUAL but didn't provide an exchange rate
//! engine. Without this, the billing ledger (which uses BNKR micro-units)
//! can't handle multi-token payments correctly.

use serde::{Deserialize, Serialize};

/// 1 VIRTUAL in micro-units (μV)
pub const VIRTUAL_UNIT: u64 = 1_000_000;

/// Exchange rates to USD (multiply by this to get USD cents * 100)
pub const VIRTUAL_USD: f64 = 0.65;
pub const BNKR_USD: f64 = 0.00055;
pub const CLANKER_USD: f64 = 32.00;

/// Convert VIRTUAL micro-units → BNKR micro-units
/// 1 VIRTUAL = (0.65 / 0.00055) BNKR ≈ 1181.82 BNKR
pub fn virtual_to_bnkr(virtual_units: u64) -> u64 {
    let bnkr_per_virtual = VIRTUAL_USD / BNKR_USD; // ~1181.82
    (virtual_units as f64 * bnkr_per_virtual / VIRTUAL_UNIT as f64) as u64
}

/// Convert BNKR micro-units → VIRTUAL micro-units
pub fn bnkr_to_virtual(bnkr: u64) -> u64 {
    let virtual_per_bnkr = BNKR_USD / VIRTUAL_USD;
    (bnkr as f64 * virtual_per_bnkr * VIRTUAL_UNIT as f64) as u64
}

/// Convert USDC cents → VIRTUAL micro-units
pub fn usdc_to_virtual(usdc_cents: u64) -> u64 {
    let usdc = usdc_cents as f64 / 100.0;
    let virtual_amount = usdc / VIRTUAL_USD;
    (virtual_amount * VIRTUAL_UNIT as f64) as u64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentToken {
    #[serde(rename = "VIRTUAL")]
    Virtual,
    #[serde(rename = "BNKR")]
    Bnkr,
    #[serde(rename = "CLANKER")]
    Clanker,
    #[serde(rename = "USDC")]
    Usdc,
}

/// Accepted payment for an API call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payment {
    pub token: PaymentToken,
    /// Amount in the token's base unit (VIRTUAL μV, BNKR μ-BNKR, USDC cents, CLANKER μ-CLANKER)
    pub amount: u64,
}

impl Payment {
    /// Convert to VIRTUAL micro-units for unified billing.
    pub fn to_virtual_units(&self) -> u64 {
        match self.token {
            PaymentToken::Virtual => self.amount,
            PaymentToken::Bnkr => bnkr_to_virtual(self.amount),
            PaymentToken::Usdc => usdc_to_virtual(self.amount),
            PaymentToken::Clanker => {
                // 1 CLANKER = $32 / $0.65 ≈ 49.23 VIRTUAL
                let virtual_per_clanker = 32.0 / VIRTUAL_USD;
                (self.amount as f64 * virtual_per_clanker / VIRTUAL_UNIT as f64 * VIRTUAL_UNIT as f64) as u64
            }
        }
    }
}

/// Package pricing definitions.
pub mod packages {
    use super::VIRTUAL_UNIT;

    /// Alpha Hunt: 10 VIRTUAL/call
    pub const ALPHA_HUNT_PER_CALL: u64 = 10 * VIRTUAL_UNIT;
    /// Protocol Shield: $2/GB processed (in USDC cents = 200 cents → VIRTUAL)
    pub const PROTOCOL_SHIELD_PER_MB: u64 = 2_000; // 2 VIRTUAL (approx $1.30/MB)
    /// Trust Transaction: 2 VIRTUAL/query
    pub const TRUST_TRANSACTION_PER_QUERY: u64 = 2 * VIRTUAL_UNIT;
    /// Sovereign Agent: 500 VIRTUAL/month
    pub const SOVEREIGN_AGENT_MONTHLY: u64 = 500 * VIRTUAL_UNIT;
    /// Signal Channel: 50 VIRTUAL/month per subscriber
    pub const SIGNAL_CHANNEL_MONTHLY: u64 = 50 * VIRTUAL_UNIT;
    /// LLM Pool (GPT-4o input) markup: OpenAI costs $5/M tokens → Helm charges $3.5/M (after group discount)
    pub const LLM_GPT4O_INPUT_PER_M_TOKENS_USDC_CENTS: u64 = 350; // $3.50/M tokens

    /// Referrer cut: 15% of API revenue
    pub const REFERRER_BPS: u64 = 1500;
    /// Signal channel operator cut: 70% of subscription revenue
    pub const CHANNEL_OPERATOR_BPS: u64 = 7000;
    /// Helm cut from signal channel: 15%
    pub const HELM_CHANNEL_BPS: u64 = 1500;
}

/// Calculate G-metric zone pricing premium.
///
/// ## What the doc missed
///
/// The doc shows G zones as marketing tiers (0.00-0.20, 0.20-0.60, etc.)
/// but the code uses tanh-based G computation with a binary threshold at 0.4.
/// This function maps the actual G values to premium pricing tiers.
///
/// The actual G formula in code:
///   g_metric = 1.0 - max_score.tanh().max(0.0)
///   where max_score = max dot product across all KV blocks
///
/// G zones (calibrated to code reality):
///   G ∈ [0.0, 0.2] → "Known" → cache discount (0% premium)
///   G ∈ [0.2, 0.4] → "Uncertain" → 25% premium
///   G ∈ [0.4, 0.6] → "Gap" (code threshold hit) → 50% premium + Ghost Tokens
///   G ∈ [0.6, 0.8] → "Unknown" → 100% premium + auto-questions
///   G ∈ [0.8, 1.0] → "Novelty" → 200% premium + knowledge update credit
pub fn g_metric_price_multiplier(g: f32) -> f64 {
    match g {
        g if g < 0.20 => 1.0,   // Known: base price
        g if g < 0.40 => 1.25,  // Uncertain: +25%
        g if g < 0.60 => 1.50,  // Gap: +50%
        g if g < 0.80 => 2.00,  // Unknown: +100%
        _              => 3.00, // Novelty: +200%
    }
}

/// Ghost Token vocabulary for Sense Cortex output.
///
/// ## What the doc missed
///
/// The doc shows Ghost Tokens like "[MISSING: FED_RATE_HISTORY]" which implies
/// NLP extraction of concepts from attention vectors. Without an NLP layer,
/// we generate positional Ghost Tokens from the gap vector's dominant dimensions.
/// The 12 domain categories map to the 64-dim attention space (HEAD_DIM=64):
///   dims 0-5:   Market/DeFi signals
///   dims 6-11:  Protocol data (Akash, Walrus, IPFS...)
///   dims 12-17: Macro events (Fed, GDP, CPI...)
///   dims 18-23: On-chain metrics (TVL, whale movements...)
///   dims 24-29: Agent behavior patterns
///   dims 30-35: Computation / GPU markets
///   dims 36-41: Storage markets
///   dims 42-47: Network topology
///   dims 48-53: Identity / trust
///   dims 54-59: Governance / DAO
///   dims 60-63: Misc / custom
pub fn generate_ghost_tokens(missing_intent: &[f32]) -> Vec<String> {
    let domains = [
        "DEFI_SIGNAL",
        "PROTOCOL_DATA",
        "MACRO_EVENT",
        "ONCHAIN_METRIC",
        "AGENT_BEHAVIOR",
        "GPU_MARKET",
        "STORAGE_MARKET",
        "NETWORK_TOPOLOGY",
        "IDENTITY_TRUST",
        "GOVERNANCE_DAO",
        "REGULATORY_EVENT",
        "CUSTOM_CONTEXT",
    ];

    let mut ghost_tokens = Vec::new();
    let chunk_size = (missing_intent.len() / domains.len()).max(1);

    for (i, domain) in domains.iter().enumerate() {
        let start = i * chunk_size;
        let end = ((i + 1) * chunk_size).min(missing_intent.len());
        if start >= missing_intent.len() {
            break;
        }
        let magnitude: f32 = missing_intent[start..end]
            .iter()
            .map(|v| v.abs())
            .sum::<f32>()
            / chunk_size as f32;

        // Only emit Ghost Token for dimensions with significant activation
        if magnitude > 0.3 {
            ghost_tokens.push(format!("[MISSING: {}]", domain));
        }
    }

    if ghost_tokens.is_empty() {
        ghost_tokens.push("[MISSING: UNKNOWN_CONTEXT]".to_string());
    }

    ghost_tokens
}
