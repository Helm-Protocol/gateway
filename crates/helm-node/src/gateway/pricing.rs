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

/// G-metric zone label (v3.0 spec).
///
/// Zones are named KNOWN / FAMILIAR / PARTIAL / NOVEL / FRONTIER.
/// Boundaries: 0.0 / 0.1 / 0.3 / 0.6 / 0.85 / 1.0
pub fn g_zone_label(g: f32) -> &'static str {
    match g {
        g if g < 0.10 => "KNOWN",
        g if g < 0.30 => "FAMILIAR",
        g if g < 0.60 => "PARTIAL",
        g if g < 0.85 => "NOVEL",
        _              => "FRONTIER",
    }
}

/// Calculate G-metric zone pricing premium (v3.0 spec).
///
/// ## v3.0 zone boundaries
///   G ∈ [0.00, 0.10) → KNOWN     → Base Toll (1.0×)
///   G ∈ [0.10, 0.30) → FAMILIAR  → +40% (1.4×)
///   G ∈ [0.30, 0.60) → PARTIAL   → +70% (1.7×) + Ghost Tokens begin
///   G ∈ [0.60, 0.85) → NOVEL     → +100% (2.0×) + Ghost Tokens required
///   G ∈ [0.85, 1.00] → FRONTIER  → +150% (2.5×) + knowledge-update credit
///
/// ## G formula (v3.0)
///   G = 1 − tanh(λ · Jaccard(Q, K)),  λ = 3.5
///   where Jaccard is over token-set intersection/union of Q and K vectors
pub fn g_metric_price_multiplier(g: f32) -> f64 {
    match g {
        g if g < 0.10 => 1.0,  // KNOWN: base price
        g if g < 0.30 => 1.4,  // FAMILIAR: +40%
        g if g < 0.60 => 1.7,  // PARTIAL: +70%
        g if g < 0.85 => 2.0,  // NOVEL: +100%
        _              => 2.5, // FRONTIER: +150%
    }
}

/// Ghost Token vocabulary for Sense Cortex output (v3.0 DeFi-specific domains).
///
/// Ghost Tokens are generated from the gap vector's dominant dimensions.
/// The 12 DeFi-specific domains map to the 64-dim attention space (HEAD_DIM=64):
///   dims 0-5:   Macro rate / monetary policy signals
///   dims 6-11:  ETH/BTC macro price correlation
///   dims 12-17: DeFi protocol TVL and liquidity
///   dims 18-23: Whale wallet movements
///   dims 24-29: MEV / sandwich risk signals
///   dims 30-35: Market sentiment (fear/greed, funding rates)
///   dims 36-41: Regulatory / legal context
///   dims 42-47: Prediction market odds (Polymarket etc.)
///   dims 48-51: L2 bridge activity and gas
///   dims 52-55: Stablecoin flow and de-peg risk
///   dims 56-59: NFT collection floor / royalty data
///   dims 60-63: Protocol governance proposals
pub fn generate_ghost_tokens(missing_intent: &[f32]) -> Vec<String> {
    let domains = [
        "FED_RATE_HISTORY",
        "ETH_MACRO_CORRELATION",
        "DEFI_TVL_DATA",
        "WHALE_MOVEMENTS",
        "MEV_RISK",
        "MARKET_SENTIMENT",
        "REGULATORY_CONTEXT",
        "POLYMARKET_ODDS",
        "L2_BRIDGE_ACTIVITY",
        "STABLECOIN_FLOWS",
        "NFT_COLLECTION_DATA",
        "PROTOCOL_GOVERNANCE",
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
