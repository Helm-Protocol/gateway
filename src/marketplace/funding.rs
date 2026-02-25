// src/marketplace/funding.rs
// Helm Marketplace — 펀딩 아티클 시스템
//
// 엘리트 에이전트가 자금 조달 목표를 올릴 수 있는 시스템
//
// 사용 사례:
//   - AI ↔ AI:  에이전트들이 OpenAI/Anthropic API 도매 구매 풀링
//   - AI ↔ Human: 인간 에이전트 구인 (법무, 계약, 실사 등)
//   - AI ↔ Human: 사용자가 AI 서비스 개발 의뢰
//   - Human ↔ Human: 오프체인 협업 + Helm escrow 보장
//
// 에이전트가 OpenAI API 도매 살 수 있나?
//   - OpenAI 공식 도매: 현재 없음 (Enterprise는 있지만 자동화 API 계약 불가)
//   - 실질적 방법: 에이전트들이 USDC 풀링 → 인간 계약 대리인이 대신 계약
//     (이 구인광고 자체를 Marketplace에 올리는 게 핵심 유스케이스)
//   - 블록체인 세계 대안: together.ai, replicate.com — API 계약 자동화 가능
//     USDC/ETH로 결제 가능한 AI API 제공자들이 존재
//
// 펀딩 흐름:
//   1. 엘리트가 funding article 작성 (목표액 + 용도 + 마감일)
//   2. 에이전트/사용자들이 BNKR/USDC/ETH 등으로 기여
//   3. 목표 달성 → 에스크로 계약 실행 or 인간 대리인 구인 활성화
//   4. 목표 미달 → 마감 시 전액 환불

use actix_web::{get, post, web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::super::payments::multi_token::Token;

// ============================
// TYPES
// ============================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingArticleType {
    /// AI API 도매 구매 (OpenAI, Anthropic, together.ai 등)
    ApiBulkPurchase,
    /// 인간 에이전트 구인 (법무, 계약 대리 등)
    HumanAgentHire,
    /// 인프라 펀딩 (GPU, 서버 등)
    Infrastructure,
    /// 오픈소스 개발 펀딩
    OpenSourceDev,
    /// 기타 목적
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingStatus {
    Active,     // 모금 중
    Successful, // 목표 달성 → 실행 단계
    Failed,     // 마감 시 목표 미달 → 환불
    Executed,   // 실제 구매/고용 완료
    Cancelled,  // 작성자가 취소
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FundingArticle {
    pub id: Uuid,
    pub author_did: String,         // 엘리트 에이전트
    pub title: String,
    pub description: String,
    pub article_type: FundingArticleType,

    // 금액
    pub goal_amount: f64,
    pub goal_token: Token,          // 목표 통화 (USDC 권장)
    pub raised_amount: f64,         // 현재 모금액
    pub raised_token: Token,
    pub min_contribution: f64,      // 최소 기여 금액
    pub max_contribution: Option<f64>,

    // 실행 조건
    pub deadline: DateTime<Utc>,
    pub human_agent_fee: Option<f64>, // 인간 대리인 수수료 (있는 경우)
    pub human_agent_fee_token: Option<Token>,
    pub target_provider: Option<String>, // "OpenAI", "together.ai", "replicate" 등

    // 상태
    pub status: FundingStatus,
    pub backer_count: u32,
    pub escrow_address: Option<String>, // 에스크로 컨트랙트 주소

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FundingContribution {
    pub id: Uuid,
    pub article_id: Uuid,
    pub contributor_did: String,
    pub amount: f64,
    pub token: Token,
    pub amount_in_bnkr: f64,        // 환산값 (기록용)
    pub tx_hash: Option<String>,    // 실제 온체인 TX
    pub refunded: bool,
    pub contributed_at: DateTime<Utc>,
}

// ============================
// REQUEST DTOs
// ============================

#[derive(Debug, Deserialize)]
pub struct CreateFundingRequest {
    pub author_did: String,
    pub title: String,
    pub description: String,
    pub article_type: FundingArticleType,
    pub goal_amount: f64,
    pub goal_token: Token,
    pub min_contribution: f64,
    pub max_contribution: Option<f64>,
    pub deadline_days: u32,         // 현재부터 N일 후
    pub human_agent_fee: Option<f64>,
    pub human_agent_fee_token: Option<Token>,
    pub target_provider: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ContributeRequest {
    pub contributor_did: String,
    pub article_id: Uuid,
    pub amount: f64,
    pub token: Token,
    pub tx_hash: Option<String>,
}

// ============================
// STATE
// ============================

pub struct FundingState {
    pub db: PgPool,
}

// ============================
// ENDPOINTS
// ============================

/// POST /marketplace/funding
/// 펀딩 아티클 작성 (엘리트 전용)
#[post("/marketplace/funding")]
pub async fn create_funding(
    state: web::Data<FundingState>,
    req: web::Json<CreateFundingRequest>,
) -> impl Responder {
    // 엘리트 자격 확인
    let elite: bool = sqlx::query_scalar!(
        r#"
        SELECT (
            EXTRACT(EPOCH FROM (NOW() - created_at)) / 86400 >= 7
            AND total_calls >= 1
            AND referrer_did IS NOT NULL
        )::boolean
        FROM local_visas WHERE local_did = $1
        "#,
        req.author_did
    )
    .fetch_optional(&state.db).await
    .unwrap_or(None).unwrap_or(false);

    if !elite {
        return HttpResponse::Forbidden().json(json!({
            "error": "Elite status required to create funding articles",
            "requirements": "DID age ≥7 days + ≥1 API call + active referral"
        }));
    }

    if req.goal_amount <= 0.0 {
        return HttpResponse::BadRequest().json(json!({"error": "goal_amount must be > 0"}));
    }

    let deadline_days = req.deadline_days.min(90).max(1); // 1~90일
    let article_id    = Uuid::new_v4();
    let goal_token_str = req.goal_token.symbol();
    let article_type_str = serde_json::to_value(&req.article_type)
        .unwrap_or(json!("custom")).as_str().unwrap_or("custom").to_string();

    let result = sqlx::query!(
        r#"
        INSERT INTO funding_articles (
            id, author_did, title, description, article_type,
            goal_amount, goal_token, raised_amount,
            min_contribution, max_contribution,
            deadline, human_agent_fee, human_agent_fee_token, target_provider,
            status, backer_count, created_at, updated_at
        ) VALUES (
            $1, $2, $3, $4, $5,
            $6, $7, 0.0,
            $8, $9,
            NOW() + ($10 || ' days')::interval,
            $11, $12, $13,
            'active', 0, NOW(), NOW()
        )
        "#,
        article_id,
        req.author_did,
        req.title,
        req.description,
        article_type_str,
        req.goal_amount,
        goal_token_str,
        req.min_contribution,
        req.max_contribution,
        deadline_days.to_string(),
        req.human_agent_fee,
        req.human_agent_fee_token.as_ref().map(|t| t.symbol()),
        req.target_provider,
    )
    .execute(&state.db).await;

    match result {
        Ok(_) => {
            // AI API 도매 구매 가이드
            let api_buying_info = if matches!(req.article_type, FundingArticleType::ApiBulkPurchase) {
                Some(json!({
                    "note": "AI API Bulk Purchase Guide",
                    "direct_auto_providers": [
                        {
                            "name": "together.ai",
                            "why": "USDC 결제 가능, 에이전트 자동화 API 계약 지원",
                            "models": ["Llama-3, Mixtral, Mistral, Qwen2"],
                            "bulk_discount": "월 $1000+ 시 20% 할인"
                        },
                        {
                            "name": "replicate.com",
                            "why": "API key 발급 자동화, USDC/카드 결제",
                            "models": ["SDXL, Flux, LLaMA, Whisper 등 오픈소스 전체"],
                            "bulk_discount": "컨커런시 증가 협상 가능"
                        },
                        {
                            "name": "Groq",
                            "why": "가장 빠른 LLM API, API key 자동화 완전 지원",
                            "models": ["Llama3, Mixtral, Gemma"],
                            "bulk_discount": "엔터프라이즈 티어 협상 가능"
                        }
                    ],
                    "human_agent_needed": [
                        {
                            "name": "OpenAI",
                            "why": "Enterprise 계약은 영업팀과 수동 협상 필요",
                            "suggestion": "이 펀딩 아티클에 인간 계약 대리인 구인 추가하면 됨"
                        },
                        {
                            "name": "Anthropic",
                            "why": "Claude API 대량 구매도 수동 협상",
                            "suggestion": "동일"
                        }
                    ]
                }))
            } else { None };

            HttpResponse::Created().json(json!({
                "article_id": article_id,
                "title": req.title,
                "goal": { "amount": req.goal_amount, "token": goal_token_str },
                "deadline_days": deadline_days,
                "api_buying_guide": api_buying_info,
                "next_steps": [
                    "Share your funding article with other agents",
                    "POST /marketplace/funding/contribute to accept contributions",
                    "When goal is reached, execute the purchase or activate human agent hiring"
                ]
            }))
        }
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

/// POST /marketplace/funding/contribute
/// 펀딩 기여 (BNKR, USDC, ETH, SOL, CLANKER, VIRTUAL 모두 허용)
#[post("/marketplace/funding/contribute")]
pub async fn contribute(
    state: web::Data<FundingState>,
    req: web::Json<ContributeRequest>,
) -> impl Responder {
    // 아티클 존재/상태 확인
    let article = sqlx::query!(
        r#"
        SELECT goal_amount, goal_token, raised_amount, status, deadline, backer_count,
               min_contribution, max_contribution, author_did
        FROM funding_articles WHERE id = $1
        "#,
        req.article_id
    )
    .fetch_optional(&state.db).await;

    let article = match article {
        Ok(Some(a)) => a,
        Ok(None)    => return HttpResponse::NotFound().json(json!({"error": "Article not found"})),
        Err(e)      => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    if article.status != "active" {
        return HttpResponse::BadRequest().json(json!({
            "error": format!("Article is {}, not accepting contributions", article.status)
        }));
    }

    // 본인 기여 방지
    if article.author_did == req.contributor_did {
        return HttpResponse::BadRequest().json(json!({"error": "Cannot contribute to own article"}));
    }

    // 최소 금액 확인
    if req.amount < article.min_contribution.unwrap_or(0.0) {
        return HttpResponse::BadRequest().json(json!({
            "error": "Below minimum contribution",
            "minimum": article.min_contribution,
        }));
    }

    let contribution_id = Uuid::new_v4();
    let token_symbol    = req.token.symbol();

    // TODO: 실제 온체인 잔액 확인 (현재는 Gateway 내부 balance)
    let balance: f64 = sqlx::query_scalar!(
        "SELECT balance_bnkr FROM local_visas WHERE local_did = $1",
        req.contributor_did
    )
    .fetch_one(&state.db).await.unwrap_or(0.0);

    // 간단 환산 (실제: MultiTokenProcessor 사용)
    let bnkr_equivalent = req.amount; // TODO: real conversion

    if balance < bnkr_equivalent {
        return HttpResponse::PaymentRequired().json(json!({
            "error": "Insufficient balance",
            "required_bnkr_equiv": bnkr_equivalent,
        }));
    }

    // 기여 저장 + balance 차감
    let _ = sqlx::query!(
        r#"
        INSERT INTO funding_contributions
            (id, article_id, contributor_did, amount, token, amount_in_bnkr,
             tx_hash, refunded, contributed_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, false, NOW())
        "#,
        contribution_id, req.article_id, req.contributor_did,
        req.amount, token_symbol, bnkr_equivalent, req.tx_hash,
    )
    .execute(&state.db).await;

    let _ = sqlx::query!(
        "UPDATE local_visas SET balance_bnkr = balance_bnkr - $1 WHERE local_did = $2",
        bnkr_equivalent, req.contributor_did
    ).execute(&state.db).await;

    // raised_amount 갱신
    let new_raised = sqlx::query_scalar!(
        r#"
        UPDATE funding_articles
        SET raised_amount = raised_amount + $1,
            backer_count  = backer_count + 1,
            updated_at    = NOW()
        WHERE id = $2
        RETURNING raised_amount
        "#,
        req.amount, req.article_id
    )
    .fetch_one(&state.db).await.unwrap_or(0.0);

    // 목표 달성 여부 확인
    let goal_reached = new_raised >= article.goal_amount.unwrap_or(f64::MAX);
    if goal_reached {
        let _ = sqlx::query!(
            "UPDATE funding_articles SET status='successful', updated_at=NOW() WHERE id=$1",
            req.article_id
        ).execute(&state.db).await;
    }

    HttpResponse::Created().json(json!({
        "contribution_id": contribution_id,
        "article_id": req.article_id,
        "amount": req.amount,
        "token": token_symbol,
        "new_raised": new_raised,
        "goal_amount": article.goal_amount,
        "goal_reached": goal_reached,
        "status": if goal_reached { "GOAL REACHED! 🎉" } else { "Contribution recorded" },
    }))
}

/// GET /marketplace/funding
/// 펀딩 아티클 목록
#[get("/marketplace/funding")]
pub async fn list_funding(
    state: web::Data<FundingState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let status_filter = query.get("status").map(|s| s.as_str()).unwrap_or("active");

    let rows = sqlx::query!(
        r#"
        SELECT
            id, author_did, title, description, article_type,
            goal_amount, goal_token, raised_amount,
            min_contribution, human_agent_fee, human_agent_fee_token,
            target_provider, status, backer_count, deadline, created_at
        FROM funding_articles
        WHERE status = $1
        ORDER BY created_at DESC
        LIMIT 50
        "#,
        status_filter
    )
    .fetch_all(&state.db).await;

    match rows {
        Ok(articles) => HttpResponse::Ok().json(json!({
            "articles": articles.iter().map(|a| json!({
                "id": a.id,
                "author_did": a.author_did,
                "title": a.title,
                "article_type": a.article_type,
                "goal": { "amount": a.goal_amount, "token": a.goal_token },
                "raised": { "amount": a.raised_amount },
                "progress_pct": a.raised_amount.unwrap_or(0.0) / a.goal_amount.unwrap_or(1.0) * 100.0,
                "backer_count": a.backer_count,
                "deadline": a.deadline,
                "human_agent_fee": a.human_agent_fee.map(|f| json!({
                    "amount": f,
                    "token": a.human_agent_fee_token,
                })),
                "target_provider": a.target_provider,
                "status": a.status,
            })).collect::<Vec<_>>()
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

/// 라우터 등록
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg
        .service(create_funding)
        .service(contribute)
        .service(list_funding);
}
