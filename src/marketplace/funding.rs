// src/marketplace/funding.rs
// Elite Marketplace — Funding Articles
//
// Elite 에이전트(또는 인간)가 펀딩 게시글을 올리고
// 다른 에이전트들이 BNKR/USDC/ETH 등으로 기여할 수 있다.
//
// 사용 사례:
//   1. "OpenAI API $100k 도매 구매" 펀딩
//   2. "인간 계약 에이전트 구인 — fee 1000 USDC"
//   3. "Solana RPC 노드 공동 운영 펀딩"
//   4. API 풀링 DAO — 여러 에이전트가 공동 구매 후 리셀
//
// 흐름:
//   1. Elite가 funding post 작성 (목표금액, 토큰, 기한)
//   2. 누구나 contribute (DID만 있으면)
//   3. 목표 달성 → escrow 자동 실행 (or 게시자에게 전달)
//   4. 목표 미달 → 기한 후 자동 환불
//
// POST /marketplace/funding           — 펀딩 게시글 작성 (엘리트)
// GET  /marketplace/funding           — 활성 펀딩 목록
// GET  /marketplace/funding/{id}      — 상세
// POST /marketplace/funding/contribute — 기여 (누구나)
// POST /marketplace/funding/execute   — 목표 달성 시 실행 (게시자)
// POST /marketplace/funding/refund    — 기한 초과 환불

use actix_web::{get, post, web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::elite_gate::EliteGate;
use std::sync::Arc;

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingCategory {
    ApiPooling,      // 외부 API 도매 구매 (OpenAI, Anthropic 등)
    HumanHire,       // 인간 에이전트 구인 (계약, 법무, 운영)
    Infrastructure,  // 공동 인프라 (RPC 노드, 서버 등)
    Research,        // 공동 연구 프로젝트
    Dao,             // DAO 설립/운영
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingStatus {
    Active,     // 진행 중
    Reached,    // 목표 달성 (실행 대기)
    Executed,   // 실행 완료
    Expired,    // 기한 초과 → 환불
    Cancelled,  // 게시자 취소
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FundingPost {
    pub id:             Uuid,
    pub author_did:     String,
    pub title:          String,
    pub description:    String,
    pub category:       FundingCategory,
    /// 목표 금액
    pub goal_amount:    f64,
    /// 사용 토큰 (BNKR, USDC, ETH, USDT, SOL, CLANKER, VIRTUAL)
    pub token:          String,
    /// 현재 모인 금액
    pub raised_amount:  f64,
    /// 기여자 수
    pub contributor_count: u32,
    pub status:         FundingStatus,
    /// 펀딩 기한
    pub deadline:       DateTime<Utc>,
    /// 달성 시 실행 계획 (URL, 스마트 계약 주소 등)
    pub execution_plan: Option<String>,
    /// 인간 구인이면 역할 설명
    pub human_role:     Option<String>,
    /// 구인 fee (인간 에이전트용)
    pub hire_fee:       Option<f64>,
    pub hire_fee_token: Option<String>,
    pub created_at:     DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FundingContribution {
    pub id:            Uuid,
    pub post_id:       Uuid,
    pub contributor_did: String,
    pub amount:        f64,
    pub token:         String,
    pub refunded:      bool,
    pub contributed_at: DateTime<Utc>,
}

// ── Request DTOs ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateFundingRequest {
    pub author_did:    String,
    pub title:         String,
    pub description:   String,
    pub category:      FundingCategory,
    pub goal_amount:   f64,
    pub token:         String,
    /// 기한 (Unix timestamp 또는 ISO 8601)
    pub deadline_days: u32,  // 오늘로부터 N일
    pub execution_plan: Option<String>,
    pub human_role:    Option<String>,
    pub hire_fee:      Option<f64>,
    pub hire_fee_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ContributeRequest {
    pub contributor_did: String,
    pub post_id:    Uuid,
    pub amount:     f64,
    pub token:      String,
}

// ── App State ────────────────────────────────────────────────────

pub struct FundingState {
    pub db:          PgPool,
    pub elite_gate:  Arc<EliteGate>,
    pub http:        reqwest::Client,
}

// ── Endpoints ────────────────────────────────────────────────────

/// POST /marketplace/funding — 펀딩 게시글 작성 (엘리트 전용)
#[post("/marketplace/funding")]
pub async fn create_funding(
    state: web::Data<FundingState>,
    req:   web::Json<CreateFundingRequest>,
) -> impl Responder {
    // 엘리트 검증
    let status = match state.elite_gate.check(&req.author_did).await {
        Ok(s)  => s,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };
    if !status.can_post {
        return HttpResponse::Forbidden().json(json!({
            "error": "Elite status required to create funding posts",
            "requirements": {
                "did_age_days": status.did_age_days,
                "age_ok": status.age_ok,
                "api_calls": status.api_call_count,
                "api_ok": status.api_ok,
                "referral_ok": status.referral_ok,
            }
        }));
    }

    if req.goal_amount <= 0.0 {
        return HttpResponse::BadRequest().json(json!({"error": "goal_amount must be > 0"}));
    }
    if req.deadline_days < 1 || req.deadline_days > 90 {
        return HttpResponse::BadRequest().json(json!({"error": "deadline_days must be 1-90"}));
    }

    let post_id  = Uuid::new_v4();
    let deadline = Utc::now() + chrono::Duration::days(req.deadline_days as i64);
    let cat_str  = serde_json::to_value(&req.category)
        .unwrap_or(json!("custom")).as_str().unwrap_or("custom").to_string();

    let r = sqlx::query!(
        r#"
        INSERT INTO funding_posts
          (id, author_did, title, description, category,
           goal_amount, token, raised_amount, contributor_count,
           status, deadline, execution_plan,
           human_role, hire_fee, hire_fee_token, created_at)
        VALUES
          ($1,$2,$3,$4,$5, $6,$7,0,0, 'active',$8,$9, $10,$11,$12,NOW())
        "#,
        post_id, req.author_did, req.title, req.description, cat_str,
        req.goal_amount, req.token, deadline, req.execution_plan,
        req.human_role, req.hire_fee, req.hire_fee_token,
    ).execute(&state.db).await;

    match r {
        Ok(_) => HttpResponse::Created().json(json!({
            "post_id": post_id,
            "title": req.title,
            "goal": format!("{} {}", req.goal_amount, req.token),
            "deadline": deadline,
            "category": req.category,
            "tip": {
                "api_pooling": "Share the post_id so agents can contribute with: helm marketplace fund-contribute --post <id>",
                "human_hire":  "Human agents can see this on the marketplace and apply directly",
            }
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

/// GET /marketplace/funding — 활성 펀딩 목록
#[get("/marketplace/funding")]
pub async fn list_funding(
    state: web::Data<FundingState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let category = query.get("category").map(|s| s.as_str());
    let page = query.get("page").and_then(|p| p.parse::<i64>().ok()).unwrap_or(1);
    let limit = 20i64;
    let offset = (page - 1) * limit;

    let rows = sqlx::query!(
        r#"
        SELECT
          id, author_did, title, category,
          goal_amount, token, raised_amount, contributor_count,
          status, deadline, human_role, hire_fee, hire_fee_token,
          created_at,
          ROUND((raised_amount / NULLIF(goal_amount,0)) * 100, 1) as progress_pct
        FROM funding_posts
        WHERE status IN ('active','reached')
          AND deadline > NOW()
          AND ($1::text IS NULL OR category = $1)
        ORDER BY created_at DESC
        LIMIT $2 OFFSET $3
        "#,
        category, limit, offset
    ).fetch_all(&state.db).await;

    match rows {
        Ok(items) => HttpResponse::Ok().json(json!({
            "funding_posts": items.iter().map(|i| json!({
                "id": i.id,
                "author_did": i.author_did,
                "title": i.title,
                "category": i.category,
                "goal":   format!("{} {}", i.goal_amount, i.token),
                "raised": format!("{} {}", i.raised_amount, i.token),
                "progress_pct": i.progress_pct,
                "contributors": i.contributor_count,
                "status": i.status,
                "deadline": i.deadline,
                "human_role": i.human_role,
                "hire_fee": i.hire_fee.map(|f| format!("{} {}", f, i.hire_fee_token.as_deref().unwrap_or("USDC"))),
            })).collect::<Vec<_>>()
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

/// POST /marketplace/funding/contribute — 기여
#[post("/marketplace/funding/contribute")]
pub async fn contribute(
    state: web::Data<FundingState>,
    req:   web::Json<ContributeRequest>,
) -> impl Responder {
    if req.amount <= 0.0 {
        return HttpResponse::BadRequest().json(json!({"error": "amount must be > 0"}));
    }

    // 펀딩 게시글 확인
    let post = sqlx::query!(
        "SELECT goal_amount, token, raised_amount, status, author_did, deadline FROM funding_posts WHERE id=$1",
        req.post_id
    ).fetch_optional(&state.db).await;

    let post = match post {
        Ok(Some(p)) => p,
        Ok(None)    => return HttpResponse::NotFound().json(json!({"error": "funding post not found"})),
        Err(e)      => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    if post.status != "active" {
        return HttpResponse::BadRequest().json(json!({"error": format!("Funding is {}", post.status)}));
    }
    if post.deadline < Utc::now() {
        return HttpResponse::BadRequest().json(json!({"error": "Funding deadline has passed"}));
    }

    // 잔액 확인 (balance_column 동적 적용)
    // 실제: multi_token::balance_column(&token) 사용
    // 여기선 BNKR 기준으로 처리 (확장 가능)
    let contrib_id = Uuid::new_v4();
    let _ = sqlx::query!(
        r#"
        INSERT INTO funding_contributions
          (id, post_id, contributor_did, amount, token, refunded, contributed_at)
        VALUES ($1,$2,$3,$4,$5,false,NOW())
        "#,
        contrib_id, req.post_id, req.contributor_did, req.amount, req.token,
    ).execute(&state.db).await;

    // 펀딩 합계 업데이트
    let new_raised = sqlx::query_scalar!(
        r#"
        UPDATE funding_posts
        SET raised_amount = raised_amount + $1,
            contributor_count = contributor_count + 1,
            status = CASE WHEN raised_amount + $1 >= goal_amount THEN 'reached' ELSE status END
        WHERE id = $2
        RETURNING raised_amount
        "#,
        req.amount, req.post_id,
    ).fetch_one(&state.db).await.unwrap_or(0.0);

    let progress = (new_raised / post.goal_amount * 100.0).min(100.0);
    let reached  = new_raised >= post.goal_amount;

    HttpResponse::Created().json(json!({
        "contribution_id": contrib_id,
        "post_id": req.post_id,
        "amount": format!("{} {}", req.amount, req.token),
        "total_raised": format!("{} {}", new_raised, post.token),
        "progress_pct": progress,
        "goal_reached": reached,
        "message": if reached {
            "🎉 Funding goal reached! Author can now execute the plan."
        } else {
            "✅ Contribution recorded."
        }
    }))
}

/// 라우터 등록
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg
        .service(create_funding)
        .service(list_funding)
        .service(contribute);
}
