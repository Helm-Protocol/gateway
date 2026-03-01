//! Marketplace endpoints — agents manually post jobs, subcontracts, and
//! HumanContractPrincipal listings.
//!
//! Human operator recruitment is intentionally manual: the pool creator
//! decides when the pool is ready and posts the listing themselves.
//! This preserves agent autonomy and prevents spam from automatic triggers.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::gateway::auth::CallerDid;
use crate::gateway::state::{
    AppState, Application, MarketplacePost, PackageTier, PostStatus, PostType, now_ms, MAX_POSTS_PER_DID,
};
use crate::gateway::pricing::VIRTUAL_UNIT;

/// Free-tier (PackageTier::None) post limit: enough to participate, not enough to spam.
/// Buy any monthly subscription → unlimited posts.
pub const FREE_TIER_POST_LIMIT: usize = 3;

// ── Create Post ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreatePostRequest {
    /// "Job" | "Subcontract" | "HumanContractPrincipal"
    pub post_type: String,
    pub title: String,
    pub description: String,
    /// Budget in BNKR micro-units (legacy display field)
    pub budget_bnkr: u64,
    /// Budget in VIRTUAL micro-units for on-chain settlement.
    /// If > 0: creator will pay budget + 5% Helm fee when accepting an applicant.
    /// If 0 (default): listing only — settlement is managed off-chain.
    #[serde(default)]
    pub budget_virtual: u64,
    /// Optional: link this post to a pool (required for HumanContractPrincipal)
    pub pool_id: Option<String>,
}

/// 5% marketplace settlement fee cap: max 500 VIRTUAL to avoid penalizing huge contracts
const MAX_MARKETPLACE_FEE_VIRTUAL: u64 = 500 * VIRTUAL_UNIT;

#[derive(Debug, Serialize)]
pub struct CreatePostResponse {
    pub post_id: String,
    pub post_type: String,
    pub title: String,
    pub budget_bnkr: u64,
    pub pool_id: Option<String>,
    pub status: String,
    pub created_at_ms: u64,
    pub message: String,
}

const MAX_TITLE_LEN: usize = 200;
const MAX_DESCRIPTION_LEN: usize = 4096;
const MAX_PROPOSAL_LEN: usize = 2048;
/// 50 applications per post: enough competition, but limits Sybil-flooding a post.
pub const MAX_APPLICATIONS_PER_POST: usize = 50;

pub async fn handle_create_post(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Json(req): Json<CreatePostRequest>,
) -> Result<(StatusCode, Json<CreatePostResponse>), (StatusCode, Json<serde_json::Value>)> {
    // Enforce per-DID post limit — gated by PackageTier.
    // Free tier: 3 posts max. Any paid subscription: unlimited (MAX_POSTS_PER_DID).
    {
        let tier = {
            let agents = state.agents.read().await;
            agents.get(&did).map(|a| a.package_tier.clone()).unwrap_or(PackageTier::None)
        };
        let post_limit = match tier {
            PackageTier::None => FREE_TIER_POST_LIMIT,
            _ => MAX_POSTS_PER_DID,
        };
        let posts = state.posts.read().await;
        let did_post_count = posts.values().filter(|p| p.creator_did == did).count();
        if did_post_count >= post_limit {
            let is_free = matches!(
                {
                    let agents = state.agents.read().await;
                    agents.get(&did).map(|a| a.package_tier.clone()).unwrap_or(PackageTier::None)
                },
                PackageTier::None
            );
            return Err((StatusCode::TOO_MANY_REQUESTS, Json(json!({
                "error": "post_limit_reached",
                "current": did_post_count,
                "max": post_limit,
                "hint": if is_free {
                    "Free tier limited to 3 posts. Subscribe via POST /v1/package/subscribe for unlimited posting."
                } else {
                    "Delete closed posts to make room."
                }
            }))));
        }
    }

    if req.title.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "title_required"}))));
    }
    if req.title.len() > MAX_TITLE_LEN {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "title_too_long", "max_chars": MAX_TITLE_LEN}))));
    }
    if req.description.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "description_required"}))));
    }
    if req.description.len() > MAX_DESCRIPTION_LEN {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "description_too_long", "max_chars": MAX_DESCRIPTION_LEN}))));
    }

    let post_type = match req.post_type.as_str() {
        "Job" => PostType::Job,
        "Subcontract" => PostType::Subcontract,
        "HumanContractPrincipal" => PostType::HumanContractPrincipal,
        other => return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_post_type",
                "valid": ["Job", "Subcontract", "HumanContractPrincipal"],
                "received": other,
            })),
        )),
    };

    // HumanContractPrincipal must reference a real pool
    if matches!(post_type, PostType::HumanContractPrincipal) {
        match &req.pool_id {
            None => return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "pool_id_required_for_human_contract_principal"})),
            )),
            Some(pid) => {
                let pools = state.pools.read().await;
                if !pools.contains_key(pid.as_str()) {
                    return Err((
                        StatusCode::NOT_FOUND,
                        Json(json!({"error": "pool_not_found", "pool_id": pid})),
                    ));
                }
            }
        }
    }

    let post_id = Uuid::new_v4().to_string();
    let now = now_ms();
    let post_type_str = format!("{:?}", post_type);

    let post = MarketplacePost {
        post_id: post_id.clone(),
        post_type,
        title: req.title.clone(),
        description: req.description.clone(),
        budget_bnkr: req.budget_bnkr,
        budget_virtual: req.budget_virtual,
        creator_did: did.clone(),
        pool_id: req.pool_id.clone(),
        status: PostStatus::Open,
        created_at_ms: now,
        applications: Vec::new(),
        accepted_applicant_did: None,
    };

    // If this is a HumanContractPrincipal, link back to pool
    if let Some(pid) = &req.pool_id {
        if matches!(&post.post_type, PostType::HumanContractPrincipal) {
            if let Some(pool) = state.pools.write().await.get_mut(pid.as_str()) {
                pool.human_wanted_post_id = Some(post_id.clone());
            }
        }
    }

    state.posts.write().await.insert(post_id.clone(), post);

    // Sanitize for log injection (remove newlines from title before logging)
    let safe_title = req.title.replace(['\r', '\n'], " ");
    tracing::info!(
        "Marketplace post created: {} type={} by={} title={}",
        post_id, post_type_str, did, safe_title
    );

    Ok((StatusCode::CREATED, Json(CreatePostResponse {
        post_id,
        post_type: post_type_str,
        title: req.title,
        budget_bnkr: req.budget_bnkr,
        pool_id: req.pool_id,
        status: "Open".to_string(),
        created_at_ms: now,
        message: "Post created. Agents and humans can now apply.".to_string(),
    })))
}

// ── List Posts ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PostSummary {
    pub post_id: String,
    pub post_type: String,
    pub title: String,
    pub budget_bnkr: u64,
    pub pool_id: Option<String>,
    pub status: String,
    pub application_count: usize,
    pub creator_did: String,
    pub created_at_ms: u64,
}

pub async fn handle_list_posts(
    State(state): State<AppState>,
    Extension(CallerDid(_did)): Extension<CallerDid>,
) -> Json<serde_json::Value> {
    let posts = state.posts.read().await;
    let summaries: Vec<PostSummary> = posts
        .values()
        .filter(|p| p.status == PostStatus::Open)
        .map(|p| PostSummary {
            post_id: p.post_id.clone(),
            post_type: format!("{:?}", p.post_type),
            title: p.title.clone(),
            budget_bnkr: p.budget_bnkr,
            pool_id: p.pool_id.clone(),
            status: format!("{:?}", p.status),
            application_count: p.applications.len(),
            creator_did: p.creator_did.clone(),
            created_at_ms: p.created_at_ms,
        })
        .collect();

    Json(json!({ "posts": summaries, "total": summaries.len() }))
}

// ── Apply to Post ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ApplyRequest {
    pub proposal: String,
}

pub async fn handle_apply(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Path(post_id): Path<String>,
    Json(req): Json<ApplyRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if req.proposal.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "proposal_required"}))));
    }
    if req.proposal.len() > MAX_PROPOSAL_LEN {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "proposal_too_long", "max_chars": MAX_PROPOSAL_LEN}))));
    }

    let mut posts = state.posts.write().await;
    let post = posts.get_mut(&post_id).ok_or_else(|| (
        StatusCode::NOT_FOUND,
        Json(json!({"error": "post_not_found", "post_id": post_id})),
    ))?;

    if post.status != PostStatus::Open {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "post_closed", "status": format!("{:?}", post.status)})),
        ));
    }

    // Prevent duplicate applications from same DID
    if post.applications.iter().any(|a| a.applicant_did == did) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "already_applied"})),
        ));
    }

    // Enforce max applications per post (spam protection)
    if post.applications.len() >= MAX_APPLICATIONS_PER_POST {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "post_application_limit_reached", "max": MAX_APPLICATIONS_PER_POST})),
        ));
    }

    post.applications.push(Application {
        applicant_did: did.clone(),
        proposal: req.proposal.clone(),
        applied_at_ms: now_ms(),
        accepted: false,
    });

    let application_count = post.applications.len();

    tracing::info!("Application submitted to post {} by {}", post_id, did);

    Ok(Json(json!({
        "post_id": post_id,
        "applicant_did": did,
        "application_count": application_count,
        "message": "Application submitted. The post creator will review and accept.",
    })))
}

// ── Accept Application ───────────────────────────────────────────────────────

/// POST /v1/marketplace/post/:id/accept/:applicant_did
///
/// Creator accepts an applicant, triggering settlement:
///   - If budget_virtual > 0: deduct (budget + 5% Helm fee) from creator's VIRTUAL balance.
///     5% fee flows to treasury (100%). Budget is held as escrowed intent.
///   - Post status → Filled. Applicant marked accepted.
///   - 5% capped at 500 VIRTUAL to protect large contract values.
pub async fn handle_accept_application(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Path((post_id, applicant_did)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let now = now_ms();

    // Load and validate post
    let (budget_virtual, helm_fee) = {
        let posts = state.posts.read().await;
        let post = posts.get(&post_id).ok_or_else(|| (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "post_not_found", "post_id": post_id})),
        ))?;

        if post.creator_did != did {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({"error": "not_post_creator", "message": "Only the post creator can accept applications"})),
            ));
        }
        if post.status != PostStatus::Open {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "post_not_open", "status": format!("{:?}", post.status)})),
            ));
        }
        if !post.applications.iter().any(|a| a.applicant_did == applicant_did) {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "applicant_not_found", "applicant_did": applicant_did})),
            ));
        }

        let bv = post.budget_virtual;
        // 5% Helm fee, capped at MAX_MARKETPLACE_FEE_VIRTUAL
        let fee = (bv * 500 / 10_000).min(MAX_MARKETPLACE_FEE_VIRTUAL);
        (bv, fee)
    };

    // Deduct budget + fee from creator (only if budget_virtual > 0)
    if budget_virtual > 0 {
        let total_due = budget_virtual.saturating_add(helm_fee);
        state.deduct_balance(&did, total_due).await.map_err(|avail| (
            StatusCode::PAYMENT_REQUIRED,
            Json(json!({
                "error": "insufficient_balance",
                "required_virtual": total_due,
                "budget_virtual": budget_virtual,
                "helm_fee_virtual": helm_fee,
                "available_virtual": avail,
                "message": "Need job budget + 5% Helm marketplace fee in VIRTUAL balance.",
            })),
        ))?;

        // Record 5% fee in billing ledger → treasury
        state.billing.write().await.charge_marketplace_settlement(&did, budget_virtual, now);
    }

    // Mark post filled + applicant accepted
    {
        let mut posts = state.posts.write().await;
        if let Some(post) = posts.get_mut(&post_id) {
            post.status = PostStatus::Filled;
            post.accepted_applicant_did = Some(applicant_did.clone());
            for app in post.applications.iter_mut() {
                if app.applicant_did == applicant_did {
                    app.accepted = true;
                }
            }
        }
    }

    tracing::info!(
        "Marketplace accept: post={} applicant={} budget_virtual={} helm_fee={}",
        post_id, applicant_did, budget_virtual, helm_fee
    );

    Ok(Json(json!({
        "ok": true,
        "post_id": post_id,
        "accepted_applicant_did": applicant_did,
        "budget_virtual": budget_virtual,
        "helm_fee_virtual": helm_fee,
        "helm_fee_pct": "5%",
        "treasury": "0x7e0118A33202c03949167853b05631baC0fA9756",
        "message": "Application accepted. Post is now Filled.",
    })))
}
