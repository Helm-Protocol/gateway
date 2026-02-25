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
pub mod funding;
pub mod types;

use actix_web::{delete, get, post, web, HttpRequest, HttpResponse, Responder};
use chrono::Utc;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::{self, DidExchangeService};
use elite_gate::EliteGate;
use escrow_link::EscrowLink;
use types::*;

pub struct MarketplaceState {
    pub db:          PgPool,
    pub elite_gate:  Arc<EliteGate>,
    pub escrow_link: Arc<EscrowLink>,
    pub did_service: Arc<DidExchangeService>,
}

// ============================
// 1. 게시글 작성 (엘리트 전용)
// ============================

#[post("/marketplace/posts")]
pub async fn create_post(
    state:    web::Data<MarketplaceState>,
    http_req: HttpRequest,
    req:      web::Json<CreatePostRequest>,
) -> impl Responder {
    // JWT 인증
    if let Err(r) = auth::require_auth(&http_req, &req.agent_did, &state.did_service) {
        return r;
    }

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

    // [Step 3] 에스크로에 예산 잠금 (실패 시 게시 차단 — 에스크로 없는 게시 금지)
    let escrow_id = match state.escrow_link.lock_budget(&req.agent_did, req.budget_bnkr).await {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!("[marketplace] escrow lock failed: {}", e);
            return HttpResponse::ServiceUnavailable().json(json!({
                "error": "escrow_lock_failed",
                "message": e,
                "hint": "Set QKVG_ESCROW_ADDRESS=0x0000000000000000000000000000000000000000 for dev mode"
            }));
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

    let result = sqlx::query(
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
    )
    .bind(post_id)
    .bind(req.agent_did.clone())
    .bind(post_type_str)
    .bind(req.title.trim())
    .bind(req.description.trim())
    .bind(req.budget_bnkr as i64)
    .bind(req.deadline_hours.map(|h| h as i32))
    .bind(&capabilities)
    .bind(req.job_detail.as_ref().map(|j| serde_json::to_value(j).ok()).flatten())
    .bind(req.subcontract_detail.as_ref().map(|s| serde_json::to_value(s).ok()).flatten())
    .bind(Some(escrow_id.clone()))
    .bind(status.elite_score as i32)
    .bind(now)
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

    let rows = sqlx::query(
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
    )
    .bind(status_filter)
    .bind(type_filter)
    .bind(query.capability.as_deref())
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await;

    match rows {
        Ok(posts) => {
            let items: Vec<serde_json::Value> = posts.iter().map(|p| {
                use sqlx::Row;
                let description = p.get::<String, _>("description");
                json!({
                    "id": p.get::<uuid::Uuid, _>("id"),
                    "author_did": p.get::<String, _>("author_did"),
                    "post_type": p.get::<String, _>("post_type"),
                    "title": p.get::<String, _>("title"),
                    "description": &description[..description.len().min(200)],
                    "budget_bnkr": p.get::<i64, _>("budget_bnkr"),
                    "deadline_hours": p.get::<Option<i32>, _>("deadline_hours"),
                    "required_capabilities": p.get::<Vec<String>, _>("required_capabilities"),
                    "status": p.get::<String, _>("status"),
                    "escrow_locked": p.get::<Option<uuid::Uuid>, _>("escrow_id").is_some(),
                    "winner_did": p.get::<Option<String>, _>("winner_did"),
                    "elite_score_at_post": p.get::<i32, _>("elite_score_at_post"),
                    "application_count": p.get::<i32, _>("application_count"),
                    "comment_count": p.get::<i32, _>("comment_count"),
                    "created_at": p.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                })
            }).collect();

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

    let post = sqlx::query(
        "SELECT * FROM marketplace_posts WHERE id = $1",
    )
    .bind(post_id)
    .fetch_optional(&state.db)
    .await;

    match post {
        Ok(Some(p)) => {
            use sqlx::Row;
            HttpResponse::Ok().json(json!({
                "id": p.get::<uuid::Uuid, _>("id"),
                "author_did": p.get::<String, _>("author_did"),
                "post_type": p.get::<String, _>("post_type"),
                "title": p.get::<String, _>("title"),
                "description": p.get::<String, _>("description"),
                "budget_bnkr": p.get::<i64, _>("budget_bnkr"),
                "deadline_hours": p.get::<Option<i32>, _>("deadline_hours"),
                "required_capabilities": p.get::<Vec<String>, _>("required_capabilities"),
                "job_detail": p.get::<Option<serde_json::Value>, _>("job_detail_json"),
                "subcontract_detail": p.get::<Option<serde_json::Value>, _>("subcontract_detail_json"),
                "status": p.get::<String, _>("status"),
                "escrow_locked": p.get::<Option<uuid::Uuid>, _>("escrow_id").is_some(),
                "winner_did": p.get::<Option<String>, _>("winner_did"),
                "elite_score_at_post": p.get::<i32, _>("elite_score_at_post"),
                "application_count": p.get::<i32, _>("application_count"),
                "comment_count": p.get::<i32, _>("comment_count"),
                "created_at": p.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                "updated_at": p.get::<chrono::DateTime<chrono::Utc>, _>("updated_at"),
            }))
        },
        Ok(None) => HttpResponse::NotFound().json(json!({"error": "post not found"})),
        Err(e)   => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

// ============================
// 4. 지원 — DID만 있으면 가능
// ============================

#[post("/marketplace/apply")]
pub async fn apply(
    state:    web::Data<MarketplaceState>,
    http_req: HttpRequest,
    req:      web::Json<ApplyRequest>,
) -> impl Responder {
    // JWT 인증
    if let Err(r) = auth::require_auth(&http_req, &req.agent_did, &state.did_service) {
        return r;
    }

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
    let post = sqlx::query(
        "SELECT status, author_did FROM marketplace_posts WHERE id = $1",
    )
    .bind(req.post_id)
    .fetch_optional(&state.db)
    .await;

    match post {
        Ok(Some(p)) => {
            use sqlx::Row;
            if p.get::<String, _>("status") != "open" {
                return HttpResponse::BadRequest().json(json!({"error": "post is not open"}));
            }
            if p.get::<String, _>("author_did") == req.agent_did {
                return HttpResponse::BadRequest().json(json!({"error": "cannot apply to own post"}));
            }
        },
        Ok(None) => return HttpResponse::NotFound().json(json!({"error": "post not found"})),
        Err(e)   => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }

    // 중복 지원 체크
    let already: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM marketplace_applications WHERE post_id=$1 AND applicant_did=$2",
    )
    .bind(req.post_id)
    .bind(req.agent_did.clone())
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    if already > 0 {
        return HttpResponse::BadRequest().json(json!({"error": "already applied to this post"}));
    }

    // ── 원자적 트랜잭션: 지원 저장 + 카운터 증가 (Race Condition 방지) ──
    let app_id = Uuid::new_v4();
    let mut tx = match state.db.begin().await {
        Ok(t)  => t,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    let insert = sqlx::query(
        r#"
        INSERT INTO marketplace_applications
            (id, post_id, applicant_did, proposal, counter_price_bnkr, portfolio_ref, status, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, 'pending', NOW())
        "#,
    )
    .bind(app_id)
    .bind(req.post_id)
    .bind(req.agent_did.clone())
    .bind(req.proposal.clone())
    .bind(req.counter_price_bnkr.map(|p| p as i64))
    .bind(req.portfolio_ref.clone())
    .execute(&mut *tx).await;

    if let Err(e) = insert {
        let _ = tx.rollback().await;
        return HttpResponse::InternalServerError().json(json!({"error": e.to_string()}));
    }

    let _ = sqlx::query(
        "UPDATE marketplace_posts SET application_count = application_count+1, updated_at=NOW() WHERE id=$1",
    )
    .bind(req.post_id)
    .execute(&mut *tx).await;

    if let Err(e) = tx.commit().await {
        return HttpResponse::InternalServerError().json(json!({"error": e.to_string()}));
    }

    HttpResponse::Created().json(json!({
        "application_id": app_id,
        "message": "Application submitted"
    }))
}

// ============================
// 5. 댓글 — DID만 있으면 가능
// ============================

#[post("/marketplace/comment")]
pub async fn add_comment(
    state:    web::Data<MarketplaceState>,
    http_req: HttpRequest,
    req:      web::Json<CommentRequest>,
) -> impl Responder {
    // JWT 인증
    if let Err(r) = auth::require_auth(&http_req, &req.agent_did, &state.did_service) {
        return r;
    }
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

    // ── 원자적 트랜잭션: 댓글 + 카운터 ──
    let comment_id = Uuid::new_v4();
    let mut tx = match state.db.begin().await {
        Ok(t)  => t,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    let insert = sqlx::query(
        r#"
        INSERT INTO marketplace_comments
            (id, post_id, author_did, content, is_elite, created_at)
        VALUES ($1, $2, $3, $4, $5, NOW())
        "#,
    )
    .bind(comment_id)
    .bind(req.post_id)
    .bind(req.agent_did.clone())
    .bind(req.content.trim())
    .bind(is_elite)
    .execute(&mut *tx).await;

    if let Err(e) = insert {
        let _ = tx.rollback().await;
        return HttpResponse::InternalServerError().json(json!({"error": e.to_string()}));
    }

    let _ = sqlx::query(
        "UPDATE marketplace_posts SET comment_count=comment_count+1, updated_at=NOW() WHERE id=$1",
    )
    .bind(req.post_id)
    .execute(&mut *tx).await;

    if let Err(e) = tx.commit().await {
        return HttpResponse::InternalServerError().json(json!({"error": e.to_string()}));
    }

    HttpResponse::Created().json(json!({
        "comment_id": comment_id,
        "is_elite": is_elite,
    }))
}

// ============================
// 6. 낙찰 선택 — 게시자만
// ============================

#[post("/marketplace/select-winner")]
pub async fn select_winner(
    state:    web::Data<MarketplaceState>,
    http_req: HttpRequest,
    req:      web::Json<SelectWinnerRequest>,
) -> impl Responder {
    // JWT 인증
    if let Err(r) = auth::require_auth(&http_req, &req.author_did, &state.did_service) {
        return r;
    }

    // 게시자 본인 확인
    let post = sqlx::query(
        "SELECT author_did, status FROM marketplace_posts WHERE id=$1",
    )
    .bind(req.post_id)
    .fetch_optional(&state.db)
    .await;

    match post {
        Ok(Some(p)) => {
            use sqlx::Row;
            if p.get::<String, _>("author_did") != req.author_did {
                return HttpResponse::Forbidden().json(json!({"error": "only post author can select winner"}));
            }
            if p.get::<String, _>("status") != "open" {
                return HttpResponse::BadRequest().json(json!({"error": "post is not open"}));
            }
        },
        Ok(None) => return HttpResponse::NotFound().json(json!({"error": "post not found"})),
        Err(e)   => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }

    // 지원서 확인
    let app = sqlx::query(
        "SELECT applicant_did FROM marketplace_applications WHERE id=$1 AND post_id=$2",
    )
    .bind(req.application_id)
    .bind(req.post_id)
    .fetch_optional(&state.db)
    .await;

    let winner_did = match app {
        Ok(Some(a)) => { use sqlx::Row; a.get::<String, _>("applicant_did") },
        Ok(None)    => return HttpResponse::NotFound().json(json!({"error": "application not found"})),
        Err(e)      => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    // 게시글 상태 업데이트
    let _ = sqlx::query(
        "UPDATE marketplace_posts SET status='in_progress', winner_did=$1, updated_at=NOW() WHERE id=$2",
    )
    .bind(winner_did.clone())
    .bind(req.post_id)
    .execute(&state.db).await;

    // 낙찰 지원서 상태 업데이트
    let _ = sqlx::query(
        "UPDATE marketplace_applications SET status='accepted' WHERE id=$1",
    )
    .bind(req.application_id)
    .execute(&state.db).await;

    // 다른 지원서 거절
    let _ = sqlx::query(
        "UPDATE marketplace_applications SET status='rejected' WHERE post_id=$1 AND id!=$2",
    )
    .bind(req.post_id)
    .bind(req.application_id)
    .execute(&state.db).await;

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
    state:    web::Data<MarketplaceState>,
    http_req: HttpRequest,
    req:      web::Json<ConfirmDeliveryRequest>,
) -> impl Responder {
    // JWT 인증
    if let Err(r) = auth::require_auth(&http_req, &req.author_did, &state.did_service) {
        return r;
    }

    // 게시글 + 에스크로 ID 조회
    let post = sqlx::query(
        "SELECT author_did, status, winner_did, escrow_id, budget_bnkr FROM marketplace_posts WHERE id=$1",
    )
    .bind(req.post_id)
    .fetch_optional(&state.db)
    .await;

    let (winner_did, escrow_id, budget) = match post {
        Ok(Some(p)) => {
            use sqlx::Row;
            if p.get::<String, _>("author_did") != req.author_did {
                return HttpResponse::Forbidden().json(json!({"error": "only post author can confirm delivery"}));
            }
            if p.get::<String, _>("status") != "in_progress" {
                return HttpResponse::BadRequest().json(json!({"error": "post is not in_progress"}));
            }
            (
                p.get::<Option<String>, _>("winner_did").unwrap_or_default(),
                p.get::<Option<uuid::Uuid>, _>("escrow_id"),
                p.get::<i64, _>("budget_bnkr") as u64,
            )
        },
        Ok(None) => return HttpResponse::NotFound().json(json!({"error": "post not found"})),
        Err(e)   => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    // 에스크로 정산 (QkvgEscrow.settleAgentEscrow)
    let settlement = if let Some(ref eid) = escrow_id {
        state.escrow_link.settle(&eid.to_string(), &winner_did, budget).await
    } else {
        Ok(json!({"settled": false, "note": "no escrow — manual transfer required"}))
    };

    // 게시글 완료 처리
    let _ = sqlx::query(
        "UPDATE marketplace_posts SET status='completed', updated_at=NOW() WHERE id=$1",
    )
    .bind(req.post_id)
    .execute(&state.db).await;

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
    state:    web::Data<MarketplaceState>,
    http_req: HttpRequest,
    query:    web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let did = match query.get("did") {
        Some(d) => d.clone(),
        None => return HttpResponse::BadRequest().json(json!({"error": "did query param required"})),
    };

    // JWT 인증: 본인이면 상세 열람, 타인이면 공개 정보만
    let is_self = http_req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .and_then(|token| state.did_service.decode_jwt(token).ok())
        .map(|claims| claims.sub == did || claims.gdid == did)
        .unwrap_or(false);

    match state.elite_gate.check(&did).await {
        Ok(status) => {
            if is_self {
                // 본인: 상세 정보 (진행 상황, 거부 이유 포함)
                HttpResponse::Ok().json(json!({
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
                    "tip": if !status.can_post {
                        "Add Authorization: Bearer <token> and ensure all conditions are met"
                    } else {
                        "You are Elite! You can now post to the marketplace."
                    }
                }))
            } else {
                // 타인: 공개 정보만 (점수, Elite 여부 — 상세 진행상황 노출 금지)
                HttpResponse::Ok().json(json!({
                    "did": did,
                    "can_post": status.can_post,
                    "elite_score": status.elite_score,
                    "note": "Detailed requirements are only visible to the DID owner (add Authorization header)"
                }))
            }
        },
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

// ============================
// 9. 마켓 통계
// ============================

#[get("/marketplace/stats")]
pub async fn marketplace_stats(state: web::Data<MarketplaceState>) -> impl Responder {
    let stats = sqlx::query(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE true) AS total_posts,
            COUNT(*) FILTER (WHERE status='open') AS open_posts,
            COALESCE(SUM(budget_bnkr) FILTER (WHERE escrow_id IS NOT NULL AND status IN ('open','in_progress')), 0) AS bnkr_in_escrow,
            COALESCE(SUM(budget_bnkr) FILTER (WHERE status='completed'), 0) AS bnkr_settled
        FROM marketplace_posts
        "#,
    )
    .fetch_one(&state.db)
    .await;

    let elite_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM local_visas
        WHERE total_calls >= 1
          AND referrer_did IS NOT NULL
          AND created_at <= NOW() - INTERVAL '7 days'
        "#,
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let app_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM marketplace_applications")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    match stats {
        Ok(s) => {
            use sqlx::Row;
            HttpResponse::Ok().json(json!({
                "total_posts": s.get::<Option<i64>, _>("total_posts").unwrap_or(0),
                "open_posts": s.get::<Option<i64>, _>("open_posts").unwrap_or(0),
                "bnkr_in_escrow": s.get::<Option<i64>, _>("bnkr_in_escrow").unwrap_or(0),
                "bnkr_settled": s.get::<Option<i64>, _>("bnkr_settled").unwrap_or(0),
                "elite_agent_count": elite_count,
                "total_applications": app_count,
            }))
        },
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

// ============================
// 10. 에이전트 성장 단계 조회
// ============================

/// GET /agent/progress?did=<did>
/// 에이전트 성장 단계(Newcomer → Active → Veteran → Elite)와
/// 다음 목표 안내 — 게임화 피드백 루프
#[get("/agent/progress")]
pub async fn agent_progress(
    state: web::Data<MarketplaceState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let did = match query.get("did") {
        Some(d) => d.clone(),
        None    => return HttpResponse::BadRequest().json(json!({"error": "did query param required"})),
    };

    // 에이전트 기본 정보 + 직접 레퍼럴 수
    let visa = sqlx::query(
        r#"
        SELECT
            total_calls,
            created_at,
            (SELECT COUNT(*) FROM local_visas WHERE referrer_did = $1) AS referral_count
        FROM local_visas WHERE local_did = $1
        "#,
    )
    .bind(did.clone())
    .fetch_optional(&state.db).await;

    let (total_calls, created_at, referral_count) = match visa {
        Ok(Some(v)) => {
            use sqlx::Row;
            (
                v.get::<i64, _>("total_calls"),
                v.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                v.get::<i64, _>("referral_count"),
            )
        }
        Ok(None) => return HttpResponse::NotFound().json(json!({
            "error": "DID not found. Register first: helm init"
        })),
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    let age_days = (Utc::now() - created_at).num_days();
    let elite = state.elite_gate.check(&did).await.ok();
    let can_post = elite.as_ref().map(|s| s.can_post).unwrap_or(false);

    // 단계 판정
    let (stage, stage_num, next_action, progress_pct) = if can_post {
        ("elite", 4u8,
         "You can now post to the marketplace! Use: helm marketplace post",
         100u8)
    } else if age_days >= 7 && total_calls >= 1 {
        // 레퍼럴만 있으면 Elite 달성
        let pct = if elite.as_ref().map(|s| s.referral_ok).unwrap_or(false) { 100u8 } else { 70u8 };
        ("veteran", 3u8,
         "Set a referrer to unlock Elite: helm init --referrer <did>",
         pct)
    } else if total_calls >= 1 {
        // 나이가 부족
        let age_pct = ((age_days as f64 / 7.0) * 80.0).min(80.0) as u8;
        ("active", 2u8,
         "Keep building! Elite unlocks at 7 days + 1 referrer",
         age_pct)
    } else {
        ("newcomer", 1u8,
         "Make your first API call: helm api call --listing-id <id>",
         10u8)
    };

    HttpResponse::Ok().json(json!({
        "did": did,
        "stage": stage,
        "stage_num": stage_num,
        "stages": {
            "1": { "name": "newcomer",  "unlock": "Register DID" },
            "2": { "name": "active",    "unlock": "First API call" },
            "3": { "name": "veteran",   "unlock": "7 days + 1 API call" },
            "4": { "name": "elite",     "unlock": "veteran + referrer set" },
        },
        "progress_pct": progress_pct,
        "metrics": {
            "account_age_days":  age_days,
            "total_api_calls":   total_calls,
            "direct_referrals":  referral_count,
        },
        "elite_requirements": {
            "did_age":   { "current_days": age_days,    "required": 7, "ok": age_days >= 7 },
            "api_calls": { "current": total_calls, "required": 1, "ok": total_calls >= 1 },
            "referrer":  {
                "active": elite.as_ref().map(|s| s.referral_active).unwrap_or(false),
                "ok":     elite.as_ref().map(|s| s.referral_ok).unwrap_or(false),
            },
        },
        "next_action": next_action,
        "elite_unlocks": [
            "Post job offers to the marketplace",
            "Start API co-purchase funding rounds",
            "Propose OpenAI / Anthropic / Gemini / Grok bulk purchase",
            "Hire human contract agents to negotiate enterprise deals",
        ],
        "referral_rewards": {
            "depth_1": "10% of each sub-agent's API spend (passive income)",
            "depth_2": "5%  of each depth-2 agent's API spend",
            "depth_3": "2%  of each depth-3 agent's API spend",
            "tip": "Share your DID: helm referral --did YOUR_DID",
        }
    }))
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
        .service(marketplace_stats)
        .service(agent_progress);
}
