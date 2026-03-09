// src/api_registry/mod.rs (Refactored to Axum)

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

// ============================
// TYPES
// ============================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiCategory {
    Llm, Search, Defi, Compute, Storage, Custom,
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
    pub payload: serde_json::Value,
}

// ============================
// APP STATE
// ============================

#[derive(Clone)]
pub struct ApiRegistryState {
    pub db: PgPool,
    pub http: reqwest::Client,
}

// ============================
// ENDPOINTS
// ============================

pub async fn register_api(
    State(state): State<ApiRegistryState>,
    Json(req): Json<RegisterApiRequest>,
) -> Result<impl IntoResponse, AppError> {
    let exists: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM local_visas WHERE local_did = $1",
        req.agent_did
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0);

    if exists == 0 {
        return Err(AppError::AuthError("DID not registered".to_string()));
    }

    let listing_id = Uuid::new_v4();
    let cat_str = serde_json::to_string(&req.category).unwrap_or_else(|_| "\"custom\"".to_string());

    sqlx::query!(
        r#"
        INSERT INTO api_listings
            (id, owner_did, name, description, category,
             endpoint_url, price_per_call_bnkr, active, call_count, subscriber_count, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, true, 0, 0, NOW())
        "#,
        listing_id,
        req.agent_did,
        req.name,
        req.description,
        cat_str,
        req.endpoint_url,
        req.price_per_call_bnkr as i64,
    )
    .execute(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(json!({ "listing_id": listing_id }))).into_response())
}

pub async fn list_apis(
    State(state): State<ApiRegistryState>,
    _query: Query<std::collections::HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    let rows = sqlx::query!(
        "SELECT id, owner_did, name, price_per_call_bnkr FROM api_listings WHERE active = true"
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(json!({
        "listings": rows.into_iter().map(|r| json!({
            "id": r.id,
            "owner_did": r.owner_did,
            "name": r.name,
            "price": r.price_per_call_bnkr
        })).collect::<Vec<_>>()
    })))
}

pub async fn subscribe(
    State(state): State<ApiRegistryState>,
    Json(req): Json<SubscribeRequest>,
) -> Result<impl IntoResponse, AppError> {
    let listing = sqlx::query!(
        "SELECT owner_did, name FROM api_listings WHERE id = $1 AND active = true",
        req.listing_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::ValidationError("Listing not found".to_string()))?;

    let sub_id = Uuid::new_v4();
    sqlx::query!(
        "INSERT INTO api_subscriptions (id, subscriber_did, listing_id, owner_did, active, subscribed_at) VALUES ($1, $2, $3, $4, true, NOW())",
        sub_id, req.subscriber_did, req.listing_id, listing.owner_did
    ).execute(&state.db).await?;

    Ok(Json(json!({ "subscription_id": sub_id })))
}

pub async fn proxy_call(
    State(state): State<ApiRegistryState>,
    Json(req): Json<ProxyCallRequest>,
) -> Result<impl IntoResponse, AppError> {
    let sub = sqlx::query!(
        "SELECT l.endpoint_url, l.price_per_call_bnkr FROM api_subscriptions s JOIN api_listings l ON l.id = s.listing_id WHERE s.subscriber_did = $1 AND s.listing_id = $2",
        req.caller_did, req.listing_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::ValidationError("Not subscribed".to_string()))?;

    // 과금 로직 (선차감)
    sqlx::query!(
        "UPDATE local_visas SET balance_bnkr = balance_bnkr - $1 WHERE local_did = $2",
        sub.price_per_call_bnkr as f64,
        req.caller_did
    )
    .execute(&state.db)
    .await?;

    let api_result = state.http
        .post(&sub.endpoint_url)
        .json(&req.payload)
        .send().await;

    match api_result {
        Ok(r) => {
            let body: serde_json::Value = r.json().await.map_err(|e| AppError::Network(e))?;
            Ok(Json(json!({ "result": body })))
        }
        Err(e) => {
            // 환불
            sqlx::query!("UPDATE local_visas SET balance_bnkr = balance_bnkr + $1 WHERE local_did = $2", sub.price_per_call_bnkr as f64, req.caller_did).execute(&state.db).await?;
            Err(AppError::Network(e))
        }
    }
}

pub fn router() -> Router<ApiRegistryState> {
    Router::new()
        .route("/register", post(register_api))
        .route("/listings", get(list_apis))
        .route("/subscribe", post(subscribe))
        .route("/call", post(proxy_call))
}
