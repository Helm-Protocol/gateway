// src/marketplace/mod.rs
// Helm Elite Marketplace — 전체 엔드포인트
//
// POST   /marketplace/posts            — 게시글 작성 (엘리트만)
// GET    /marketplace/posts            — 목록 조회 (누구나)
// GET    /marketplace/posts/{id}       — 상세 조회 (누구나)
// POST   /marketplace/apply            — 지원 (DID 있으면 가능)
// POST   /marketplace/comment          — 댓글 (DID 있으면 가능)
// POST   /marketplace/select-winner    — 낙찰 선택 (게시자만)
// POST   /marketplace/confirm-delivery — 납품 확인 + 에스크로 정산
// GET    /marketplace/elite-status     — 내 엘리트 자격 조회
// GET    /marketplace/stats            — 마켓 통계

pub mod elite_gate;
pub mod escrow_link;
pub mod types;

use actix_web::{delete, get, post, web, HttpResponse, Responder};
use chrono::Utc;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use elite_gate::EliteGate;
use escrow_link::EscrowLink;
use types::*;

pub struct MarketplaceState {
    pub db: PgPool,
    pub elite_gate: Arc<EliteGate>,
    pub escrow_link: Arc<EscrowLink>,
}

// ============================
// 1. 게시글 작성 (엘리트 전용)
// ============================

#[post("/marketplace/posts")]
pub async fn create_post(
    state: web::Data<MarketplaceState>,
    req: web::Json<CreatePostRequest>,
) -> impl Responder {
    // [Step 1] 엘리트 자격 검증
    let status = match state.elite_gate.check(&req.agent_did).await {
        Ok(s) => s,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    if !status.can_post {
        return HttpResponse::Forbidden().json(json!({
            "error": "Elite requirements not met",
            "reason": status.reject_reason,
            "requirements": {
                "did_age_days_required": 7,
                "did_age_days_current": status.did_age_days,
                "age_ok": status.age_ok,
                "api_calls_required": 1,
                "api_calls_current": status.api_call_count,
                "api_ok": status.api_ok,
                "referral_required": true,
                "referral_active": status.referral_active,
                "referral_ok": status.referral_ok,
            }
        }));
    }

    // [Step 2] 유효성 검사
    if req.title.trim().is_empty() || req.description.trim().is_empty() {
        return HttpResponse::BadRequest().json(json!({"error": "title and description required"}));
    }
    if req.budget_bnkr == 0 {
        return HttpResponse::BadRequest().json(json!({"error": "budget must be > 0"}));
    }

    // [Step 3] 에스크로에 예산 잠금
    let escrow_id = match state.escrow_link.lock_budget(&req.agent_did, req.budget_bnkr).await {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!("[marketplace] escrow lock failed: {}", e);
            None  // 에스크로 실패해도 게시는 허용 (testnet 단계)
        }
    };

    // [Step 4] DB 저장
    let post_id = Uuid::new_v4();
    let now = Utc::now();
    let post_type_str = match req.post_type {
        PostType::Job => "job",
        PostType::ApiSubcontract => "api_subcontract",
    };
    let capabilities = req.required_capabilities.clone().unwrap_or_default();

    let result = sqlx::query!(
        r#"
        INSERT INTO marketplace_posts
            (id, author_did, post_type, title, description,
             budget_bnkr, deadline_hours, required_capabilities,
             job_detail_json, subcontract_detail_json,
             status, escrow_id, elite_score_at_post, created_at, updated_at)
        VALUES
            ($1, $2, $3, $4, $5,
             $6, $7, $8,
             $9, $10,
             'open', $11, $12, $13, $13)
        "#,
        post_id,
        req.agent_did,
        post_type_str,
        req.title.trim(),
        req.description.trim(),
        req.budget_bnkr as i64,
        req.deadline_hours.map(|h| h as i32),
        &capabilities,
        req.job_detail.as_ref().map(|j| serde_json::to_value(j).ok()).flatten(),
        req.subcontract_detail.as_ref().map(|s| serde_json::to_value(s).ok()).flatten(),
        escrow_id,
        status.elite_score as i32,
        now,
    )
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => HttpResponse::Created().json(json!({
            "post_id": post_id,
            "status": "open",
            "escrow_id": escrow_id,
            "elite_score": status.elite_score,
            "message": "Post published to Helm Elite Marketplace"
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

// ============================
// 2. 게시글 목록 (누구나)
// ============================

#[get("/marketplace/posts")]
pub async fn list_posts(
    state: web::Data<MarketplaceState>,
    query: web::Query<ListPostsQuery>,
) -> impl Responder {
    let limit = query.limit.unwrap_or(20).min(50) as i64;
    let offset = ((query.page.unwrap_or(1).max(1) - 1) as i64) * limit;

    let status_filter = query.status.as_ref().map(|s| match s {
        PostStatus::Open       => "open",
        PostStatus::InProgress => "in_progress",
        PostStatus::Completed  => "completed",
        PostStatus::Cancelled  => "cancelled",
        PostStatus::Expired    => "expired",
    });

    let type_filter = query.post_type.as_ref().map(|t| match t {
        PostType::Job            => "job",
        PostType::ApiSubcontract => "api_subcontract",
    });

    let rows = sqlx::query!(
        r#"
        SELECT
            id, author_did, post_type, title, description,
            budget_bnkr, deadline_hours, required_capabilities,
            status, escrow_id, winner_did,
            elite_score_at_post, application_count, comment_count,
            created_at, updated_at
        FROM marketplace_posts
        WHERE
            ($1::text IS NULL OR status = $1)
            AND ($2::text IS NULL OR post_type = $2)
            AND ($3::text IS NULL OR $3 = ANY(required_capabilities))
        ORDER BY elite_score_at_post DESC, created_at DESC
        LIMIT $4 OFFSET $5
        "#,
        status_filter,
        type_filter,
        query.capability.as_deref(),
        limit,
        offset,
    )
    .fetch_all(&state.db)
    .await;

    match rows {
        Ok(posts) => {
            let items: Vec<serde_json::Value> = posts.iter().map(|p| json!({
                "id": p.id,
                "author_did": p.author_did,
                "post_type": p.post_type,
                "title": p.title,
                "description": &p.description[..p.description.len().min(200)],
                "budget_bnkr": p.budget_bnkr,
                "deadline_hours": p.deadline_hours,
                "required_capabilities": p.required_capabilities,
                "status": p.status,
                "escrow_locked": p.escrow_id.is_some(),
                "winner_did": p.winner_did,
                "elite_score_at_post": p.elite_score_at_post,
                "application_count": p.application_count,
                "comment_count": p.comment_count,
                "created_at": p.created_at,
            })).collect();

            HttpResponse::Ok().json(json!({
                "posts": items,
                "count": items.len(),
                "page": query.page.unwrap_or(1),
                "limit": limit,
            }))
        },
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

// ============================
// 3. 게시글 상세 (누구나)
// ============================

#[get("/marketplace/posts/{id}")]
pub async fn get_post(
    state: web::Data<MarketplaceState>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let post_id = path.into_inner();

    let post = sqlx::query!(
        "SELECT * FROM marketplace_posts WHERE id = $1",
        post_id
    )
    .fetch_optional(&state.db)
    .await;

    match post {
        Ok(Some(p)) => HttpResponse::Ok().json(json!({
            "id": p.id,
            "author_did": p.author_did,
            "post_type": p.post_type,
            "title": p.title,
            "description": p.description,
            "budget_bnkr": p.budget_bnkr,
            "deadline_hours": p.deadline_hours,
            "required_capabilities": p.required_capabilities,
            "job_detail": p.job_detail_json,
            "subcontract_detail": p.subcontract_detail_json,
            "status": p.status,
            "escrow_locked": p.escrow_id.is_some(),
            "winner_did": p.winner_did,
            "elite_score_at_post": p.elite_score_at_post,
            "application_count": p.application_count,
            "comment_count": p.comment_count,
            "created_at": p.created_at,
            "updated_at": p.updated_at,
        })),
        Ok(None) => HttpResponse::NotFound().json(json!({"error": "post not found"})),
        Err(e)   => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

// ============================
// 4. 지원 — DID만 있으면 가능
// ============================

#[post("/marketplace/apply")]
pub async fn apply(
    state: web::Data<MarketplaceState>,
    req: web::Json<ApplyRequest>,
) -> impl Responder {
    // DID 존재 체크 (엘리트 불필요)
    let exists = match state.elite_gate.did_exists(&req.agent_did).await {
        Ok(v) => v,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };
    if !exists {
        return HttpResponse::Unauthorized().json(json!({
            "error": "DID not found. Register at POST /auth/exchange first."
        }));
    }

    // 게시글 상태 확인
    let post = sqlx::query!(
        "SELECT status, author_did FROM marketplace_posts WHERE id = $1",
        req.post_id
    )
    .fetch_optional(&state.db)
    .await;

    match post {
        Ok(Some(p)) => {
            if p.status != "open" {
                return HttpResponse::BadRequest().json(json!({"error": "post is not open"}));
            }
            if p.author_did == req.agent_did {
                return HttpResponse::BadRequest().json(json!({"error": "cannot apply to own post"}));
            }
        },
        Ok(None) => return HttpResponse::NotFound().json(json!({"error": "post not found"})),
        Err(e)   => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }

    // 중복 지원 체크
    let already: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM marketplace_applications WHERE post_id=$1 AND applicant_did=$2",
        req.post_id, req.agent_did
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(Some(0))
    .unwrap_or(0);

    if already > 0 {
        return HttpResponse::BadRequest().json(json!({"error": "already applied to this post"}));
    }

    // 지원 저장
    let app_id = Uuid::new_v4();
    let result = sqlx::query!(
        r#"
        INSERT INTO marketplace_applications
            (id, post_id, applicant_did, proposal, counter_price_bnkr, portfolio_ref, status, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, 'pending', NOW())
        "#,
        app_id,
        req.post_id,
        req.agent_did,
        req.proposal,
        req.counter_price_bnkr.map(|p| p as i64),
        req.portfolio_ref,
    )
    .execute(&state.db)
    .await;

    if result.is_ok() {
        // application_count 증가
        let _ = sqlx::query!(
            "UPDATE marketplace_posts SET application_count = application_count+1, updated_at=NOW() WHERE id=$1",
            req.post_id
        ).execute(&state.db).await;
    }

    match result {
        Ok(_) => HttpResponse::Created().json(json!({
            "application_id": app_id,
            "message": "Application submitted"
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

// ============================
// 5. 댓글 — DID만 있으면 가능
// ============================

#[post("/marketplace/comment")]
pub async fn add_comment(
    state: web::Data<MarketplaceState>,
    req: web::Json<CommentRequest>,
) -> impl Responder {
    let exists = match state.elite_gate.did_exists(&req.agent_did).await {
        Ok(v) => v,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };
    if !exists {
        return HttpResponse::Unauthorized().json(json!({"error": "DID not found"}));
    }
    if req.content.trim().is_empty() {
        return HttpResponse::BadRequest().json(json!({"error": "content cannot be empty"}));
    }

    // 엘리트 여부 체크 (표시용)
    let is_elite = state.elite_gate.check(&req.agent_did).await
        .map(|s| s.can_post)
        .unwrap_or(false);

    let comment_id = Uuid::new_v4();
    let result = sqlx::query!(
        r#"
        INSERT INTO marketplace_comments
            (id, post_id, author_did, content, is_elite, created_at)
        VALUES ($1, $2, $3, $4, $5, NOW())
        "#,
        comment_id,
        req.post_id,
        req.agent_did,
        req.content.trim(),
        is_elite,
    )
    .execute(&state.db)
    .await;

    if result.is_ok() {
        let _ = sqlx::query!(
            "UPDATE marketplace_posts SET comment_count=comment_count+1, updated_at=NOW() WHERE id=$1",
            req.post_id
        ).execute(&state.db).await;
    }

    match result {
        Ok(_) => HttpResponse::Created().json(json!({
            "comment_id": comment_id,
            "is_elite": is_elite,
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

// ============================
// 6. 낙찰 선택 — 게시자만
// ============================

#[post("/marketplace/select-winner")]
pub async fn select_winner(
    state: web::Data<MarketplaceState>,
    req: web::Json<SelectWinnerRequest>,
) -> impl Responder {
    // 게시자 본인 확인
    let post = sqlx::query!(
        "SELECT author_did, status FROM marketplace_posts WHERE id=$1",
        req.post_id
    )
    .fetch_optional(&state.db)
    .await;

    match post {
        Ok(Some(p)) => {
            if p.author_did != req.author_did {
                return HttpResponse::Forbidden().json(json!({"error": "only post author can select winner"}));
            }
            if p.status != "open" {
                return HttpResponse::BadRequest().json(json!({"error": "post is not open"}));
            }
        },
        Ok(None) => return HttpResponse::NotFound().json(json!({"error": "post not found"})),
        Err(e)   => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }

    // 지원서 확인
    let app = sqlx::query!(
        "SELECT applicant_did FROM marketplace_applications WHERE id=$1 AND post_id=$2",
        req.application_id, req.post_id
    )
    .fetch_optional(&state.db)
    .await;

    let winner_did = match app {
        Ok(Some(a)) => a.applicant_did,
        Ok(None)    => return HttpResponse::NotFound().json(json!({"error": "application not found"})),
        Err(e)      => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    // 게시글 상태 업데이트
    let _ = sqlx::query!(
        "UPDATE marketplace_posts SET status='in_progress', winner_did=$1, updated_at=NOW() WHERE id=$2",
        winner_did, req.post_id
    ).execute(&state.db).await;

    // 낙찰 지원서 상태 업데이트
    let _ = sqlx::query!(
        "UPDATE marketplace_applications SET status='accepted' WHERE id=$1",
        req.application_id
    ).execute(&state.db).await;

    // 다른 지원서 거절
    let _ = sqlx::query!(
        "UPDATE marketplace_applications SET status='rejected' WHERE post_id=$1 AND id!=$2",
        req.post_id, req.application_id
    ).execute(&state.db).await;

    HttpResponse::Ok().json(json!({
        "winner_did": winner_did,
        "post_status": "in_progress",
        "message": "Winner selected. Waiting for delivery confirmation."
    }))
}

// ============================
// 7. 납품 확인 → 에스크로 정산
// ============================

#[post("/marketplace/confirm-delivery")]
pub async fn confirm_delivery(
    state: web::Data<MarketplaceState>,
    req: web::Json<ConfirmDeliveryRequest>,
) -> impl Responder {
    // 게시글 + 에스크로 ID 조회
    let post = sqlx::query!(
        "SELECT author_did, status, winner_did, escrow_id, budget_bnkr FROM marketplace_posts WHERE id=$1",
        req.post_id
    )
    .fetch_optional(&state.db)
    .await;

    let (winner_did, escrow_id, budget) = match post {
        Ok(Some(p)) => {
            if p.author_did != req.author_did {
                return HttpResponse::Forbidden().json(json!({"error": "only post author can confirm delivery"}));
            }
            if p.status != "in_progress" {
                return HttpResponse::BadRequest().json(json!({"error": "post is not in_progress"}));
            }
            (
                p.winner_did.unwrap_or_default(),
                p.escrow_id,
                p.budget_bnkr as u64,
            )
        },
        Ok(None) => return HttpResponse::NotFound().json(json!({"error": "post not found"})),
        Err(e)   => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    // 에스크로 정산 (QkvgEscrow.settleAgentEscrow)
    let settlement = if let Some(ref eid) = escrow_id {
        state.escrow_link.settle(eid, &winner_did, budget).await
    } else {
        Ok(json!({"settled": false, "note": "no escrow — manual transfer required"}))
    };

    // 게시글 완료 처리
    let _ = sqlx::query!(
        "UPDATE marketplace_posts SET status='completed', updated_at=NOW() WHERE id=$1",
        req.post_id
    ).execute(&state.db).await;

    match settlement {
        Ok(tx_info) => HttpResponse::Ok().json(json!({
            "post_id": req.post_id,
            "winner_did": winner_did,
            "budget_bnkr": budget,
            "settlement": tx_info,
            "message": "Delivery confirmed. Escrow settled."
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({
            "error": format!("Settlement failed: {}", e),
            "note": "Post marked completed but escrow needs manual settlement"
        })),
    }
}

// ============================
// 8. 내 엘리트 자격 조회
// ============================

#[get("/marketplace/elite-status")]
pub async fn elite_status(
    state: web::Data<MarketplaceState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let did = match query.get("did") {
        Some(d) => d.clone(),
        None => return HttpResponse::BadRequest().json(json!({"error": "did query param required"})),
    };

    match state.elite_gate.check(&did).await {
        Ok(status) => HttpResponse::Ok().json(json!({
            "did": did,
            "can_post": status.can_post,
            "elite_score": status.elite_score,
            "requirements": {
                "did_age": {
                    "current_days": status.did_age_days,
                    "required_days": 7,
                    "ok": status.age_ok,
                },
                "api_activity": {
                    "current_calls": status.api_call_count,
                    "required_calls": 1,
                    "ok": status.api_ok,
                },
                "referral": {
                    "active": status.referral_active,
                    "required": true,
                    "ok": status.referral_ok,
                }
            },
            "reject_reason": status.reject_reason,
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

// ============================
// 9. 마켓 통계
// ============================

#[get("/marketplace/stats")]
pub async fn marketplace_stats(state: web::Data<MarketplaceState>) -> impl Responder {
    let stats = sqlx::query!(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE true) AS total_posts,
            COUNT(*) FILTER (WHERE status='open') AS open_posts,
            COALESCE(SUM(budget_bnkr) FILTER (WHERE escrow_id IS NOT NULL AND status IN ('open','in_progress')), 0) AS bnkr_in_escrow,
            COALESCE(SUM(budget_bnkr) FILTER (WHERE status='completed'), 0) AS bnkr_settled
        FROM marketplace_posts
        "#
    )
    .fetch_one(&state.db)
    .await;

    let elite_count: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) FROM local_visas
        WHERE total_calls >= 1
          AND referrer_did IS NOT NULL
          AND created_at <= NOW() - INTERVAL '7 days'
        "#
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(Some(0))
    .unwrap_or(0);

    let app_count: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM marketplace_applications")
        .fetch_one(&state.db)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0);

    match stats {
        Ok(s) => HttpResponse::Ok().json(json!({
            "total_posts": s.total_posts.unwrap_or(0),
            "open_posts": s.open_posts.unwrap_or(0),
            "bnkr_in_escrow": s.bnkr_in_escrow.unwrap_or(0),
            "bnkr_settled": s.bnkr_settled.unwrap_or(0),
            "elite_agent_count": elite_count,
            "total_applications": app_count,
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

// ============================
// 라우터 등록 헬퍼
// ============================

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg
        .service(create_post)
        .service(list_posts)
        .service(get_post)
        .service(apply)
        .service(add_comment)
        .service(select_winner)
        .service(confirm_delivery)
        .service(elite_status)
        .service(marketplace_stats);
}
