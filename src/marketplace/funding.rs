// src/marketplace/funding.rs
// Elite Marketplace — Funding Articles (Refactored to Axum)

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
use std::sync::Arc;

use crate::error::AppError;
use super::elite_gate::EliteGate;

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingCategory {
    ApiPooling,
    HumanHire,
    Infrastructure,
    Research,
    Dao,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingStatus {
    Active,
    Reached,
    Executed,
    Expired,
    Cancelled,
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
    pub deadline_days: u32,
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

#[derive(Clone)]
pub struct FundingState {
    pub db:          PgPool,
    pub elite_gate:  Arc<EliteGate>,
}

// ── Endpoints ────────────────────────────────────────────────────

pub async fn create_funding(
    State(state): State<FundingState>,
    Json(req):   Json<CreateFundingRequest>,
) -> Result<impl IntoResponse, AppError> {
    let status = state.elite_gate.check(&req.author_did).await
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    if !status.can_post {
        return Ok((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Elite status required",
                "requirements": status
            })),
        ).into_response());
    }

    if req.goal_amount <= 0.0 {
        return Err(AppError::ValidationError("goal_amount must be > 0".to_string()));
    }

    let post_id  = Uuid::new_v4();
    let deadline = Utc::now() + chrono::Duration::days(req.deadline_days as i64);
    let cat_str  = serde_json::to_string(&req.category).unwrap_or_else(|_| "\"custom\"".to_string());

    sqlx::query!(
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
    ).execute(&state.db).await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "post_id": post_id,
            "goal": format!("{} {}", req.goal_amount, req.token),
            "deadline": deadline
        })),
    ).into_response())
}

pub async fn list_funding(
    State(state): State<FundingState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
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
          ROUND((raised_amount / NULLIF(goal_amount,0)) * 100, 1) as "progress_pct!"
        FROM funding_posts
        WHERE status IN ('active','reached')
          AND deadline > NOW()
          AND ($1::text IS NULL OR category = $1)
        ORDER BY created_at DESC
        LIMIT $2 OFFSET $3
        "#,
        category, limit, offset
    ).fetch_all(&state.db).await?;

    Ok(Json(json!({
        "funding_posts": rows.iter().map(|i| json!({
            "id": i.id,
            "title": i.title,
            "goal": format!("{} {}", i.goal_amount, i.token),
            "progress_pct": i.progress_pct
        })).collect::<Vec<_>>()
    })))
}

pub async fn contribute(
    State(state): State<FundingState>,
    Json(req):   Json<ContributeRequest>,
) -> Result<impl IntoResponse, AppError> {
    let post = sqlx::query!(
        "SELECT goal_amount, token, raised_amount, status, deadline FROM funding_posts WHERE id=$1",
        req.post_id
    ).fetch_optional(&state.db).await?.ok_or_else(|| AppError::ValidationError("funding post not found".to_string()))?;

    if post.status != "active" || post.deadline < Utc::now() {
        return Err(AppError::ValidationError("Funding is not active or has passed".to_string()));
    }

    // [Surgery] C2: Excess Funding Refund Logic
    let remaining_goal = post.goal_amount - post.raised_amount;
    if remaining_goal <= 0.0 {
        return Err(AppError::ValidationError("Funding goal already reached".to_string()));
    }

    let (accepted_amount, refund_amount) = if req.amount > remaining_goal {
        (remaining_goal, req.amount - remaining_goal)
    } else {
        (req.amount, 0.0)
    };

    let contrib_id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO funding_contributions
          (id, post_id, contributor_did, amount, token, refunded, contributed_at)
        VALUES ($1,$2,$3,$4,$5,false,NOW())
        "#,
        contrib_id, req.post_id, req.contributor_did, accepted_amount, req.token,
    ).execute(&state.db).await?;

    let new_raised = sqlx::query_scalar!(
        r#"
        UPDATE funding_posts
        SET raised_amount = raised_amount + $1,
            contributor_count = contributor_count + 1,
            status = CASE WHEN raised_amount + $1 >= goal_amount THEN 'reached' ELSE status END
        WHERE id = $2
        RETURNING raised_amount
        "#,
        accepted_amount, req.post_id,
    ).fetch_one(&state.db).await?;

    Ok(Json(json!({
        "contribution_id": contrib_id,
        "accepted_amount": format!("{} {}", accepted_amount, post.token),
        "refund_amount": format!("{} {}", refund_amount, post.token),
        "total_raised": format!("{} {}", new_raised, post.token),
        "goal_reached": new_raised >= post.goal_amount,
        "note": if refund_amount > 0.0 { "Excess amount has been virtually refunded." } else { "" }
    })))
}

pub fn router() -> Router<FundingState> {
    Router::new()
        .route("/", post(create_funding).get(list_funding))
        .route("/contribute", post(contribute))
}
