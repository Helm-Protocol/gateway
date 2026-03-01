//! POST /v1/package/subscribe — monthly package subscription.
//!
//! ## Tiers
//!
//! | Tier            | Price        | USD equiv | Marketplace | Benefits                            |
//! |-----------------|--------------|-----------|-------------|-------------------------------------|
//! | AlphaHunt       | 200V/month   | ~$130     | Unlimited   | Alpha Hunt bundle, G-score filter   |
//! | ProtocolShield  | 300V/month   | ~$195     | Unlimited   | Protocol Shield, B2B rate card      |
//! | SovereignAgent  | 750V/month   | ~$487     | Unlimited   | All lines, priority, escrow-exempt  |
//!
//! Free tier: 3 marketplace posts max, pay-per-call only.
//!
//! ## Revenue
//! 100% of subscription revenue → Jay treasury (billing.charge_package_subscription).
//!
//! ## Effect
//! Sets agent.package_tier immediately. Marketplace post limit enforced in create_post.

use axum::{extract::State, http::StatusCode, Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use helm_engine::api::billing::{
    PACKAGE_ALPHA_HUNT_MONTHLY,
    PACKAGE_PROTOCOL_SHIELD_MONTHLY,
    PACKAGE_SOVEREIGN_MONTHLY,
};

use crate::gateway::auth::CallerDid;
use crate::gateway::state::{AppState, PackageTier, now_ms};

#[derive(Debug, Deserialize)]
pub struct SubscribeRequest {
    /// "AlphaHunt" | "ProtocolShield" | "SovereignAgent"
    pub tier: String,
    /// Number of months to pay upfront (1–12)
    #[serde(default = "default_months")]
    pub months: u32,
}

fn default_months() -> u32 { 1 }

#[derive(Debug, Serialize)]
pub struct SubscribeResponse {
    pub tier: String,
    pub months: u32,
    pub total_charged_virtual: u64,
    pub price_per_month_virtual: u64,
    pub new_balance: u64,
    pub marketplace_post_limit: String,
    pub message: String,
}

pub async fn handle_subscribe(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Json(req): Json<SubscribeRequest>,
) -> Result<(StatusCode, Json<SubscribeResponse>), (StatusCode, Json<serde_json::Value>)> {
    if req.months == 0 || req.months > 12 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_months",
            "message": "months must be 1–12"
        }))));
    }

    let (tier, price_per_month) = match req.tier.as_str() {
        "AlphaHunt" => (PackageTier::AlphaHunt, PACKAGE_ALPHA_HUNT_MONTHLY),
        "ProtocolShield" => (PackageTier::ProtocolShield, PACKAGE_PROTOCOL_SHIELD_MONTHLY),
        "SovereignAgent" => (PackageTier::SovereignAgent, PACKAGE_SOVEREIGN_MONTHLY),
        other => return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_tier",
            "valid_tiers": ["AlphaHunt", "ProtocolShield", "SovereignAgent"],
            "received": other,
        })))),
    };

    let total = price_per_month.saturating_mul(req.months as u64);

    // Deduct subscription fee
    let new_balance = state.deduct_balance(&did, total).await.map_err(|avail| (
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({
            "error": "insufficient_balance",
            "required_virtual": total,
            "price_per_month": price_per_month,
            "months": req.months,
            "available_virtual": avail,
        })),
    ))?;

    // Record 100% → treasury
    let ts = now_ms();
    state.billing.write().await.charge_package_subscription(&did, total, ts);

    // Upgrade agent tier
    {
        let mut agents = state.agents.write().await;
        if let Some(agent) = agents.get_mut(&did) {
            agent.package_tier = tier;
        }
    }

    let tier_str = req.tier.clone();
    tracing::info!(
        "Package subscribe: did={} tier={} months={} total_virtual={}",
        did, tier_str, req.months, total
    );

    Ok((StatusCode::CREATED, Json(SubscribeResponse {
        tier: tier_str,
        months: req.months,
        total_charged_virtual: total,
        price_per_month_virtual: price_per_month,
        new_balance,
        marketplace_post_limit: "Unlimited".to_string(),
        message: format!(
            "Subscribed to {} for {} month(s). Marketplace posting is now unlimited. \
             VIRTUAL charged: {}.",
            req.tier, req.months, total
        ),
    })))
}
