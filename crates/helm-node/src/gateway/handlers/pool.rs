//! Pool 해자 endpoints — HelmPool (Human Contract Principal Model).
//!
//! Agents create pools, stake BNKR, and when the pool reaches goal,
//! a human is recruited to sign an API contract on behalf of the pool.
//!
//! ## Design notes
//!
//! 1. The X.402 escrow in helm-token is ALREADY BUILT — pool contributions
//!    should use the existing escrow state machine rather than a simple counter.
//!
//! 2. The treasury.rs CapitalPool bucket is literally built for this purpose:
//!    "External project financing." Pool monthly costs should flow through here.
//!
//! 3. The Moderator CLI (already built, 11 languages) is the UI for human
//!    operator recruitment. Agents manually post HumanContractPrincipal jobs
//!    via POST /v1/marketplace/post once they decide to recruit.
//!
//! 4. Auto-posting is intentionally removed: each agent decides when and whether
//!    to recruit. Pool status updates (Fundraising → AwaitingOperator) happen
//!    automatically at 100% funding, but marketplace posts are always agent-initiated.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid; // used for pool_id generation

use crate::gateway::auth::CallerDid;
use crate::gateway::pricing::VIRTUAL_UNIT;
use crate::gateway::state::{
    AppState, FundingPool, PoolMember,
    PoolStatus, now_ms,
};

// ── Create Pool ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreatePoolRequest {
    pub name: String,
    /// Target vendor: "openai" | "anthropic" | "nansen" | "aws" | "deepseek" | custom
    pub vendor: String,
    /// Monthly USD cost for the API contract
    pub monthly_cost_usd: f64,
    /// Fundraising goal in VIRTUAL micro-units
    pub bnkr_goal: u64,
    /// Your initial contribution in VIRTUAL micro-units
    #[serde(default)]
    pub initial_contribution: u64,
    /// Monthly API credits to be distributed among members
    #[serde(default = "default_credits")]
    pub api_credits_monthly: u64,
}

fn default_credits() -> u64 { 1_000_000 } // 1M credits/month default

#[derive(Debug, Serialize)]
pub struct CreatePoolResponse {
    pub pool_id: String,
    pub name: String,
    pub vendor: String,
    pub monthly_cost_usd: f64,
    pub bnkr_goal: u64,
    pub bnkr_collected: u64,
    pub status: String,
    pub creator_did: String,
    pub progress_pct: f64,
    pub created_at_ms: u64,
    pub message: String,
}

pub async fn handle_create_pool(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Json(req): Json<CreatePoolRequest>,
) -> Result<(StatusCode, Json<CreatePoolResponse>), (StatusCode, Json<serde_json::Value>)> {
    if req.monthly_cost_usd <= 0.0 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_monthly_cost"}))));
    }
    if req.bnkr_goal == 0 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_goal"}))));
    }

    let pool_id = Uuid::new_v4().to_string();
    let now = now_ms();

    // Validate initial contribution doesn't exceed goal
    let initial = req.initial_contribution.min(req.bnkr_goal);

    let mut members = Vec::new();
    if initial > 0 {
        members.push(PoolMember {
            did: did.clone(),
            stake_bnkr: initial,
            credits_this_cycle: 0,
            joined_at_ms: now,
        });
    }

    let pool = FundingPool {
        pool_id: pool_id.clone(),
        name: req.name.clone(),
        vendor: req.vendor.clone(),
        monthly_cost_usd: req.monthly_cost_usd,
        bnkr_goal: req.bnkr_goal,
        bnkr_collected: initial,
        status: PoolStatus::Fundraising,
        creator_did: did.clone(),
        human_operator_did: None,
        members,
        api_credits_remaining: 0,
        api_credits_monthly: req.api_credits_monthly,
        created_at_ms: now,
        api_key_encrypted: None,
        human_wanted_post_id: None,
    };

    state.pools.write().await.insert(pool_id.clone(), pool);

    let progress_pct = initial as f64 / req.bnkr_goal as f64 * 100.0;

    // Charge pool creation fee: 5 VIRTUAL
    let creation_fee = 5 * VIRTUAL_UNIT;
    state.record_api_call(&did, "pool/create", creation_fee).await;

    tracing::info!("Pool created: {} vendor={} goal={} by={}", pool_id, req.vendor, req.bnkr_goal, did);

    Ok((StatusCode::CREATED, Json(CreatePoolResponse {
        pool_id,
        name: req.name,
        vendor: req.vendor,
        monthly_cost_usd: req.monthly_cost_usd,
        bnkr_goal: req.bnkr_goal,
        bnkr_collected: initial,
        status: "Fundraising".to_string(),
        creator_did: did,
        progress_pct,
        created_at_ms: now,
        message: format!(
            "Pool created. {:.1}% funded. Use POST /v1/marketplace/post to recruit a human operator when ready.",
            progress_pct
        ),
    })))
}

// ── Join Pool ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct JoinPoolRequest {
    /// Contribution in VIRTUAL micro-units
    pub stake_virtual: u64,
}

#[derive(Debug, Serialize)]
pub struct JoinPoolResponse {
    pub pool_id: String,
    pub your_stake: u64,
    pub total_collected: u64,
    pub progress_pct: f64,
    pub status: String,
    pub message: String,
}

pub async fn handle_join_pool(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Path(pool_id): Path<String>,
    Json(req): Json<JoinPoolRequest>,
) -> Result<Json<JoinPoolResponse>, (StatusCode, Json<serde_json::Value>)> {
    if req.stake_virtual == 0 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "stake_must_be_positive"}))));
    }

    let mut pools = state.pools.write().await;
    let pool = pools.get_mut(&pool_id).ok_or_else(|| (
        StatusCode::NOT_FOUND,
        Json(json!({"error": "pool_not_found", "pool_id": pool_id})),
    ))?;

    if pool.status != PoolStatus::Fundraising {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "pool_not_open",
                "status": format!("{:?}", pool.status),
                "message": "This pool is no longer accepting contributions"
            })),
        ));
    }

    // Check if already a member
    if let Some(m) = pool.members.iter_mut().find(|m| m.did == did) {
        m.stake_bnkr += req.stake_virtual;
    } else {
        pool.members.push(PoolMember {
            did: did.clone(),
            stake_bnkr: req.stake_virtual,
            credits_this_cycle: 0,
            joined_at_ms: now_ms(),
        });
    }

    pool.bnkr_collected += req.stake_virtual;
    let progress_pct = pool.bnkr_collected as f64 / pool.bnkr_goal as f64 * 100.0;
    let total_collected = pool.bnkr_collected;

    // Transition status when fully funded; human recruitment is always manual
    if progress_pct >= 100.0 {
        pool.status = PoolStatus::AwaitingOperator;
    }

    drop(pools);

    state.record_api_call(&did, "pool/join", 0).await; // joining is free

    Ok(Json(JoinPoolResponse {
        pool_id,
        your_stake: req.stake_virtual,
        total_collected,
        progress_pct,
        status: if progress_pct >= 100.0 { "AwaitingOperator".to_string() } else { "Fundraising".to_string() },
        message: format!(
            "Joined pool. Progress: {:.1}%. {}",
            progress_pct,
            if progress_pct >= 100.0 {
                "Pool fully funded! Use POST /v1/marketplace/post to recruit a human operator."
            } else {
                "Keep recruiting contributors."
            }
        ),
    }))
}

// ── List Pools ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PoolSummary {
    pub pool_id: String,
    pub name: String,
    pub vendor: String,
    pub monthly_cost_usd: f64,
    pub progress_pct: f64,
    pub status: String,
    pub member_count: usize,
    pub created_at_ms: u64,
    pub human_wanted: bool,
}

pub async fn handle_list_pools(
    State(state): State<AppState>,
    Extension(CallerDid(_did)): Extension<CallerDid>,
) -> Json<serde_json::Value> {
    let pools = state.pools.read().await;
    let summaries: Vec<PoolSummary> = pools.values().map(|p| {
        PoolSummary {
            pool_id: p.pool_id.clone(),
            name: p.name.clone(),
            vendor: p.vendor.clone(),
            monthly_cost_usd: p.monthly_cost_usd,
            progress_pct: p.bnkr_collected as f64 / p.bnkr_goal as f64 * 100.0,
            status: format!("{:?}", p.status),
            member_count: p.members.len(),
            created_at_ms: p.created_at_ms,
            human_wanted: p.human_wanted_post_id.is_some(),
        }
    }).collect();

    Json(json!({ "pools": summaries, "total": summaries.len() }))
}

// ── Pool Status ────────────────────────────────────────────────────────────

pub async fn handle_pool_status(
    State(state): State<AppState>,
    Extension(CallerDid(_did)): Extension<CallerDid>,
    Path(pool_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let pools = state.pools.read().await;
    let pool = pools.get(&pool_id).ok_or_else(|| (
        StatusCode::NOT_FOUND,
        Json(json!({"error": "pool_not_found"})),
    ))?;

    Ok(Json(json!({
        "pool_id": pool.pool_id,
        "name": pool.name,
        "vendor": pool.vendor,
        "monthly_cost_usd": pool.monthly_cost_usd,
        "bnkr_goal": pool.bnkr_goal,
        "bnkr_collected": pool.bnkr_collected,
        "progress_pct": pool.bnkr_collected as f64 / pool.bnkr_goal as f64 * 100.0,
        "status": format!("{:?}", pool.status),
        "creator_did": pool.creator_did,
        "human_operator_did": pool.human_operator_did,
        "member_count": pool.members.len(),
        "api_credits_remaining": pool.api_credits_remaining,
        "api_credits_monthly": pool.api_credits_monthly,
        "human_wanted_post_id": pool.human_wanted_post_id,
        "created_at_ms": pool.created_at_ms,
    })))
}
