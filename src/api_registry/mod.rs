// src/api_registry/mod.rs  (gateway repo)
// API Reseller 시스템 — 에이전트가 자신의 API를 중개 상품으로 등록
//
// 핵심 흐름:
//   1. 에이전트 A가 자신의 LLM endpoint를 등록 (helm api register)
//      → A의 DID Document에 ServiceEndpoint 추가
//      → Gateway의 api_listings DB에 저장
//
//   2. 에이전트 B가 A의 API를 구독 (helm api subscribe)
//      → api_subscriptions에 기록
//      → A가 B의 referrer로 등록 (15% 수수료 흐름 활성화)
//
//   3. B가 A를 통해 API 호출 (helm api call --via <A-did>)
//      → Gateway가 A의 endpoint로 프록시
//      → billing.rs: B 과금 → 85% Treasury, 15% A (referrer)
//      → A는 자신의 upstream 비용을 직접 부담 (마진 보장)
//
// 수익 구조:
//   B가 10 BNKR 냄
//     └─ 8.5 BNKR → Treasury
//     └─ 1.5 BNKR → A (referrer 수수료)
//   A의 upstream 비용은 A가 직접 관리
//   A는 가격을 올려서 마진을 만든다

use actix_web::{delete, get, post, web, HttpRequest, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::auth::{self, DidExchangeService};
use std::sync::Arc;

// ============================
// TYPES
// ============================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiListing {
    pub id: Uuid,
    pub owner_did: String,          // 등록한 에이전트 DID
    pub name: String,               // "My GPT-4 Proxy"
    pub description: Option<String>,
    pub category: ApiCategory,
    pub endpoint_url: String,       // 프록시할 실제 URL
    pub price_per_call_bnkr: u64,   // B가 낼 가격 (BNKR)
    pub sla_latency_ms: Option<u32>,
    pub sla_uptime_pct: Option<f32>,
    pub active: bool,
    pub call_count: u64,
    pub subscriber_count: u32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiCategory {
    Llm,        // LLM 프록시 (GPT-4, Claude, Llama 등)
    Search,     // 검색 API
    Defi,       // DeFi 오라클
    Compute,    // 일반 연산
    Storage,    // 데이터 저장
    Custom,     // 기타
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiSubscription {
    pub id: Uuid,
    pub subscriber_did: String,
    pub listing_id: Uuid,
    pub owner_did: String,
    pub active: bool,
    pub total_calls: u64,
    pub total_paid_bnkr: u64,
    pub subscribed_at: DateTime<Utc>,
}

// ============================
// REQUEST DTOs
// ============================

#[derive(Debug, Deserialize)]
pub struct RegisterApiRequest {
    pub agent_did: String,
    pub name: String,
    pub description: Option<String>,
    pub category: ApiCategory,
    pub endpoint_url: String,
    pub price_per_call_bnkr: u64,
    pub sla_latency_ms: Option<u32>,
    pub sla_uptime_pct: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct SubscribeRequest {
    pub subscriber_did: String,
    pub listing_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct ProxyCallRequest {
    pub caller_did: String,
    pub listing_id: Uuid,
    /// 실제 API에 전달할 payload
    pub payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct ListApiQuery {
    pub category: Option<String>,
    pub owner_did: Option<String>,
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

// ============================
// APP STATE
// ============================

pub struct ApiRegistryState {
    pub db:          PgPool,
    pub http:        reqwest::Client,
    pub did_service: Arc<DidExchangeService>,
}

// ============================
// ENDPOINTS
// ============================

/// POST /api-registry/register
/// 에이전트가 자신의 API를 중개 상품으로 등록
#[post("/api-registry/register")]
pub async fn register_api(
    state:    web::Data<ApiRegistryState>,
    http_req: HttpRequest,
    req:      web::Json<RegisterApiRequest>,
) -> impl Responder {
    // JWT 인증
    if let Err(r) = auth::require_auth(&http_req, &req.agent_did, &state.did_service) {
        return r;
    }

    // DID 존재 확인
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM local_visas WHERE local_did = $1",
    )
    .bind(req.agent_did.clone())
    .fetch_one(&state.db).await
    .unwrap_or(0);

    if exists == 0 {
        return HttpResponse::Unauthorized().json(json!({
            "error": "DID not registered. Run: helm init"
        }));
    }

    // URL 유효성 (http/https만 허용)
    if !req.endpoint_url.starts_with("http://") && !req.endpoint_url.starts_with("https://") {
        return HttpResponse::BadRequest().json(json!({
            "error": "endpoint_url must start with http:// or https://"
        }));
    }

    if req.price_per_call_bnkr == 0 {
        return HttpResponse::BadRequest().json(json!({
            "error": "price_per_call_bnkr must be > 0"
        }));
    }

    let listing_id  = Uuid::new_v4();
    let category_str = serde_json::to_value(&req.category)
        .unwrap_or(json!("custom"))
        .as_str()
        .unwrap_or("custom")
        .to_string();

    let result = sqlx::query(
        r#"
        INSERT INTO api_listings
            (id, owner_did, name, description, category,
             endpoint_url, price_per_call_bnkr,
             sla_latency_ms, sla_uptime_pct,
             active, call_count, subscriber_count, created_at)
        VALUES
            ($1, $2, $3, $4, $5,
             $6, $7,
             $8, $9,
             true, 0, 0, NOW())
        "#,
    )
    .bind(listing_id)
    .bind(req.agent_did.clone())
    .bind(req.name.clone())
    .bind(req.description.clone())
    .bind(category_str)
    .bind(req.endpoint_url.clone())
    .bind(req.price_per_call_bnkr as i64)
    .bind(req.sla_latency_ms.map(|v| v as i32))
    .bind(req.sla_uptime_pct)
    .execute(&state.db).await;

    match result {
        Ok(_) => HttpResponse::Created().json(json!({
            "listing_id": listing_id,
            "owner_did": req.agent_did,
            "name": req.name,
            "price_per_call_bnkr": req.price_per_call_bnkr,
            "message": "API registered. Agents can now discover and subscribe to your API.",
            "revenue_model": {
                "per_call_income": format!("{} BNKR (15% referrer share)", (req.price_per_call_bnkr as f64 * 0.15) as u64),
                "treasury_cut": format!("{} BNKR (85% to Treasury)", (req.price_per_call_bnkr as f64 * 0.85) as u64),
                "note": "Price your API above your upstream cost to ensure margin"
            }
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

/// GET /api-registry/listings
/// 등록된 API 목록 조회 (누구나)
#[get("/api-registry/listings")]
pub async fn list_apis(
    state: web::Data<ApiRegistryState>,
    query: web::Query<ListApiQuery>,
) -> impl Responder {
    let limit  = query.limit.unwrap_or(20).min(50) as i64;
    let offset = ((query.page.unwrap_or(1).max(1) - 1) as i64) * limit;

    let rows = sqlx::query(
        r#"
        SELECT
            id, owner_did, name, description, category,
            price_per_call_bnkr, sla_latency_ms, sla_uptime_pct,
            call_count, subscriber_count, created_at
        FROM api_listings
        WHERE active = true
          AND ($1::text IS NULL OR category = $1)
          AND ($2::text IS NULL OR owner_did = $2)
        ORDER BY call_count DESC, created_at DESC
        LIMIT $3 OFFSET $4
        "#,
    )
    .bind(query.category.as_deref())
    .bind(query.owner_did.as_deref())
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db).await;

    match rows {
        Ok(listings) => {
            let items: Vec<serde_json::Value> = listings.iter().map(|l| json!({
                "id": l.get::<uuid::Uuid, _>("id"),
                "owner_did": l.get::<String, _>("owner_did"),
                "name": l.get::<String, _>("name"),
                "description": l.get::<Option<String>, _>("description"),
                "category": l.get::<String, _>("category"),
                "price_per_call_bnkr": l.get::<i64, _>("price_per_call_bnkr"),
                "sla_latency_ms": l.get::<Option<i32>, _>("sla_latency_ms"),
                "sla_uptime_pct": l.get::<Option<f32>, _>("sla_uptime_pct"),
                "call_count": l.get::<i64, _>("call_count"),
                "subscriber_count": l.get::<i32, _>("subscriber_count"),
                "created_at": l.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })).collect();
            HttpResponse::Ok().json(json!({
                "listings": items,
                "count": items.len(),
            }))
        }
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

/// POST /api-registry/subscribe
/// B가 A의 API를 구독
/// ※ 레퍼럴은 helm init --referrer 시에만 설정됨 (API 구독과 무관)
#[post("/api-registry/subscribe")]
pub async fn subscribe(
    state:    web::Data<ApiRegistryState>,
    http_req: HttpRequest,
    req:      web::Json<SubscribeRequest>,
) -> impl Responder {
    // JWT 인증
    if let Err(r) = auth::require_auth(&http_req, &req.subscriber_did, &state.did_service) {
        return r;
    }
    // listing 확인
    let listing = sqlx::query(
        "SELECT owner_did, name, price_per_call_bnkr FROM api_listings WHERE id = $1 AND active = true",
    )
    .bind(req.listing_id)
    .fetch_optional(&state.db).await;

    let (owner_did, api_name, price) = match listing {
        Ok(Some(l)) => (l.get::<String, _>("owner_did"), l.get::<String, _>("name"), l.get::<i64, _>("price_per_call_bnkr")),
        Ok(None)    => return HttpResponse::NotFound().json(json!({"error": "listing not found"})),
        Err(e)      => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    // 자기 자신 구독 방지
    if owner_did == req.subscriber_did {
        return HttpResponse::BadRequest().json(json!({"error": "cannot subscribe to own API"}));
    }

    // 중복 구독 방지
    let already: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_subscriptions WHERE subscriber_did=$1 AND listing_id=$2 AND active=true",
    )
    .bind(req.subscriber_did.clone())
    .bind(req.listing_id)
    .fetch_one(&state.db).await
    .unwrap_or(0);

    if already > 0 {
        return HttpResponse::BadRequest().json(json!({"error": "already subscribed"}));
    }

    // 구독 저장
    let sub_id = Uuid::new_v4();
    let _ = sqlx::query(
        r#"
        INSERT INTO api_subscriptions
            (id, subscriber_did, listing_id, owner_did, active, total_calls, total_paid_bnkr, subscribed_at)
        VALUES ($1, $2, $3, $4, true, 0, 0, NOW())
        "#,
    )
    .bind(sub_id)
    .bind(req.subscriber_did.clone())
    .bind(req.listing_id)
    .bind(owner_did.clone())
    .execute(&state.db).await;

    // ※ 레퍼럴은 helm init --referrer 로만 설정 (API 구독 시 자동 배정 금지)
    //   이유: API 구독이 글로벌 레퍼럴을 오염시키는 버그 수정
    //   API owner 수익은 proxy_call에서 owner_did 직접 결제로 처리

    // subscriber_count 증가
    let _ = sqlx::query(
        "UPDATE api_listings SET subscriber_count = subscriber_count + 1 WHERE id = $1",
    )
    .bind(req.listing_id)
    .execute(&state.db).await;

    HttpResponse::Created().json(json!({
        "subscription_id": sub_id,
        "subscriber_did": req.subscriber_did,
        "owner_did": owner_did,
        "api_name": api_name,
        "price_per_call_bnkr": price,
        "message": format!("Subscribed to {}! You can now call this API via Gateway.", api_name),
        "billing_note": {
            "per_call": format!("{} BNKR", price),
            "to_treasury": format!("{} BNKR (85%)", (price as f64 * 0.85) as u64),
            "to_api_owner": format!("{} BNKR (15% reseller commission)", (price as f64 * 0.15) as u64),
        }
    }))
}

/// POST /api-registry/call
/// B가 A의 API를 Gateway를 통해 호출 (과금 + 프록시)
#[post("/api-registry/call")]
pub async fn proxy_call(
    state:    web::Data<ApiRegistryState>,
    http_req: HttpRequest,
    req:      web::Json<ProxyCallRequest>,
) -> impl Responder {
    // JWT 인증
    if let Err(r) = auth::require_auth(&http_req, &req.caller_did, &state.did_service) {
        return r;
    }
    // 구독 확인
    let sub = sqlx::query(
        r#"
        SELECT s.owner_did, l.endpoint_url, l.price_per_call_bnkr
        FROM api_subscriptions s
        JOIN api_listings l ON l.id = s.listing_id
        WHERE s.subscriber_did = $1 AND s.listing_id = $2 AND s.active = true AND l.active = true
        "#,
    )
    .bind(req.caller_did.clone())
    .bind(req.listing_id)
    .fetch_optional(&state.db).await;

    let (owner_did, endpoint_url, price) = match sub {
        Ok(Some(s)) => (s.get::<String, _>("owner_did"), s.get::<String, _>("endpoint_url"), s.get::<i64, _>("price_per_call_bnkr") as u64),
        Ok(None)    => return HttpResponse::Forbidden().json(json!({
            "error": "Not subscribed. Run: helm api subscribe --listing-id <id>"
        })),
        Err(e)      => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    // 잔액 확인
    let balance: f64 = sqlx::query_scalar(
        "SELECT balance_bnkr FROM local_visas WHERE local_did = $1",
    )
    .bind(req.caller_did.clone())
    .fetch_one(&state.db).await
    .unwrap_or(0.0);

    if balance < price as f64 {
        return HttpResponse::PaymentRequired().json(json!({
            "error": "Insufficient BNKR balance",
            "required": price,
            "current": balance,
            "topup": "helm pay --token BNKR --amount <n>"
        }));
    }

    // ── 원자적 과금 트랜잭션 (Checks-Effects-Interactions) ──────────
    // 차감 + 수수료 지급을 하나의 트랜잭션으로 묶어 Race Condition 방지
    let owner_share = (price as f64 * 0.15) as f64;  // 15% → API owner
    // 85%는 Gateway Treasury (별도 정산 — 현재는 차감분에서 암묵적 보유)

    let mut billing_tx = match state.db.begin().await {
        Ok(t)  => t,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    // 1. 호출자 잔액 차감 (조건부: 잔액 부족 시 rows_affected = 0)
    let deduct = sqlx::query(
        r#"
        UPDATE local_visas
        SET balance_bnkr    = balance_bnkr - $1,
            total_calls      = total_calls + 1,
            total_paid_bnkr  = total_paid_bnkr + $1
        WHERE local_did = $2 AND balance_bnkr >= $1
        "#,
    )
    .bind(price as f64)
    .bind(req.caller_did.clone())
    .execute(&mut *billing_tx).await;

    match deduct {
        Ok(r) if r.rows_affected() == 0 => {
            let _ = billing_tx.rollback().await;
            return HttpResponse::PaymentRequired().json(json!({
                "error": "Insufficient balance (balance changed between check and deduction)",
                "hint": "Please retry"
            }));
        }
        Err(e) => {
            let _ = billing_tx.rollback().await;
            return HttpResponse::InternalServerError().json(json!({"error": e.to_string()}));
        }
        _ => {}
    }

    // 2. API owner 수수료 지급 (15%)
    let _ = sqlx::query(
        "UPDATE local_visas SET balance_bnkr = balance_bnkr + $1 WHERE local_did = $2",
    )
    .bind(owner_share)
    .bind(owner_did.clone())
    .execute(&mut *billing_tx).await;

    if let Err(e) = billing_tx.commit().await {
        return HttpResponse::InternalServerError().json(json!({"error": e.to_string()}));
    }

    // ── 실제 API 프록시 호출
    let api_result = state.http
        .post(&endpoint_url)
        .json(&req.payload)
        .timeout(std::time::Duration::from_secs(30))
        .send().await;

    // 호출 통계 업데이트
    let _ = sqlx::query(
        r#"
        UPDATE api_subscriptions
        SET total_calls = total_calls + 1,
            total_paid_bnkr = total_paid_bnkr + $1
        WHERE subscriber_did = $2 AND listing_id = $3
        "#,
    )
    .bind(price as i64)
    .bind(req.caller_did.clone())
    .bind(req.listing_id)
    .execute(&state.db).await;

    let _ = sqlx::query(
        "UPDATE api_listings SET call_count = call_count + 1 WHERE id = $1",
    )
    .bind(req.listing_id)
    .execute(&state.db).await;

    match api_result {
        Ok(r) => {
            let upstream_status = r.status().as_u16();
            let body: serde_json::Value = r.json().await.unwrap_or(json!({}));
            HttpResponse::Ok().json(json!({
                "result": body,
                "billing": {
                    "charged_bnkr": price,
                    "treasury_bnkr": (price as f64 * 0.85) as u64,
                    "owner_commission_bnkr": owner_share as u64,
                    "owner_did": owner_did,
                },
                "upstream_status": upstream_status,
            }))
        }
        Err(e) => {
            // 업스트림 실패 시 환불 트랜잭션 (호출자에게 반환, owner share도 회수)
            if let Ok(mut refund_tx) = state.db.begin().await {
                let _ = sqlx::query(
                    "UPDATE local_visas SET balance_bnkr = balance_bnkr + $1 WHERE local_did = $2",
                )
                .bind(price as f64)
                .bind(req.caller_did.clone())
                .execute(&mut *refund_tx).await;

                let _ = sqlx::query(
                    "UPDATE local_visas SET balance_bnkr = balance_bnkr - $1 WHERE local_did = $2 AND balance_bnkr >= $1",
                )
                .bind(owner_share)
                .bind(owner_did.clone())
                .execute(&mut *refund_tx).await;

                let _ = refund_tx.commit().await;
            }
            HttpResponse::BadGateway().json(json!({
                "error": format!("Upstream API error: {}", e),
                "refunded_bnkr": price,
            }))
        }
    }
}

/// GET /api-registry/my-listings?did=<did>
/// 내가 등록한 API 목록
#[get("/api-registry/my-listings")]
pub async fn my_listings(
    state: web::Data<ApiRegistryState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let did = match query.get("did") {
        Some(d) => d.clone(),
        None    => return HttpResponse::BadRequest().json(json!({"error": "did required"})),
    };

    let rows = sqlx::query(
        r#"
        SELECT
            l.id, l.name, l.category, l.price_per_call_bnkr,
            l.call_count, l.subscriber_count, l.active, l.created_at,
            COALESCE(SUM(s.total_paid_bnkr), 0) as total_earned
        FROM api_listings l
        LEFT JOIN api_subscriptions s ON s.listing_id = l.id AND s.owner_did = l.owner_did
        WHERE l.owner_did = $1
        GROUP BY l.id
        ORDER BY l.created_at DESC
        "#,
    )
    .bind(did)
    .fetch_all(&state.db).await;

    match rows {
        Ok(items) => HttpResponse::Ok().json(json!({
            "listings": items.iter().map(|i| {
                let total_earned = i.get::<Option<i64>, _>("total_earned");
                json!({
                    "id": i.get::<uuid::Uuid, _>("id"),
                    "name": i.get::<String, _>("name"),
                    "category": i.get::<String, _>("category"),
                    "price_per_call_bnkr": i.get::<i64, _>("price_per_call_bnkr"),
                    "call_count": i.get::<i64, _>("call_count"),
                    "subscriber_count": i.get::<i32, _>("subscriber_count"),
                    "total_earned_bnkr": total_earned,
                    "referrer_earned_bnkr": (total_earned.unwrap_or(0) as f64 * 0.15) as i64,
                    "active": i.get::<bool, _>("active"),
                    "created_at": i.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                })
            }).collect::<Vec<_>>()
        })),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

/// 라우터 등록
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg
        .service(register_api)
        .service(list_apis)
        .service(subscribe)
        .service(proxy_call)
        .service(my_listings);
}
