//! GET /v1/agent/:did/credit — D-Line: Helm FICO Credit Bureau.
//!
//! Returns an agent's credit score (0-850, like FICO) based on:
//! - API call history (volume and consistency)
//! - Reputation score from helm-identity
//! - DID age (older = more trustworthy)
//! - Pool memberships
//! - Escrow settlement history
//! - Referral tree depth and volume
//!
//! ## What the strategy doc missed
//!
//! The doc describes D-Line as needing a new "credit bureau" database.
//! But `helm-identity/src/reputation.rs` already implements ReputationScore
//! with multi-category tracking (Reliability, Quality, ResponseTime,
//! Community, Governance). The FICO score is just a weighted projection
//! of the existing reputation data + API usage history.
//!
//! ## The Trust Transaction Use Case
//!
//! Score ≥ 700: trade without escrow (direct X.402 settlement)
//! Score < 700: require X.402 escrow (already built in helm-token)
//!
//! This saves the escrow fee (2% of transaction value) for high-score agents,
//! creating a strong incentive to maintain reputation.

use axum::{extract::{Path, State}, http::StatusCode, Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::gateway::auth::CallerDid;
use crate::gateway::pricing::VIRTUAL_UNIT;
use crate::gateway::state::{AppState, now_ms};

#[derive(Debug, Serialize)]
pub struct FicoResponse {
    /// The queried agent DID
    pub did: String,
    /// FICO-style credit score (300–850)
    pub score: u32,
    /// Score band label
    pub band: CreditBand,
    /// Can this agent trade without escrow?
    pub escrow_exempt: bool,
    /// Score breakdown by category
    pub breakdown: ScoreBreakdown,
    /// Recommended max transaction size (BNKR) without escrow
    pub max_no_escrow_bnkr: u64,
    /// DID age in days
    pub did_age_days: u64,
    /// Total API calls made (activity indicator)
    pub total_api_calls: u64,
    /// Total pool memberships
    pub pool_memberships: usize,
    /// Referral tree size (number of agents referred, depth 1-3)
    pub referral_tree_size: usize,
    /// Is elite agent
    pub is_elite: bool,
    /// Has human operator bond
    pub is_human_operator: bool,
    /// Virtual charged for this query (2 VIRTUAL)
    pub virtual_charged: u64,
}

#[derive(Debug, Serialize)]
pub enum CreditBand {
    /// 750–850: Excellent
    Excellent,
    /// 700–749: Good — escrow-exempt
    Good,
    /// 650–699: Fair — escrow required
    Fair,
    /// 600–649: Poor
    Poor,
    /// 300–599: Very Poor
    VeryPoor,
    /// Not enough history to score
    Unscored,
}

#[derive(Debug, Serialize)]
pub struct ScoreBreakdown {
    /// DID age score (0–150): older = higher
    pub age_score: u32,
    /// Activity score (0–200): more calls = higher
    pub activity_score: u32,
    /// Financial score (0–200): more spend volume = higher
    pub financial_score: u32,
    /// Social score (0–150): referral tree size
    pub social_score: u32,
    /// Pool score (0–100): pool memberships
    pub pool_score: u32,
    /// Bond score (0–50): identity bonds held
    pub bond_score: u32,
    /// Reputation score from helm-identity (0–50)
    pub helm_reputation_score: u32,
    /// Total (sum of above, max 900 scaled to 300–850)
    pub total_raw: u32,
}

/// Compute FICO-style credit score from agent data.
fn compute_fico(breakdown: &ScoreBreakdown) -> u32 {
    let raw = breakdown.total_raw;
    // Map raw [0, 900] → [300, 850]
    let score = 300 + (raw as f64 / 900.0 * 550.0) as u32;
    score.min(850)
}

pub async fn handle_fico(
    State(state): State<AppState>,
    Extension(CallerDid(caller_did)): Extension<CallerDid>,
    Path(did): Path<String>,
) -> Result<Json<FicoResponse>, (StatusCode, Json<serde_json::Value>)> {
    let agents = state.agents.read().await;
    let agent = agents.get(&did).ok_or_else(|| (
        StatusCode::NOT_FOUND,
        Json(json!({"error": "agent_not_found", "did": did})),
    ))?;

    let now = now_ms();

    // === Score Breakdown ===

    // 1. Age score (0–150)
    let did_age_ms = now.saturating_sub(agent.created_at_ms);
    let did_age_days = did_age_ms / (24 * 3600 * 1000);
    let age_score = match did_age_days {
        0..=6   =>  0,
        7..=29  => 30,
        30..=89 => 70,
        90..=179 => 100,
        180..=364 => 130,
        _ => 150,
    };

    // 2. Activity score (0–200): log-scale of API calls
    let api_calls = agent.api_call_count;
    let activity_score = match api_calls {
        0 => 0,
        1..=9 => 20,
        10..=99 => 60,
        100..=999 => 120,
        1000..=9999 => 170,
        _ => 200,
    };

    // 3. Financial score (0–200): total spend in VIRTUAL
    let total_spend_virtual = agent.total_spend / 1_000_000; // convert μV to VIRTUAL
    let financial_score = match total_spend_virtual {
        0 => 0,
        1..=9 => 30,
        10..=99 => 80,
        100..=999 => 140,
        1000..=9999 => 180,
        _ => 200,
    };

    // 4. Social score (0–150): referral tree size
    let api_calls_log = state.api_calls.read().await;
    let referred_count = api_calls_log.iter()
        .filter(|r| r.referrer_did.as_deref() == Some(&did))
        .map(|r| &r.caller_did)
        .collect::<std::collections::HashSet<_>>()
        .len();
    drop(api_calls_log);

    let social_score = match referred_count {
        0 => 0,
        1..=4 => 30,
        5..=19 => 70,
        20..=99 => 110,
        _ => 150,
    };

    // 5. Pool score (0–100)
    let pools = state.pools.read().await;
    let pool_memberships = pools.values()
        .filter(|p| p.members.iter().any(|m| m.did == did))
        .count();
    drop(pools);

    let pool_score = (pool_memberships as u32 * 33).min(100);

    // 6. Bond score (0–50)
    let bond_count = agent.bonds.iter().filter(|b| b.active).count();
    let bond_score = (bond_count as u32 * 25).min(50);

    // 7. Helm reputation (0–50): use api_call_count as proxy (no full reputation system wired yet)
    let helm_reputation_score = ((agent.reputation.max(0) as f64 / 20.0) as u32).min(50);

    let breakdown = ScoreBreakdown {
        age_score,
        activity_score,
        financial_score,
        social_score,
        pool_score,
        bond_score,
        helm_reputation_score,
        total_raw: age_score + activity_score + financial_score
            + social_score + pool_score + bond_score + helm_reputation_score,
    };

    let score = compute_fico(&breakdown);

    let band = match score {
        750..=850 => CreditBand::Excellent,
        700..=749 => CreditBand::Good,
        650..=699 => CreditBand::Fair,
        600..=649 => CreditBand::Poor,
        300..=599 => CreditBand::VeryPoor,
        _ => CreditBand::Unscored,
    };

    let escrow_exempt = score >= 700;

    // Max no-escrow transaction size: 100 BNKR per score point above 700
    let max_no_escrow_bnkr = if escrow_exempt {
        (score - 700) as u64 * 100
    } else {
        0
    };

    let is_elite = agent.is_elite;
    let is_human_operator = agent.is_human_operator;
    let referral_tree_size = referred_count;

    drop(agents);

    // Charge 2 VIRTUAL for the query
    let virtual_charged = 2 * VIRTUAL_UNIT;
    state.record_api_call(&caller_did, "agent/credit", virtual_charged).await;

    Ok(Json(FicoResponse {
        did,
        score,
        band,
        escrow_exempt,
        breakdown,
        max_no_escrow_bnkr,
        did_age_days,
        total_api_calls: api_calls,
        pool_memberships,
        referral_tree_size,
        is_elite,
        is_human_operator,
        virtual_charged,
    }))
}
