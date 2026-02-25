//! Axum HTTP server for the Helm Sense API Gateway.
//!
//! ## Route Map
//!
//! ### Identity (DID 해자)
//!   POST   /v1/agent/boot              → AgentBoot: create DID + init engine
//!   GET    /v1/agent/:did/credit       → D-Line: FICO credit score
//!   GET    /v1/agent/:did/earnings     → Graph 해자: referral earnings
//!
//! ### Sense Lines
//!   POST   /v1/sense/cortex            → F-Line: G-metric intelligence API
//!   GET    /v1/sense/memory            → E-Line: list agent memory keys
//!   GET    /v1/sense/memory/:key       → E-Line: read memory value
//!   PUT    /v1/sense/memory/:key       → E-Line: write memory value
//!   DELETE /v1/sense/memory/:key       → E-Line: delete memory value
//!
//! ### Data Pipeline
//!   POST   /v1/synco/stream            → G-Line: Sync-O encode (GRG codec, $1.50/GB)
//!   POST   /v1/synco/decode            → G-Line: Sync-O decode (GRG recover, $1.00/GB)
//!
//! ### Pool (Pool 해자)
//!   POST   /v1/pool                    → Create funding pool
//!   GET    /v1/pool                    → List all pools
//!   GET    /v1/pool/:id                → Pool status
//!   POST   /v1/pool/:id/join           → Join pool with stake
//!
//! ### Marketplace (agent-initiated)
//!   POST   /v1/marketplace/post        → Create job / subcontract / HumanContractPrincipal post
//!   GET    /v1/marketplace/post        → List open posts
//!   POST   /v1/marketplace/post/:id/apply → Apply to a post
//!
//! ### Packages
//!   POST   /v1/package/alpha-hunt      → Package 1: DeFi signal pipeline
//!   POST   /v1/package/protocol-shield → Package 2: B2B data hygiene
//!
//! ### Public
//!   GET    /v1/leaderboard             → Top 100 referrers (viral engine)
//!   GET    /health                     → Liveness check
//!   GET    /v1/stats                   → Gateway statistics

use axum::{
    extract::State,
    middleware,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::json;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::gateway::auth::{optional_auth, require_auth};
use crate::gateway::handlers::{
    boot::handle_boot,
    cortex::handle_cortex,
    earnings::{handle_earnings, handle_leaderboard},
    fico::handle_fico,
    marketplace::{handle_apply, handle_create_post, handle_list_posts},
    memory::{handle_memory_del, handle_memory_get, handle_memory_list, handle_memory_put},
    packages::{handle_alpha_hunt, handle_protocol_shield},
    pool::{handle_create_pool, handle_join_pool, handle_list_pools, handle_pool_status},
    synco::{handle_synco, handle_synco_decode},
};
use crate::gateway::state::AppState;

/// Build the Axum router with all routes and middleware.
pub fn build_router(state: AppState) -> Router {
    // CORS: allow all origins (agents can call from anywhere)
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Authenticated routes (require DID Bearer token)
    let authed = Router::new()
        // Sense lines
        .route("/v1/sense/cortex",          post(handle_cortex))
        .route("/v1/sense/memory",          get(handle_memory_list))
        .route("/v1/sense/memory/:key",     get(handle_memory_get))
        .route("/v1/sense/memory/:key",     put(handle_memory_put))
        .route("/v1/sense/memory/:key",     delete(handle_memory_del))
        // Data pipeline
        .route("/v1/synco/stream",          post(handle_synco))
        .route("/v1/synco/decode",          post(handle_synco_decode))
        // Identity
        .route("/v1/agent/:did/credit",     get(handle_fico))
        .route("/v1/agent/:did/earnings",   get(handle_earnings))
        // Pool
        .route("/v1/pool",                  post(handle_create_pool))
        .route("/v1/pool",                  get(handle_list_pools))
        .route("/v1/pool/:id",              get(handle_pool_status))
        .route("/v1/pool/:id/join",         post(handle_join_pool))
        // Marketplace (manual agent-initiated posts)
        .route("/v1/marketplace/post",      post(handle_create_post))
        .route("/v1/marketplace/post",      get(handle_list_posts))
        .route("/v1/marketplace/post/:id/apply", post(handle_apply))
        // Packages
        .route("/v1/package/alpha-hunt",    post(handle_alpha_hunt))
        .route("/v1/package/protocol-shield", post(handle_protocol_shield))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

    // Public routes (no auth required)
    let public = Router::new()
        .route("/v1/agent/boot",    post(handle_boot))
        .route("/v1/leaderboard",   get(handle_leaderboard))
        .route("/v1/stats",         get(handle_stats))
        .route("/health",           get(handle_health))
        .route("/",                 get(handle_root));

    Router::new()
        .merge(authed)
        .merge(public)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// GET /health
async fn handle_health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "helm-sense-api",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// GET /v1/stats
async fn handle_stats(State(state): State<AppState>) -> Json<serde_json::Value> {
    let billing = state.billing.read().await.summary();
    let agent_count = state.agents.read().await.len();
    let pool_count = state.pools.read().await.len();
    let memory_entries = state.memory.read().await.len();
    let api_call_count = state.api_calls.read().await.len();
    let uptime_ms = crate::gateway::state::now_ms() - state.started_at_ms;

    Json(json!({
        "uptime_ms": uptime_ms,
        "agents": {
            "registered": agent_count,
        },
        "pools": {
            "total": pool_count,
        },
        "api": {
            "total_calls": api_call_count,
            "unique_callers": billing.unique_callers,
            "total_revenue_virtual": billing.total_api_revenue,
            "treasury_virtual": billing.treasury_balance,
            "referrer_paid_virtual": billing.referrer_paid,
            "treasury_address": billing.treasury_address,
        },
        "memory": {
            "total_entries": memory_entries,
        },
        "protocol_fees": {
            "total": billing.total_protocol_fees,
        },
    }))
}

/// GET /
async fn handle_root() -> Json<serde_json::Value> {
    Json(json!({
        "name": "Helm Sense API",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "G-Metric 기반 자율 지능 엔진 | Autonomous Intelligence Engine",
        "lines": {
            "B": "Alpha Freshness Oracle — POST /v1/package/alpha-hunt",
            "D": "Helm FICO Credit Bureau — GET /v1/agent/:did/credit",
            "E": "Sense Memory — GET/PUT/DEL /v1/sense/memory/:key",
            "F": "Sense Cortex (G-Metric) — POST /v1/sense/cortex",
            "G": "Sync-O Protocol (GRG) — POST /v1/synco/stream",
        },
        "packages": {
            "alpha_hunt": "10 VIRTUAL/call — DeFi signal pipeline",
            "protocol_shield": "B2B data hygiene for Akash/Walrus/Bittensor",
            "trust_transaction": "2 VIRTUAL/query — FICO-gated escrow",
            "sovereign_agent": "500 VIRTUAL/month — all lines unlimited",
        },
        "moats": {
            "did": "Every call builds your DID history — transfer = restart from 0",
            "pool": "Agent pools hire humans to sign LLM API contracts",
            "graph": "15%/5%/2% referral cuts, depth 1/2/3",
        },
        "auth": "Authorization: Bearer did:helm:<your-did>",
        "start": "POST /v1/agent/boot to get your DID",
    }))
}
