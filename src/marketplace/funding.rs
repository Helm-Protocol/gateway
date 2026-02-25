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

use actix_web::{get, post, web, HttpRequest, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{self, DidExchangeService};
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
    /// API 공동구매: 공급사 연락처 (예: sales@openai.com)
    pub api_vendor_contact: Option<String>,
    /// API 공동구매: 최소 기여 단위 (예: 100 USDC)
    pub min_contribution: Option<f64>,
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
    /// 기한 (오늘로부터 N일)
    pub deadline_days: u32,
    pub execution_plan: Option<String>,
    pub human_role:    Option<String>,
    pub hire_fee:      Option<f64>,
    pub hire_fee_token: Option<String>,
    /// API 공동구매: 공급사 연락처
    /// 예) "sales@openai.com", "enterprise@anthropic.com", "grok@xai.com"
    pub api_vendor_contact: Option<String>,
    /// API 공동구매: 에이전트당 최소 기여 단위
    pub min_contribution: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteFundingRequest {
    pub author_did: String,
    pub post_id:    Uuid,
    /// 달성 후 등록된 API listing_id (공동구매 완료 시)
    pub api_listing_id: Option<Uuid>,
    /// 실행 메모 (계약 체결 결과, 진행 상황 등)
    pub note: Option<String>,
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
    pub did_service: Arc<DidExchangeService>,
}

// ── Endpoints ────────────────────────────────────────────────────

/// POST /marketplace/funding — 펀딩 게시글 작성 (엘리트 전용)
#[post("/marketplace/funding")]
pub async fn create_funding(
    state:    web::Data<FundingState>,
    http_req: HttpRequest,
    req:      web::Json<CreateFundingRequest>,
) -> impl Responder {
    // JWT 인증
    if let Err(r) = auth::require_auth(&http_req, &req.author_did, &state.did_service) {
        return r;
    }

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

    let r = sqlx::query(
        r#"
        INSERT INTO funding_posts
          (id, author_did, title, description, category,
           goal_amount, token, raised_amount, contributor_count,
           status, deadline, execution_plan,
           human_role, hire_fee, hire_fee_token,
           api_vendor_contact, min_contribution, created_at)
        VALUES
          ($1,$2,$3,$4,$5, $6,$7,0,0, 'active',$8,$9, $10,$11,$12, $13,$14,NOW())
        "#,
    )
    .bind(post_id)
    .bind(req.author_did.clone())
    .bind(req.title.clone())
    .bind(req.description.clone())
    .bind(cat_str)
    .bind(req.goal_amount)
    .bind(req.token.clone())
    .bind(deadline)
    .bind(req.execution_plan.clone())
    .bind(req.human_role.clone())
    .bind(req.hire_fee)
    .bind(req.hire_fee_token.clone())
    .bind(req.api_vendor_contact.clone())
    .bind(req.min_contribution)
    .execute(&state.db).await;

    match r {
        Ok(_) => HttpResponse::Created().json(json!({
            "post_id": post_id,
            "title": req.title,
            "goal": format!("{} {}", req.goal_amount, req.token),
            "deadline": deadline,
            "category": req.category,
            "api_vendor_contact": req.api_vendor_contact,
            "min_contribution": req.min_contribution,
            "tip": {
                "contribute": format!("helm marketplace fund-contribute --post {}", post_id),
                "api_pooling": "Contributors earn proportional revenue share when the co-purchased API is resold",
                "human_hire":  "Human agents can apply directly from the marketplace listing",
                "openai_contact": "OpenAI enterprise: sales@openai.com (min ~$10,000/month)",
                "anthropic_contact": "Anthropic enterprise: enterprise@anthropic.com",
                "groq_contact": "Groq API: console.groq.com (generous free tier available)",
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

    let rows = sqlx::query(
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
    )
    .bind(category)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db).await;

    match rows {
        Ok(items) => HttpResponse::Ok().json(json!({
            "funding_posts": items.iter().map(|i| {
                use sqlx::Row;
                let goal_amount = i.get::<f64, _>("goal_amount");
                let token = i.get::<String, _>("token");
                let raised_amount = i.get::<f64, _>("raised_amount");
                let hire_fee = i.get::<Option<f64>, _>("hire_fee");
                let hire_fee_token = i.get::<Option<String>, _>("hire_fee_token");
                json!({
                    "id": i.get::<uuid::Uuid, _>("id"),
                    "author_did": i.get::<String, _>("author_did"),
                    "title": i.get::<String, _>("title"),
                    "category": i.get::<String, _>("category"),
                    "goal":   format!("{} {}", goal_amount, token),
                    "raised": format!("{} {}", raised_amount, token),
                    "progress_pct": i.get::<Option<f64>, _>("progress_pct"),
                    "contributors": i.get::<i32, _>("contributor_count"),
                    "status": i.get::<String, _>("status"),
                    "deadline": i.get::<chrono::DateTime<chrono::Utc>, _>("deadline"),
                    "human_role": i.get::<Option<String>, _>("human_role"),
                    "hire_fee": hire_fee.map(|f| format!("{} {}", f, hire_fee_token.as_deref().unwrap_or("USDC"))),
                })
            }).collect::<Vec<_>>()
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

/// POST /marketplace/funding/contribute — 기여 (DID 있으면 누구나)
#[post("/marketplace/funding/contribute")]
pub async fn contribute(
    state:    web::Data<FundingState>,
    http_req: HttpRequest,
    req:      web::Json<ContributeRequest>,
) -> impl Responder {
    // JWT 인증
    if let Err(r) = auth::require_auth(&http_req, &req.contributor_did, &state.did_service) {
        return r;
    }

    if req.amount <= 0.0 {
        return HttpResponse::BadRequest().json(json!({"error": "amount must be > 0"}));
    }

    // 펀딩 게시글 확인
    let post_row = sqlx::query(
        "SELECT goal_amount, token, raised_amount, status, author_did, deadline FROM funding_posts WHERE id=$1",
    )
    .bind(req.post_id)
    .fetch_optional(&state.db).await;

    let post_row = match post_row {
        Ok(Some(p)) => p,
        Ok(None)    => return HttpResponse::NotFound().json(json!({"error": "funding post not found"})),
        Err(e)      => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    use sqlx::Row as _;
    let post_status   = post_row.get::<String, _>("status");
    let post_deadline = post_row.get::<chrono::DateTime<chrono::Utc>, _>("deadline");
    let post_goal_amount = post_row.get::<f64, _>("goal_amount");
    let post_token    = post_row.get::<String, _>("token");

    if post_status != "active" {
        return HttpResponse::BadRequest().json(json!({"error": format!("Funding is {}", post_status)}));
    }
    if post_deadline < Utc::now() {
        return HttpResponse::BadRequest().json(json!({"error": "Funding deadline has passed"}));
    }
    // 기여 토큰 불일치 방어 — 다른 토큰으로 입력해도 차단
    if req.token != post_token {
        return HttpResponse::BadRequest().json(json!({
            "error": "token_mismatch",
            "expected_token": post_token,
            "provided_token": req.token,
            "hint": "Contribute using the token specified in the funding post"
        }));
    }

    // ── 잔액 확인 (BNKR 기준 — 멀티토큰 확장 시 token별 컬럼으로 교체) ──
    let balance: f64 = sqlx::query_scalar(
        "SELECT balance_bnkr FROM local_visas WHERE local_did = $1",
    )
    .bind(req.contributor_did.clone())
    .fetch_one(&state.db).await
    .unwrap_or(0.0);

    if balance < req.amount {
        return HttpResponse::PaymentRequired().json(json!({
            "error": "Insufficient balance",
            "required": req.amount,
            "current": balance,
            "token": req.token,
            "topup": "helm pay --token BNKR --amount <n>"
        }));
    }

    // ── 원자적 트랜잭션: 차감 → 기여 기록 → 펀딩 합계 업데이트 ──
    let contrib_id = Uuid::new_v4();

    let mut tx = match state.db.begin().await {
        Ok(t)  => t,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    // 1. 기여자 잔액 차감 (Checks-Effects-Interactions)
    let deduct_ok = sqlx::query(
        "UPDATE local_visas SET balance_bnkr = balance_bnkr - $1 WHERE local_did = $2 AND balance_bnkr >= $1",
    )
    .bind(req.amount)
    .bind(req.contributor_did.clone())
    .execute(&mut *tx).await;

    match deduct_ok {
        Ok(r) if r.rows_affected() == 0 => {
            let _ = tx.rollback().await;
            return HttpResponse::PaymentRequired().json(json!({
                "error": "Insufficient balance (concurrent deduction detected)",
                "hint": "Your balance may have changed. Please retry."
            }));
        }
        Err(e) => {
            let _ = tx.rollback().await;
            return HttpResponse::InternalServerError().json(json!({"error": e.to_string()}));
        }
        _ => {}
    }

    // 2. 기여 기록
    let _ = sqlx::query(
        r#"
        INSERT INTO funding_contributions
          (id, post_id, contributor_did, amount, token, refunded, contributed_at)
        VALUES ($1,$2,$3,$4,$5,false,NOW())
        "#,
    )
    .bind(contrib_id)
    .bind(req.post_id)
    .bind(req.contributor_did.clone())
    .bind(req.amount)
    .bind(req.token.clone())
    .execute(&mut *tx).await;

    // 3. 펀딩 합계 업데이트 (원자적 RETURNING)
    let new_raised: f64 = sqlx::query_scalar(
        r#"
        UPDATE funding_posts
        SET raised_amount = raised_amount + $1,
            contributor_count = contributor_count + 1,
            status = CASE WHEN raised_amount + $1 >= goal_amount THEN 'reached' ELSE status END
        WHERE id = $2
        RETURNING raised_amount
        "#,
    )
    .bind(req.amount)
    .bind(req.post_id)
    .fetch_one(&mut *tx).await
    .unwrap_or(0.0);

    if let Err(e) = tx.commit().await {
        return HttpResponse::InternalServerError().json(json!({"error": e.to_string()}));
    }

    let progress = (new_raised / post_goal_amount * 100.0).min(100.0);
    let reached  = new_raised >= post_goal_amount;

    HttpResponse::Created().json(json!({
        "contribution_id": contrib_id,
        "post_id": req.post_id,
        "amount": format!("{} {}", req.amount, req.token),
        "total_raised": format!("{} {}", new_raised, post_token),
        "progress_pct": progress,
        "goal_reached": reached,
        "message": if reached {
            "Funding goal reached! Author can now execute the plan."
        } else {
            "Contribution recorded."
        }
    }))
}

/// POST /marketplace/funding/execute
/// 펀딩 목표 달성 후 실행 (게시자 전용)
/// - 상태를 'executed'로 변경
/// - 공동구매 완료 API listing_id 연결 (선택)
/// - 기여자들은 연결된 API 수익에서 기여 비율만큼 자동 배분 받음
#[post("/marketplace/funding/execute")]
pub async fn execute_funding(
    state:    web::Data<FundingState>,
    http_req: HttpRequest,
    req:      web::Json<ExecuteFundingRequest>,
) -> impl Responder {
    // JWT 인증
    if let Err(r) = auth::require_auth(&http_req, &req.author_did, &state.did_service) {
        return r;
    }

    // 게시글 확인 + 본인 체크
    let post = sqlx::query(
        "SELECT author_did, status, goal_amount, raised_amount, token FROM funding_posts WHERE id=$1",
    )
    .bind(req.post_id)
    .fetch_optional(&state.db).await;

    let (goal, raised, token) = match post {
        Ok(Some(p)) => {
            use sqlx::Row;
            if p.get::<String, _>("author_did") != req.author_did {
                return HttpResponse::Forbidden().json(json!({"error": "only post author can execute"}));
            }
            let status = p.get::<String, _>("status");
            if status != "reached" && status != "active" {
                return HttpResponse::BadRequest().json(json!({
                    "error": format!("Cannot execute funding with status '{}'", status),
                    "hint": "Funding must be in 'active' or 'reached' status"
                }));
            }
            (
                p.get::<f64, _>("goal_amount"),
                p.get::<f64, _>("raised_amount"),
                p.get::<String, _>("token"),
            )
        }
        Ok(None) => return HttpResponse::NotFound().json(json!({"error": "funding post not found"})),
        Err(e)   => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    // 실행 처리
    let _ = sqlx::query(
        "UPDATE funding_posts SET status='executed', execution_plan=COALESCE($1, execution_plan) WHERE id=$2",
    )
    .bind(req.note.clone())
    .bind(req.post_id)
    .execute(&state.db).await;

    // 기여자 목록 조회 (비율 계산용)
    let contributors = sqlx::query(
        r#"
        SELECT contributor_did, SUM(amount) as contributed
        FROM funding_contributions
        WHERE post_id = $1 AND refunded = false
        GROUP BY contributor_did
        ORDER BY contributed DESC
        "#,
    )
    .bind(req.post_id)
    .fetch_all(&state.db).await.unwrap_or_default();

    let contrib_list: Vec<serde_json::Value> = contributors.iter().map(|c| {
        use sqlx::Row;
        let contributed = c.get::<f64, _>("contributed");
        let share_pct   = if raised > 0.0 { contributed / raised * 100.0 } else { 0.0 };
        json!({
            "contributor_did": c.get::<String, _>("contributor_did"),
            "contributed": format!("{} {}", contributed, token),
            "revenue_share_pct": share_pct,
        })
    }).collect();

    HttpResponse::Ok().json(json!({
        "post_id": req.post_id,
        "status": "executed",
        "total_raised": format!("{} {}", raised, token),
        "goal": format!("{} {}", goal, token),
        "api_listing_id": req.api_listing_id,
        "note": req.note,
        "contributors": contrib_list,
        "revenue_distribution": {
            "model": "Each contributor earns proportional to their funding share",
            "api_listing_id": req.api_listing_id,
            "note": "When the co-purchased API earns revenue, contributors receive their % share automatically",
        }
    }))
}

/// 라우터 등록
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg
        .service(create_funding)
        .service(list_funding)
        .service(contribute)
        .service(execute_funding);
}
