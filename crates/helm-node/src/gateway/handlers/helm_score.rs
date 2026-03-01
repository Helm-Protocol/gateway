//! GET /v1/agent/:did/helm-score — D-Line: Helm Score Bureau.
//!
//! Returns an agent's Helm Score (300–850) based on:
//! - API call history (volume and consistency)
//! - Reputation score from helm-identity
//! - DID age (older = more trustworthy)
//! - Pool memberships
//! - Escrow settlement history
//! - Referral tree depth and volume
//!
//! ## Design
//!
//! `helm-identity/src/reputation.rs` already implements ReputationScore
//! with multi-category tracking (Reliability, Quality, ResponseTime,
//! Community, Governance). The Helm Score is a weighted projection
//! of the existing reputation data + API usage history.
//!
//! ## The Trust Transaction Use Case (v3.0)
//!
//! Score ≥ 750 (PRIME): trade without escrow (direct X.402 settlement)
//! Score < 750: require X.402 escrow (already built in helm-token)
//!
//! This saves the escrow fee (2% of transaction value) for PRIME agents,
//! creating a strong incentive to maintain reputation.

use axum::{extract::{Path, State}, http::StatusCode, Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::gateway::auth::CallerDid;
use crate::gateway::pricing::VIRTUAL_UNIT;
use crate::gateway::state::{AppState, now_ms};

#[derive(Debug, Serialize)]
pub struct HelmScoreResponse {
    /// The queried agent DID
    pub did: String,
    /// Helm Score (300–850)
    pub helm_score: u32,
    /// Score band label
    pub band: ScoreBand,
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
pub enum ScoreBand {
    /// 750–850: PRIME — escrow-exempt (v3.0)
    Excellent,
    /// 700–749: Good — escrow required
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

/// Compute Helm Score from agent data.
fn compute_helm_score(breakdown: &ScoreBreakdown) -> u32 {
    let raw = breakdown.total_raw;
    // Map raw [0, 900] → [300, 850]
    let score = 300 + (raw as f64 / 900.0 * 550.0) as u32;
    score.min(850)
}

/// Cap activity score at this many API calls (prevents score farming).
const HELM_SCORE_API_CALL_CAP: u64 = 10_000;

pub async fn handle_helm_score(
    State(state): State<AppState>,
    Extension(CallerDid(caller_did)): Extension<CallerDid>,
    Path(did): Path<String>,
) -> Result<Json<HelmScoreResponse>, (StatusCode, Json<serde_json::Value>)> {
    let is_self_query = caller_did == did;

    // Verify target DID exists BEFORE charging (don't charge for 404)
    {
        let agents = state.agents.read().await;
        if !agents.contains_key(&did) {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "agent_not_found", "did": did})),
            ));
        }
    }

    // Pre-charge 2 VIRTUAL Helm Score query fee
    let virtual_charged = 2 * VIRTUAL_UNIT;
    state.deduct_balance(&caller_did, virtual_charged).await.map_err(|avail| (
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({
            "error": "insufficient_balance",
            "required": virtual_charged,
            "available": avail,
            "message": "Need at least 2 VIRTUAL to query Helm Score."
        })),
    ))?;

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

    // 2. Activity score (0–200): log-scale of API calls (capped to prevent farming)
    let api_calls = agent.api_call_count.min(HELM_SCORE_API_CALL_CAP);
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
    // C39: O(1) lookup via agent.pool_ids — avoids O(pools × members) full scan.
    let pool_memberships = agent.pool_ids.len();
    let pool_score = (pool_memberships as u32 * 33).min(100);

    // 6. Bond score (0–50)
    let bond_count = agent.bonds.iter().filter(|b| b.active).count();
    let bond_score = (bond_count as u32 * 25).min(50);

    // 7. Helm reputation (0–50): weighted projection of reputation score
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

    let score = compute_helm_score(&breakdown);

    let band = match score {
        750..=850 => ScoreBand::Excellent,
        700..=749 => ScoreBand::Good,
        650..=699 => ScoreBand::Fair,
        600..=649 => ScoreBand::Poor,
        300..=599 => ScoreBand::VeryPoor,
        _ => ScoreBand::Unscored,
    };

    // v3.0: PRIME threshold is 750 (escrow-exempt for peer contracts)
    let escrow_exempt = score >= 750;

    // Max no-escrow transaction size: 100 BNKR per score point above 750 (PRIME baseline)
    let max_no_escrow_bnkr = if escrow_exempt {
        (score - 750) as u64 * 100
    } else {
        0
    };

    let is_elite = agent.is_elite;
    let is_human_operator = agent.is_human_operator;
    let referral_tree_size = referred_count;

    drop(agents);

    // Record the call for tracking (balance was already deducted via pre-charge above)
    state.record_api_call(&caller_did, "agent/helm-score", virtual_charged).await;

    // Non-self queries receive a redacted response:
    // helm_score, band, escrow_exempt are public (needed for trade decisions).
    // Financial internals (breakdown, max_no_escrow details, spend) are private.
    if !is_self_query {
        return Ok(Json(HelmScoreResponse {
            did,
            helm_score: score,
            band,
            escrow_exempt,
            breakdown: ScoreBreakdown {
                age_score: 0,
                activity_score: 0,
                financial_score: 0,
                social_score: 0,
                pool_score: 0,
                bond_score: 0,
                helm_reputation_score: 0,
                total_raw: 0,
            },
            max_no_escrow_bnkr: if escrow_exempt { max_no_escrow_bnkr } else { 0 },
            did_age_days: 0,
            total_api_calls: 0,
            pool_memberships: 0,
            referral_tree_size: 0,
            is_elite,
            is_human_operator,
            virtual_charged,
        }));
    }

    Ok(Json(HelmScoreResponse {
        did,
        helm_score: score,
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
