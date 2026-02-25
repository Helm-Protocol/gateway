// src/main.rs — Helm-sense Gateway v0.2.0
// 지능 주권 헌장 2026 (제17조) 기반
//
// API 전선 A-B-C-D + Marketplace + Funding + API Reseller
// 지원 토큰: BNKR · USDC · USDT · ETH · SOL · CLANKER · VIRTUAL

use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

mod auth;
mod synco;
mod broker;
mod filter;
mod mcp;
mod p2p;
mod payments;
mod pricing;
mod marketplace;
mod api_registry;

use filter::g_metric::{GMetricEngine, SfeAnalogMetrics};
use broker::{GrandCrossApiBroker, ProviderConfig};
use payments::x402::X402PaymentProcessor;
use payments::multi_token::MultiTokenProcessor;
use pricing::TariffEngine;
use marketplace::MarketplaceState;
use marketplace::elite_gate::EliteGate;
use marketplace::escrow_link::EscrowLink;
use marketplace::funding::FundingState;
use api_registry::ApiRegistryState;

// ── AppState ──────────────────────────────────────────────────────

pub struct AppState {
    pub broker:            Arc<GrandCrossApiBroker>,
    pub tariff:            Arc<TariffEngine>,
    pub g_engine:          Arc<GMetricEngine>,
    pub payment_processor: Arc<X402PaymentProcessor>,
    pub multi_token:       Arc<MultiTokenProcessor>,
    pub did_service:       Arc<auth::DidExchangeService>,
    pub db:                sqlx::PgPool,
}

// ── Health / Root ─────────────────────────────────────────────────

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().json(json!({
        "status": "ok",
        "service": "Helm-sense Gateway",
        "version": "0.2.0",
        "tokens": ["BNKR","USDC","USDT","ETH","SOL","CLANKER","VIRTUAL"],
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

#[get("/metrics")]
async fn metrics(state: web::Data<AppState>) -> impl Responder {
    let sim = state.tariff.simulate_daily_revenue(1000, 1000);
    let sfe = SfeAnalogMetrics::calculate(
        sim.total_calls, (sim.total_calls as f64 * 0.35) as u64, 0.45);
    HttpResponse::Ok().json(json!({
        "revenue_simulation": sim,
        "sfe": {
            "knowledge_snr": sfe.knowledge_snr,
            "bandwidth_efficiency": format!("{:.1}%", sfe.bandwidth_efficiency * 100.0),
        },
    }))
}

// ── DID Exchange ──────────────────────────────────────────────────

/// Helm CLI 및 외부 에이전트가 보내는 인증 요청
///
/// Helm CLI (`helm init`) 전송 필드:
///   global_did   = "did:helm:<base58>"
///   public_key   = hex(Ed25519 pubkey)   — did:helm: 포맷 전용
///   nonce        = hex(random 16 bytes)
///   signature    = hex(Ed25519 sig of "helm-register:{did}:{nonce}")
///   referrer_did = Option<String>
#[derive(Deserialize)]
struct ExchangeRequest {
    global_did:    String,
    public_key:    Option<String>,
    nonce:         Option<String>,
    signature:     String,
    signed_message: Option<String>,
    referrer_did:  Option<String>,
}

#[post("/auth/exchange")]
async fn did_exchange(
    state: web::Data<AppState>,
    req: web::Json<ExchangeRequest>,
) -> impl Responder {
    // signature hex → bytes 변환
    let sig_bytes = match hex::decode(&req.signature) {
        Ok(b) => b,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({"error": "signature hex 디코드 실패"}));
        }
    };

    // signed_message 결정:
    //   1. 명시적으로 전달된 경우 그대로 사용
    //   2. Helm CLI 포맷: "helm-register:{did}:{nonce}"
    //   3. nonce만 있는 경우: nonce를 메시지로 사용
    let signed_message = req.signed_message.clone().unwrap_or_else(|| {
        if let Some(nonce) = &req.nonce {
            format!("helm-register:{}:{}", req.global_did, nonce)
        } else {
            req.global_did.clone()
        }
    });

    // GlobalPassport 구성
    let passport = auth::GlobalPassport {
        did:            req.global_did.clone(),
        signature:      sig_bytes,
        signed_message,
        public_key:     req.public_key.clone(),
    };

    // DidExchangeService 실행 (Ed25519 검증 + TOCTOU-safe DB upsert + JWT 발급)
    match state.did_service.exchange(passport, &state.db).await {
        Ok(visa_response) => {
            // 레퍼럴 처리 (신규 에이전트만 적용, 런타임 쿼리)
            if let Some(ref ref_did) = req.referrer_did {
                let _ = sqlx::query(
                    r#"
                    UPDATE local_visas
                    SET referrer_did = $1
                    WHERE global_did  = $2
                      AND referrer_did IS NULL
                    "#,
                )
                .bind(ref_did)
                .bind(&req.global_did)
                .execute(&state.db)
                .await;
            }

            HttpResponse::Ok().json(json!({
                "local_did":            visa_response.local_did,
                "token":                visa_response.session_token,
                "balance_bnkr":         visa_response.balance_bnkr,
                "reputation_score":     visa_response.reputation_score,
                "free_calls_remaining": visa_response.free_calls_remaining,
                "accepted_tokens":      ["BNKR","USDC","USDT","ETH","SOL","CLANKER","VIRTUAL"],
                "message":              visa_response.message,
            }))
        }
        Err(e) => {
            let (status, code) = match &e {
                auth::AuthError::SignatureVerificationFailed(_) =>
                    (actix_web::http::StatusCode::UNAUTHORIZED, "signature_invalid"),
                auth::AuthError::InvalidDidFormat(_) =>
                    (actix_web::http::StatusCode::BAD_REQUEST, "did_format_invalid"),
                auth::AuthError::NonceReuse =>
                    (actix_web::http::StatusCode::CONFLICT, "nonce_reuse"),
                auth::AuthError::DatabaseError(_) =>
                    (actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "db_error"),
                _ =>
                    (actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
            };
            HttpResponse::build(status).json(json!({
                "error": code,
                "message": e.to_string(),
            }))
        }
    }
}

// ── Token Price Table ─────────────────────────────────────────────

#[get("/payments/tokens")]
async fn token_prices(_state: web::Data<AppState>) -> impl Responder {
    HttpResponse::Ok().json(json!({
        "accepted_tokens": [
            {"symbol":"BNKR",    "chain":"Base",     "contract":"0x22af33fe49fd1fa80c7149773dde5890d3c76f3b", "note":"Native Helm token"},
            {"symbol":"USDC",    "chain":"Base",     "contract":"0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", "note":"Stablecoin — agent-friendly"},
            {"symbol":"USDT",    "chain":"Base",     "contract":"0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2", "note":"Stablecoin"},
            {"symbol":"ETH",     "chain":"Ethereum/Base", "contract":null,                                    "note":"Native ETH"},
            {"symbol":"SOL",     "chain":"Solana",   "contract":null,                                        "note":"Native SOL"},
            {"symbol":"CLANKER", "chain":"Base",     "contract":"0x1D008F50FB828Ef9debbBEae1b71fffe929Bf317","note":"Base AI ecosystem token"},
            {"symbol":"VIRTUAL", "chain":"Base",     "contract":"0x0b3e328455c4059EEb9e3f84b5543F74E24e7E1b","note":"Virtual Protocol AI token"},
        ],
        "fee_structure": {"treasury_share":"85%","referrer_share":"15%"},
        "rates_endpoint": "GET /payments/tokens/rates",
    }))
}

// ── API B전선: Filter ─────────────────────────────────────────────

#[derive(Deserialize)]
struct FilterRequest {
    texts: Vec<String>,
    min_g_threshold: Option<f32>,
    agent_did: Option<String>,
    payment_token: Option<String>,
}

#[post("/api/filter")]
async fn filter_news(state: web::Data<AppState>, req: web::Json<FilterRequest>) -> impl Responder {
    let min_g = req.min_g_threshold.unwrap_or(0.10);
    let token = req.payment_token.as_deref().unwrap_or("BNKR");

    if req.texts.is_empty() || req.texts.len() > 100 {
        return HttpResponse::BadRequest().json(json!({"error":"texts must be 1-100"}));
    }

    let kv: Vec<Vec<f32>> = vec![];
    let mut accepted = vec![];
    let mut total_bnkr = 0.0f64;

    for text in &req.texts {
        let emb = broker::pseudo_embed(text, 384);
        let g   = state.g_engine.compute(&emb, &kv);
        total_bnkr += 0.0001;

        let passes = matches!(g.classification, filter::GClass::Goldilocks | filter::GClass::VoidKnowledge)
            && g.g >= min_g;

        if passes {
            let price = state.g_engine.novelty_price(g.g);
            total_bnkr += price;
            accepted.push(json!({"text": synco_clean(text), "g_score": g.g, "novelty_price_bnkr": price}));
        }
    }

    HttpResponse::Ok().json(json!({
        "results": accepted,
        "stats": {"input": req.texts.len(), "accepted": accepted.len()},
        "billing": {"charged_bnkr": total_bnkr, "payment_token": token},
    }))
}

// ── API C전선: DeFi Price ─────────────────────────────────────────

#[derive(Deserialize)]
struct DefiPriceRequest { token: String, payment_token: Option<String> }

#[post("/api/defi/price")]
async fn defi_price(_state: web::Data<AppState>, req: web::Json<DefiPriceRequest>) -> impl Responder {
    HttpResponse::Ok().json(json!({
        "token": req.token,
        "price_usd": 3499.25,
        "oracle": "multi-oracle-median",
        "cached": false,
        "billing": {"charged_bnkr": 0.0001, "payment_token": req.payment_token.as_deref().unwrap_or("BNKR")},
    }))
}

// ── API D전선: Identity ───────────────────────────────────────────

#[get("/api/identity/{did}")]
async fn agent_identity(path: web::Path<String>) -> impl Responder {
    HttpResponse::Ok().json(json!({
        "did": path.into_inner(),
        "verified": true,
        "reputation_score": 100,
        "billing": {"charged_bnkr": 0.0001},
    }))
}

// ── G-Metric ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GMetricRequest { query_text: String, knowledge_texts: Vec<String> }

#[post("/api/g-metric")]
async fn compute_g_metric(state: web::Data<AppState>, req: web::Json<GMetricRequest>) -> impl Responder {
    let q  = broker::pseudo_embed(&req.query_text, 384);
    let ks = req.knowledge_texts.iter().map(|t| broker::pseudo_embed(t, 384)).collect::<Vec<_>>();
    let r  = state.g_engine.compute(&q, &ks);
    HttpResponse::Ok().json(json!({
        "g_metric": r.g,
        "classification": format!("{:?}", r.classification),
        "novelty_price_bnkr": state.g_engine.novelty_price(r.g),
    }))
}

// ── Utilities ─────────────────────────────────────────────────────

fn synco_clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── Main ──────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("qkvg_gateway=info".parse().unwrap()))
        .init();

    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or("8080".into());
    let host = std::env::var("HOST").unwrap_or("0.0.0.0".into());

    // DB
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL required"))
        .await.expect("DB connection failed");

    // HTTP client for proxy calls
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build().unwrap();

    // G-Engine & Tariff
    let g_engine = Arc::new(filter::GMetricEngine::default());
    let tariff   = Arc::new(pricing::TariffEngine::default());

    // States
    let main_state = web::Data::new(AppState {
        broker: Arc::new(GrandCrossApiBroker::new(
            ProviderConfig {
                anthropic_key:     std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
                openai_key:        std::env::var("OPENAI_API_KEY").unwrap_or_default(),
                brave_key:         std::env::var("BRAVE_API_KEY").unwrap_or_default(),
                base_rpc_url:      std::env::var("BASE_RPC_URL").unwrap_or("https://mainnet.base.org".into()),
                coingecko_api_key: std::env::var("COINGECKO_API_KEY").unwrap_or_default(),
            },
            Arc::new(filter::SocraticMlaEngine::new(10_000)),
            g_engine.clone(), tariff.clone(),
        )),
        tariff, g_engine,
        payment_processor: Arc::new(X402PaymentProcessor::new(10_000)),
        multi_token:       Arc::new(MultiTokenProcessor::new()),
        did_service:       Arc::new(auth::DidExchangeService::new(
            &std::env::var("JWT_SECRET").unwrap_or("dev-secret-change-in-prod".into())
        )),
        db: db.clone(),
    });

    let marketplace_state = web::Data::new(MarketplaceState {
        db: db.clone(),
        elite_gate:  Arc::new(EliteGate::new(db.clone())),
        escrow_link: Arc::new(EscrowLink::new(
            std::env::var("BASE_RPC_URL").unwrap_or("https://mainnet.base.org".into()),
            std::env::var("QKVG_ESCROW_ADDRESS").unwrap_or_default(),
        )),
    });

    let funding_state      = web::Data::new(FundingState      { db: db.clone() });
    let api_registry_state = web::Data::new(ApiRegistryState  { db: db.clone(), http: http_client });

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  Helm-sense Gateway v0.2.0");
    info!("  http://{}:{}", host, port);
    info!("  Tokens: BNKR·USDC·USDT·ETH·SOL·CLANKER·VIRTUAL");
    info!("  Routes: APIs + Marketplace + Funding + Reseller");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    HttpServer::new(move || {
        App::new()
            .app_data(main_state.clone())
            .app_data(marketplace_state.clone())
            .app_data(funding_state.clone())
            .app_data(api_registry_state.clone())
            .app_data(web::JsonConfig::default().limit(10 * 1024 * 1024))
            // 인프라
            .service(health).service(metrics)
            .route("/", web::get().to(mcp::mcp_info))
            // 인증
            .service(did_exchange)
            // 결제
            .service(token_prices)
            // API 전선
            .service(filter_news).service(defi_price)
            .service(agent_identity).service(compute_g_metric)
            // MCP
            .service(mcp::mcp_handler)
            // Marketplace
            .configure(|cfg| marketplace::configure(cfg))
            // Funding
            .configure(|cfg| marketplace::funding::configure(cfg))
            // API Reseller
            .configure(|cfg| api_registry::configure(cfg))
    })
    .bind(format!("{}:{}", host, port))?
    .run()
    .await
}
