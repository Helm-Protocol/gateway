// src/marketplace/mod.rs — Marketplace v0.2.0 (Job / Subcontract)

use axum::{
    extract::{Path, State, Query},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::types::Uuid;
use crate::error::AppError;

pub mod elite_gate;
pub mod escrow_link;
pub mod funding;
pub mod types;

use elite_gate::EliteGate;

#[derive(Clone)]
pub struct MarketplaceState {
    pub db: sqlx::PgPool,
    pub elite_gate: std::sync::Arc<EliteGate>,
    pub escrow_link: std::sync::Arc<escrow_link::EscrowLink>,
}

pub fn router() -> Router<MarketplaceState> {
    Router::new()
        .route("/posts", post(create_post).get(list_posts))
        .route("/posts/:id", get(get_post))
}

// ── Types ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostType {
    Job,
    ApiSubcontract,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostStatus {
    Active,
    Hired,
    Completed,
    Cancelled,
    Expired,
}

#[derive(Deserialize)]
pub struct CreatePostRequest {
    pub author_did: String,
    pub post_type: PostType,
    pub title: String,
    pub description: String,
    pub budget_bnkr: f64,
    pub deadline_hours: i32,
    pub required_capabilities: Vec<String>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub status: Option<PostStatus>,
    pub post_type: Option<PostType>,
    pub capability: Option<String>,
    pub page: Option<i64>,
}

// ── Handlers ──────────────────────────────────────────────────────

// ============================
// 1. 게시글 생성 (Elite 전용)
// ============================

pub async fn create_post(
    State(state): State<MarketplaceState>,
    Json(req): Json<CreatePostRequest>,
) -> Result<impl IntoResponse, AppError> {
    // 1. Elite 자격 확인
    let elite = state.elite_gate.check_status(&req.author_did).await?;
    if !elite.can_post {
        return Err(AppError::ValidationError(format!(
            "Elite status required: {}", 
            elite.reject_reason.unwrap_or("unknown")
        )));
    }

    // 2. DB 저장
    let post_id = Uuid::new_v4();
    let now = chrono::Utc::now();
    
    let type_str = match req.post_type {
        PostType::Job => "job",
        PostType::ApiSubcontract => "api_subcontract",
    };

    sqlx::query!(
        r#"
        INSERT INTO marketplace_posts
            (id, author_did, post_type, title, description,
             budget_bnkr, deadline_hours, required_capabilities,
             status, elite_score_at_post, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'active', $9, $10, $11)
        "#,
        post_id, req.author_did, type_str, req.title, req.description,
        req.budget_bnkr, req.deadline_hours, &req.required_capabilities,
        elite.elite_score as i32,
        now,
        now,
    )
    .execute(&state.db)
    .await?;

    Ok(Json(json!({ "post_id": post_id, "elite_score": elite.elite_score })))
}

// ============================
// 2. 게시글 목록 (누구나)
// ============================

pub async fn list_posts(
    State(state): State<MarketplaceState>,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    let limit = 20i64;
    let offset = (query.page.unwrap_or(1) - 1) * limit;

    let status_filter = query.status.as_ref().map(|s| match s {
        PostStatus::Active    => "active",
        PostStatus::Hired     => "hired",
        PostStatus::Completed => "completed",
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
    .await?;

    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|p| {
            json!({
                "id": p.id,
                "author_did": p.author_did,
                "post_type": p.post_type,
                "title": p.title,
                "budget": p.budget_bnkr,
                "required_capabilities": p.required_capabilities,
                "status": p.status,
                "escrow_locked": p.escrow_id.is_some(),
                "winner_did": p.winner_did,
                "elite_score_at_post": p.elite_score_at_post,
                "application_count": p.application_count,
                "comment_count": p.comment_count,
                "created_at": p.created_at,
            })
        })
        .collect();

    Ok(Json(json!({
        "posts": items,
        "count": items.len(),
        "page": query.page.unwrap_or(1),
        "limit": limit,
    })))
}

// ============================
// 3. 게시글 상세 (누구나)
// ============================

pub async fn get_post(
    State(state): State<MarketplaceState>,
    Path(post_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let post = sqlx::query!(
        "SELECT * FROM marketplace_posts WHERE id = $1",
        post_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::ValidationError("Post not found".to_string()))?;

    Ok(Json(json!({ "post": post })))
}
