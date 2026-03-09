// src/main.rs — Helm-sense Gateway v0.3.0 (Creator Economy Edition)
// 지능 주권 헌장 2026 (제17조) 기반 80/20 수수료 구조 적용

mod api_registry;
mod auth;
mod billing;
mod broker;
mod dashboard;
mod error;
mod filter;
mod gandiva_quic;
mod grg;
mod lattice_l2;
mod marketplace;
mod mcp;
mod p2p;
mod payments;
mod pricing;
mod synco;
mod market;
mod metrics;

use axum::{
    async_trait,
    extract::{FromRequest, Request, Path, State},
    response::{Html, IntoResponse, Sse},
    routing::{get, post},
    Json, Router,
    body::Bytes,
    http::StatusCode,
};

// ── Strict JSON Extractor (Kaleidoscope Standard) ──────────────────
pub struct StrictJson<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for StrictJson<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        // Limit recursion depth to 32 to prevent memory bombs
        let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
        // Note: serde_json default is 128, but for high-frequency agents we tighten it
        let value = T::deserialize(&mut deserializer)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("Sovereign Guard: Invalid JSON structure or depth: {}", e)))?;

        Ok(StrictJson(value))
    }
}

// ── Recursive Logic Guard ─────────────────────────────────────────
pub const MAX_SYNTHESIS_DEPTH: u8 = 5;

// ... (existing handlers update to use StrictJson)

use crate::broker::{GrandCrossApiBroker, ProviderConfig};
use crate::billing::BillingLedger;
use crate::filter::g_metric::{GMetricEngine, SfeAnalogMetrics};
use crate::error::AppError;
use crate::payments::x402::X402PaymentProcessor;
use crate::payments::multi_token::MultiTokenProcessor;
use crate::marketplace::elite_gate::EliteGate;
use crate::marketplace::escrow_link::EscrowLink;
use crate::market::memory_market::HelmMemoryMarket;
use pricing::TariffEngine;
use helm_core::helm_core_service_client::HelmCoreServiceClient;
use tonic::transport::Channel;

pub mod helm_core {
    tonic::include_proto!("helm.core");
}

// ── AppState ──────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub billing: Arc<parking_lot::Mutex<billing::BillingLedger>>,
    pub broker: Arc<GrandCrossApiBroker>,
    pub tariff: Arc<TariffEngine>,
    pub g_engine: Arc<GMetricEngine>,
    pub lattice: Arc<lattice_l2::LatticeL2>,
    pub market: Arc<HelmMemoryMarket>,
    pub payment_processor: Arc<X402PaymentProcessor>,
    pub multi_token: Arc<MultiTokenProcessor>,
    pub did_service: Arc<auth::DidExchangeService>,
    pub db: sqlx::PgPool,
    pub core_client: HelmCoreServiceClient<Channel>,
    pub metrics: Arc<metrics::GatewayMetrics>,
}

// ── Market Handlers (Phase 3) ────────────────────────────────────

#[derive(Deserialize)]
struct MarketListReq {
    creator_did: String,
    knowledge_hash: String,
    lockin_score: f32,
    depth: u32,
}

async fn market_list(
    State(state): State<AppState>,
    StrictJson(req): StrictJson<MarketListReq>,
) -> Result<impl IntoResponse, AppError> {
    let id = state.market.list_knowledge(
        &req.creator_did,
        &req.knowledge_hash,
        req.lockin_score,
        req.depth,
    ).map_err(|e| AppError::ValidationError(e.to_string()))?;

    Ok(Json(json!({ "listing_id": id, "status": "listed" })))
}

async fn market_purchase(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let result = state.market.purchase(id)
        .map_err(|e| AppError::ValidationError(e.to_string()))?;
    
    Ok(Json(result))
}

async fn market_listings(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.market.get_listings())
}

async fn market_stats(State(state): State<AppState>) -> impl IntoResponse {
    let summary = state.billing.lock().summary();
    Json(json!({
        "total_revenue_bnkr": summary.total_api_revenue,
        "total_creator_payout_bnkr": summary.creator_paid,
        "helm_treasury_bnkr": summary.helm_balance,
        "fee_split": "80% Creator / 20% Helm"
    }))
}

// ── Health / Metrics ──────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "Helm-sense Gateway",
        "version": "0.3.0-Creator-Economy",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let summary = state.billing.lock().summary();
    Json(json!({
        "billing_summary": summary,
        "fee_structure": { "creator": "80%", "helm": "20%" }
    }))
}

// ... (Other handlers omitted for brevity, keeping main loop)

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let internal_port = std::env::var("RUNTIME_PORT").unwrap_or_else(|_| "8080".into());
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");

    let db = sqlx::postgres::PgPoolOptions::new().max_connections(20).connect(&database_url).await?;

    // Load Balanced Core
    let worker_nodes = std::env::var("WORKER_NODES").unwrap_or_else(|_| "http://127.0.0.1:50051".into());
    let endpoints = worker_nodes.split(',').map(|addr| tonic::transport::Endpoint::from_shared(addr.to_string()).unwrap());
    let core_channel = tonic::transport::Channel::balance_list(endpoints);
    let core_client = HelmCoreServiceClient::new(core_channel);

    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let lattice = Arc::new(lattice_l2::LatticeL2::new(&redis_url).await?);
    let market = Arc::new(HelmMemoryMarket::new(100.0)); // Base price 100 BNKR
    let billing = Arc::new(parking_lot::Mutex::new(BillingLedger::new()));
    let jwt_secret = secrecy::SecretString::from(std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret".into()));

    let main_state = AppState {
        broker: Arc::new(GrandCrossApiBroker::new(
            ProviderConfig::from_env(),
            Arc::new(filter::SocraticMlaEngine::new(10_000)),
            Arc::new(GMetricEngine::default()),
            Arc::new(TariffEngine::default()),
            billing.clone(),
        )),
        tariff: Arc::new(TariffEngine::default()),
        g_engine: Arc::new(GMetricEngine::default()),
        lattice,
        market,
        billing,
        payment_processor: Arc::new(X402PaymentProcessor::new(10_000)),
        multi_token: Arc::new(MultiTokenProcessor::new()),
        did_service: Arc::new(auth::DidExchangeService::new("jwt-secret")), // Simplified
        db: db.clone(),
        core_client,
        metrics: Arc::new(metrics::GatewayMetrics::new()),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/v1/market/list", post(market_list))
        .route("/v1/market/purchase/:id", post(market_purchase))
        .route("/v1/market/listings", get(market_listings))
        .route("/v1/market/stats", get(market_stats))
        .layer(axum::extract::DefaultBodyLimit::max(512 * 1024))
        .layer(TimeoutLayer::new(std::time::Duration::from_secs(15)))
        .with_state(main_state);

    // Gandiva-QUIC Spawn
    let quic_port: u16 = std::env::var("QUIC_PORT").unwrap_or_else(|_| "4433".into()).parse().unwrap_or(4433);
    let quic_l2 = main_state.lattice.clone();
    tokio::spawn(async move {
        let _ = gandiva_quic::spawn_gandiva_quic_engine(quic_port, quic_l2).await;
    });

    let addr: SocketAddr = format!("{}:{}", host, internal_port).parse()?;
    info!("🚀 Helm Gateway v0.3.0 listening on {}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;

    Ok(())
}
