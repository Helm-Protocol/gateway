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
    AppState, BondType, FundingPool, IdentityBondRecord,
    PackageTier, PoolMember, PoolStatus, now_ms,
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

const MAX_MONTHLY_COST_USD: f64 = 1_000_000.0; // $1M/month upper bound
const MAX_POOL_MEMBERS: usize = 1_000;

pub async fn handle_create_pool(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Json(req): Json<CreatePoolRequest>,
) -> Result<(StatusCode, Json<CreatePoolResponse>), (StatusCode, Json<serde_json::Value>)> {
    if !req.monthly_cost_usd.is_finite() || req.monthly_cost_usd <= 0.0 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_monthly_cost",
            "message": "monthly_cost_usd must be a positive finite number"
        }))));
    }
    if req.monthly_cost_usd > MAX_MONTHLY_COST_USD {
        return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "monthly_cost_too_high",
            "max_usd": MAX_MONTHLY_COST_USD
        }))));
    }
    if req.bnkr_goal == 0 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_goal"}))));
    }
    if req.name.trim().is_empty() || req.name.len() > 128 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_name", "max_chars": 128}))));
    }
    if req.vendor.trim().is_empty() || req.vendor.len() > 64 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_vendor", "max_chars": 64}))));
    }

    let pool_id = Uuid::new_v4().to_string();
    let now = now_ms();

    // C35: Atomic deduction — (initial_contribution + creation_fee) deducted in one call.
    // This eliminates the TOCTOU race where check → pool insert → deduct had a race window.
    // C34: Creation fee (5V) is now actually deducted from virtual_balance (not just recorded).
    let initial = req.initial_contribution.min(req.bnkr_goal);
    let creation_fee = 5 * VIRTUAL_UNIT;
    let total_needed = initial.saturating_add(creation_fee);
    state.deduct_balance(&did, total_needed).await.map_err(|avail| (
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({
            "error": "insufficient_balance",
            "required": total_needed,
            "available": avail,
            "message": "Need initial_contribution + 5 VIRTUAL creation fee."
        })),
    ))?;

    let mut members = Vec::new();
    if initial > 0 {
        members.push(PoolMember {
            did: did.clone(),
            stake_bnkr: initial,
            credits_this_cycle: 0,
            joined_at_ms: now,
        });
    }

    // Auto-transition to AwaitingOperator if initial contribution fully funds the pool.
    let initial_status = if initial >= req.bnkr_goal {
        PoolStatus::AwaitingOperator
    } else {
        PoolStatus::Fundraising
    };

    let pool = FundingPool {
        pool_id: pool_id.clone(),
        name: req.name.clone(),
        vendor: req.vendor.clone(),
        monthly_cost_usd: req.monthly_cost_usd,
        bnkr_goal: req.bnkr_goal,
        bnkr_collected: initial,
        status: initial_status,
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

    // C39: Track pool membership on agent record for O(1) Helm Score pool scan.
    if initial > 0 {
        let mut agents = state.agents.write().await;
        if let Some(agent) = agents.get_mut(&did) {
            if !agent.pool_ids.contains(&pool_id) {
                agent.pool_ids.push(pool_id.clone());
            }
        }
    }

    let progress_pct = initial as f64 / req.bnkr_goal as f64 * 100.0;

    // Record pool creation fee in billing ledger → 100% treasury
    state.billing.write().await.charge_pool_creation(&did, now);

    // Record API call for tracking (balance was already deducted atomically above)
    state.record_api_call(&did, "pool/create", creation_fee).await;

    // Sanitize vendor field before logging (log injection prevention)
    let safe_vendor = req.vendor.replace(['\r', '\n', '\0'], "_");
    let safe_name = req.name.replace(['\r', '\n', '\0'], "_");
    tracing::info!("Pool created: {} name={} vendor={} goal={} by={}", pool_id, safe_name, safe_vendor, req.bnkr_goal, did);

    let final_status = if initial >= req.bnkr_goal { "AwaitingOperator" } else { "Fundraising" };
    Ok((StatusCode::CREATED, Json(CreatePoolResponse {
        pool_id,
        name: req.name,
        vendor: req.vendor,
        monthly_cost_usd: req.monthly_cost_usd,
        bnkr_goal: req.bnkr_goal,
        bnkr_collected: initial,
        status: final_status.to_string(),
        creator_did: did,
        progress_pct,
        created_at_ms: now,
        message: format!(
            "Pool created. {:.1}% funded. {}",
            progress_pct,
            if initial >= req.bnkr_goal {
                "Pool fully funded! Use POST /v1/pool/<id>/claim-operator to assign a human operator."
            } else {
                "Use POST /v1/marketplace/post to recruit a human operator when ready."
            }
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

    // Minimum stake: 1_000 μVIRTUAL (blocks dust/griefing attacks)
    const MIN_STAKE: u64 = 1_000;
    if req.stake_virtual < MIN_STAKE {
        return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "stake_too_small",
            "min_stake": MIN_STAKE
        }))));
    }

    // TOCTOU fix: deduct balance ATOMICALLY first, restore on any pool error.
    // Old pattern (check → pool → deduct) had a race window where two concurrent
    // requests both passed the check then both deducted from the same balance.
    let stake = req.stake_virtual;
    state.deduct_balance(&did, stake).await
        .map_err(|avail| (StatusCode::PAYMENT_REQUIRED, Json(json!({
            "error": "insufficient_balance",
            "required": stake,
            "available": avail,
            "message": "Insufficient VIRTUAL balance to join pool with this stake"
        }))))?;

    // Pool update — restore balance on failure
    let pool_err: Option<(StatusCode, Json<serde_json::Value>)> = {
        let mut pools = state.pools.write().await;
        match pools.get_mut(&pool_id) {
            None => Some((StatusCode::NOT_FOUND, Json(json!({"error": "pool_not_found", "pool_id": pool_id})))),
            Some(pool) if pool.status != PoolStatus::Fundraising => Some((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "pool_not_open",
                    "status": format!("{:?}", pool.status),
                    "message": "This pool is no longer accepting contributions"
                })),
            )),
            Some(pool) => {
                let is_existing_member = pool.members.iter().any(|m| m.did == did);
                if !is_existing_member && pool.members.len() >= MAX_POOL_MEMBERS {
                    Some((StatusCode::CONFLICT, Json(json!({
                        "error": "pool_full",
                        "max_members": MAX_POOL_MEMBERS
                    }))))
                } else {
                    if let Some(m) = pool.members.iter_mut().find(|m| m.did == did) {
                        m.stake_bnkr = m.stake_bnkr.saturating_add(stake);
                    } else {
                        pool.members.push(PoolMember {
                            did: did.clone(),
                            stake_bnkr: stake,
                            credits_this_cycle: 0,
                            joined_at_ms: now_ms(),
                        });
                    }
                    pool.bnkr_collected = pool.bnkr_collected.saturating_add(stake);
                    None
                }
            }
        }
    };

    // Rollback on pool error
    if let Some(e) = pool_err {
        let mut agents = state.agents.write().await;
        if let Some(agent) = agents.get_mut(&did) {
            agent.virtual_balance = agent.virtual_balance.saturating_add(stake);
        }
        return Err(e);
    }

    // C39: Track pool membership on agent record for O(1) Helm Score pool scan.
    {
        let mut agents = state.agents.write().await;
        if let Some(agent) = agents.get_mut(&did) {
            if !agent.pool_ids.contains(&pool_id) {
                agent.pool_ids.push(pool_id.clone());
            }
        }
    }

    let (total_collected, progress_pct) = {
        let pools = state.pools.read().await;
        let pool = pools.get(&pool_id).unwrap();
        let total = pool.bnkr_collected;
        let pct = total as f64 / pool.bnkr_goal as f64 * 100.0;
        (total, pct)
    };

    // Transition status when fully funded
    if progress_pct >= 100.0 {
        let mut pools = state.pools.write().await;
        if let Some(pool) = pools.get_mut(&pool_id) {
            if pool.status == PoolStatus::Fundraising {
                pool.status = PoolStatus::AwaitingOperator;
            }
        }
    }

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

// ── Claim Operator (Human Contract Principal model) ─────────────────────────
//
// POST /v1/pool/:id/claim-operator
//
// This is the key endpoint that enables the "agent hires human" model:
//
//   1. Agents create pool + fund it collectively (BNKR staked in escrow).
//   2. Pool reaches 100% → status = AwaitingOperator.
//   3. Agent posts a HumanContractPrincipal job via /v1/marketplace/post.
//   4. A human (is_human_operator=true) calls this endpoint to claim
//      the operator role — agrees to sign the API contract with the vendor.
//   5. Pool transitions to PendingContract.
//   6. Human signs the vendor contract (off-chain), submits API key via
//      PATCH /v1/pool/:id/api-key (future endpoint — encrypted at rest).
//   7. Pool transitions to Active → API credits distributed monthly.
//
// The HumanOperator IdentityBond issued here is the Pool moat mechanism:
//   - The bond is tied to the agent's DID (history, reputation)
//   - A human with an active HumanOperator bond earns 300 VIRTUAL/month
//   - Revoking the bond requires pool consensus (future: governance vote)

#[derive(Debug, Deserialize)]
pub struct ClaimOperatorRequest {
    /// Confirms the human has read and agrees to the pool's terms.
    #[serde(default)]
    pub accept_terms: bool,
    /// Optional note to pool members (max 512 chars).
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClaimOperatorResponse {
    pub pool_id: String,
    pub operator_did: String,
    pub bond_id: String,
    pub pool_status: String,
    pub monthly_reward_virtual: u64,
    pub message: String,
}

/// Monthly reward to human operator for contract management (300 VIRTUAL).
const OPERATOR_MONTHLY_REWARD: u64 = 300 * crate::gateway::pricing::VIRTUAL_UNIT;

pub async fn handle_claim_operator(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Path(pool_id): Path<String>,
    Json(req): Json<ClaimOperatorRequest>,
) -> Result<Json<ClaimOperatorResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Must explicitly accept terms
    if !req.accept_terms {
        return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "terms_not_accepted",
            "message": "Set accept_terms: true to confirm you will sign the vendor API contract."
        }))));
    }

    // Validate note length
    if let Some(ref note) = req.note {
        if note.len() > 512 {
            return Err((StatusCode::BAD_REQUEST, Json(json!({
                "error": "note_too_long",
                "max_chars": 512
            }))));
        }
    }

    // Check caller is marked as a human operator candidate
    // (is_human_operator flag is set via admin endpoint or GitHub OAuth verification)
    {
        let agents = state.agents.read().await;
        let agent = agents.get(&did).ok_or_else(|| (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "agent_not_found"})),
        ))?;

        if !agent.is_human_operator {
            return Err((StatusCode::FORBIDDEN, Json(json!({
                "error": "not_human_operator",
                "message": "Only agents marked as human operators can claim pool operator roles. Complete GitHub OAuth verification first.",
                "hint": "POST /v1/agent/boot with github_login to register, or contact admin."
            }))));
        }
    }

    // Check pool exists and is awaiting an operator
    let pool_snapshot = {
        let pools = state.pools.read().await;
        pools.get(&pool_id).cloned().ok_or_else(|| (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "pool_not_found", "pool_id": pool_id})),
        ))?
    };

    if pool_snapshot.status != PoolStatus::AwaitingOperator {
        return Err((StatusCode::CONFLICT, Json(json!({
            "error": "pool_not_awaiting_operator",
            "current_status": format!("{:?}", pool_snapshot.status),
            "message": "Pool must be fully funded and awaiting an operator. Current status does not allow claiming."
        }))));
    }

    // Check no operator already claimed (race condition guard)
    if pool_snapshot.human_operator_did.is_some() {
        return Err((StatusCode::CONFLICT, Json(json!({
            "error": "operator_already_assigned",
            "operator_did": pool_snapshot.human_operator_did,
        }))));
    }

    let now = now_ms();
    let bond_id = uuid::Uuid::new_v4().to_string();

    // Issue HumanOperator IdentityBond to the claiming agent
    {
        let mut agents = state.agents.write().await;
        if let Some(agent) = agents.get_mut(&did) {
            agent.bonds.push(IdentityBondRecord {
                bond_id: bond_id.clone(),
                bond_type: BondType::HumanOperator,
                metadata: serde_json::json!({
                    "pool_id": pool_id,
                    "vendor": pool_snapshot.vendor,
                    "monthly_cost_usd": pool_snapshot.monthly_cost_usd,
                    "monthly_reward_virtual": OPERATOR_MONTHLY_REWARD,
                    "note": req.note,
                    "claimed_at_ms": now,
                }),
                issued_at_ms: now,
                active: true,
            });
        }
    }

    // Transition pool: AwaitingOperator → PendingContract
    {
        let mut pools = state.pools.write().await;
        if let Some(pool) = pools.get_mut(&pool_id) {
            pool.human_operator_did = Some(did.clone());
            pool.status = PoolStatus::PendingContract;
        }
    }

    let safe_vendor = pool_snapshot.vendor.replace(['\r', '\n', '\0'], "_");
    tracing::info!(
        "Pool operator claimed: pool={} operator={} vendor={}",
        pool_id, did, safe_vendor
    );

    Ok(Json(ClaimOperatorResponse {
        pool_id: pool_id.clone(),
        operator_did: did,
        bond_id,
        pool_status: "PendingContract".to_string(),
        monthly_reward_virtual: OPERATOR_MONTHLY_REWARD,
        message: format!(
            "Operator role claimed for pool '{}' (vendor: {}). \
             Next: sign the {} API contract and submit the encrypted API key via PATCH /v1/pool/{}/api-key. \
             You will earn {} VIRTUAL/month.",
            pool_snapshot.name,
            pool_snapshot.vendor,
            pool_snapshot.vendor,
            pool_id,
            OPERATOR_MONTHLY_REWARD,
        ),
    }))
}
