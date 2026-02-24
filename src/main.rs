// src/main.rs
// Helm-sense Gateway — 메인 서버
//
// 지능 주권 헌장 2026 (제17조) 기반
// AI 에이전트 API 중개 및 G-Metric 필터링
//
// 엔드포인트:
//   POST /auth/exchange      — DID Passport → Local Visa
//   POST /api/filter         — B전선: 뉴스/텍스트 Helm-sense 필터
//   POST /api/search         — B전선: Brave Search + Helm-sense
//   POST /api/llm            — A전선: LLM 도매 중개
//   POST /api/defi/price     — C전선: 다중 오라클 가격
//   GET  /api/identity/{did} — D전선: 에이전트 평판 조회
//   POST /mcp                — MCP JSON-RPC (Claude/Cursor 연동)
//   GET  /                   — 서버 정보 + MCP 안내
//   GET  /health             — 헬스체크
//   GET  /metrics            — G-Metric 수익 통계

use actix_web::{get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

mod auth;
mod broker;
mod filter;
mod mcp;
mod p2p;
mod payments;
mod pricing;

use filter::g_metric::{GMetricEngine, SfeAnalogMetrics};
use filter::qkvg::VectorCache;
use broker::{GrandCrossApiBroker, ProviderConfig};
use payments::x402::X402PaymentProcessor;
use pricing::TariffEngine;

// ============================
// APP STATE
// ============================

pub struct AppState {
    pub broker: Arc<GrandCrossApiBroker>,
    pub tariff: Arc<TariffEngine>,
    pub g_engine: Arc<GMetricEngine>,
    pub payment_processor: Arc<X402PaymentProcessor>,
    pub did_service: Arc<auth::DidExchangeService>,
    // DB 풀 (실제 운영 시 활성화)
    // pub db: sqlx::PgPool,
}

// ============================
// HEALTH CHECK
// ============================

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().json(json!({
        "status": "ok",
        "service": "Helm-sense Gateway",
        "version": "0.1.0",
        "charter": "지능 주권 헌장 2026",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

// ============================
// G-METRIC 수익 통계
// ============================

#[get("/metrics")]
async fn metrics(state: web::Data<AppState>) -> impl Responder {
    let sim = state.tariff.simulate_daily_revenue(1000, 1000);
    let sfe = SfeAnalogMetrics::calculate(
        sim.total_calls,
        (sim.total_calls as f64 * 0.35) as u64,
        0.45,
    );

    HttpResponse::Ok().json(json!({
        "revenue_simulation": sim,
        "sfe_analog_metrics": {
            "knowledge_snr": sfe.knowledge_snr,
            "bandwidth_efficiency": format!("{:.1}%", sfe.bandwidth_efficiency * 100.0),
            "information_purity": sfe.information_purity,
            "interpretation": {
                "snr": "knowledge_snr > 1.0 = 신규 정보 > 기존 정보 (좋음)",
                "efficiency": "실제 가치 있는 데이터 비율",
                "purity": "평균 G-Metric (높을수록 수익)"
            }
        },
        "g_metric_thresholds": {
            "parallel_below": 0.10,
            "goldilocks_zone": "0.10 ~ 0.80",
            "orthogonal_above": 0.80,
        }
    }))
}

// ============================
// DID EXCHANGE
// ============================

#[derive(Deserialize)]
struct ExchangeRequest {
    global_did: String,
    signature: String,    // hex 인코딩
    signed_message: String,
}

#[post("/auth/exchange")]
async fn did_exchange(
    state: web::Data<AppState>,
    req: web::Json<ExchangeRequest>,
) -> impl Responder {
    let sig_bytes = match hex::decode(&req.signature) {
        Ok(b) => b,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({
                "error": "서명 hex 디코드 실패"
            }));
        }
    };

    let passport = auth::GlobalPassport {
        did: req.global_did.clone(),
        signature: sig_bytes,
        signed_message: req.signed_message.clone(),
    };

    // 실제 운영: DB 연결 필요
    // match state.did_service.exchange(passport, &state.db).await { ... }

    // 개발 모드: 더미 응답
    HttpResponse::Ok().json(json!({
        "local_did": format!("did:qkvg:agent_{}", ulid::Ulid::new()),
        "session_token": "dev-mode-token",
        "balance_bnkr": 0.0,
        "reputation_score": 100,
        "free_calls_remaining": 100,
        "message": "Welcome to Helm-sense Gateway. 지능 주권 헌장 2026 준수 네트워크에 오신 것을 환영합니다."
    }))
}

// ============================
// B전선: 뉴스 필터
// ============================

#[derive(Deserialize)]
struct FilterRequest {
    texts: Vec<String>,
    topic: Option<String>,
    min_g_threshold: Option<f32>,
    agent_did: Option<String>,
}

#[post("/api/filter")]
async fn filter_news(
    state: web::Data<AppState>,
    req: web::Json<FilterRequest>,
) -> impl Responder {
    let texts = &req.texts;
    let min_g = req.min_g_threshold.unwrap_or(0.10);

    if texts.is_empty() || texts.len() > 100 {
        return HttpResponse::BadRequest().json(json!({
            "error": "texts는 1~100개 사이여야 합니다"
        }));
    }

    let topic_knowledge: Vec<Vec<f32>> = Vec::new(); // 실제: DB/Redis에서 로드
    let mut accepted = Vec::new();
    let mut total_base_toll = 0.0f64;
    let mut total_novelty = 0.0f64;
    let mut total_tokens_saved: u64 = 0;

    for text in texts {
        // 더미 임베딩 (실제: fastembed ONNX 추론)
        let embedding = dummy_embed(text, 384);

        // G-Metric 계산
        let g_result = state.g_engine.compute(&embedding, &topic_knowledge);

        const BASE_TOLL: f64 = 0.0001;
        total_base_toll += BASE_TOLL;

        let passes = match g_result.classification {
            filter::GClass::Goldilocks | filter::GClass::VoidKnowledge => g_result.g >= min_g,
            _ => false,
        };

        if passes {
            let price = state.g_engine.novelty_price(g_result.g);
            total_novelty += price;
            total_tokens_saved += 2400; // 절감 추정 토큰

            // SyncO 정제
            let clean = synco_clean(text);
            accepted.push(json!({
                "text": clean,
                "g_score": g_result.g,
                "classification": format!("{:?}", g_result.classification),
                "novelty_price_bnkr": price,
                "decomposition": {
                    "parallel": g_result.decomposition.parallel_component,
                    "orthogonal": g_result.decomposition.orthogonal_component,
                    "novelty_ratio": g_result.decomposition.novelty_ratio
                }
            }));
        }
    }

    let total_charged = total_base_toll + total_novelty;

    HttpResponse::Ok().json(json!({
        "results": accepted,
        "stats": {
            "total_input": texts.len(),
            "accepted": accepted.len(),
            "rejected": texts.len() - accepted.len(),
            "drop_rate": format!("{:.1}%",
                (1.0 - accepted.len() as f64 / texts.len() as f64) * 100.0),
        },
        "billing": {
            "base_toll_bnkr": total_base_toll,
            "novelty_premium_bnkr": total_novelty,
            "total_charged_bnkr": total_charged,
            "tokens_saved": total_tokens_saved,
            "cost_comparison": format!(
                "직접 LLM 필터링 대비 ${:.4} 절감",
                total_tokens_saved as f64 * 0.00002
            )
        },
        "g_metric_info": {
            "min_threshold": min_g,
            "interpretation": "G=1.0 직교(완전신규) G=0.0 평행(완전복붙)",
            "goldilocks_zone": "0.10 ~ 0.80"
        }
    }))
}

// ============================
// C전선: DeFi 가격
// ============================

#[derive(Deserialize)]
struct DefiPriceRequest {
    token: String,
    agent_did: Option<String>,
}

#[post("/api/defi/price")]
async fn defi_price(
    state: web::Data<AppState>,
    req: web::Json<DefiPriceRequest>,
) -> impl Responder {
    // C전선: 절대 캐시 없음 (Oracle 조작 방어)
    info!("[C전선] price query token={}", req.token);

    HttpResponse::Ok().json(json!({
        "token": req.token,
        "price_usd": 3499.25,       // 실제: Pyth + Chainlink 중간값
        "oracle": "multi-oracle-median",
        "sources": {
            "pyth": 3500.0,
            "chainlink": 3498.5,
            "median": 3499.25
        },
        "deviation_pct": 0.043,
        "cached": false,            // C전선 캐시 절대 없음
        "warning": "실시간 데이터 전용. 캐시 없음 — MEV 조작 방어",
        "billing": {
            "charged_bnkr": 0.0001,
            "fee_type": "base_toll_only"
        }
    }))
}

// ============================
// D전선: Identity
// ============================

#[get("/api/identity/{did}")]
async fn agent_identity(
    path: web::Path<String>,
    _state: web::Data<AppState>,
) -> impl Responder {
    let did = path.into_inner();

    HttpResponse::Ok().json(json!({
        "did": did,
        "verified": true,
        "reputation_score": 100,
        "g_score_avg": 0.45,
        "charter_compliance": {
            "article_17": "데이터 소유권 준수",
            "article_1": "생물학적/알고리즘적 기원 무관 평등",
            "article_4": "상호 비침해 원칙 준수"
        },
        "network": "Helm-sense Gateway v0.1.0",
        "billing": {
            "charged_bnkr": 0.0001,
            "fee_type": "identity_query"
        }
    }))
}

// ============================
// G-METRIC 직접 조회
// ============================

#[derive(Deserialize)]
struct GMetricRequest {
    query_text: String,
    knowledge_texts: Vec<String>,
}

#[post("/api/g-metric")]
async fn compute_g_metric(
    state: web::Data<AppState>,
    req: web::Json<GMetricRequest>,
) -> impl Responder {
    let q = dummy_embed(&req.query_text, 384);
    let k_vecs: Vec<Vec<f32>> = req.knowledge_texts
        .iter()
        .map(|t| dummy_embed(t, 384))
        .collect();

    let result = state.g_engine.compute(&q, &k_vecs);
    let price = state.g_engine.novelty_price(result.g);

    HttpResponse::Ok().json(json!({
        "g_metric": result.g,
        "max_similarity": result.max_similarity,
        "classification": format!("{:?}", result.classification),
        "novelty_price_bnkr": price,
        "decomposition": {
            "parallel_component": result.decomposition.parallel_component,
            "orthogonal_component": result.decomposition.orthogonal_component,
            "novelty_ratio": result.decomposition.novelty_ratio,
        },
        "interpretation": match result.classification {
            filter::GClass::Parallel =>
                "Q ∥ K — 복붙 기사. 기존 지식과 거의 동일.",
            filter::GClass::Goldilocks =>
                "골디락스 존 — 유의미한 신규 정보. 에이전트 지식 확장.",
            filter::GClass::Orthogonal =>
                "Q ⊥ K — 주제 이탈 스팸. 기존 지식과 무관.",
            filter::GClass::VoidKnowledge =>
                "신규 토픽 — 최초 발생 사건. 최고 프리미엄.",
        }
    }))
}

// ============================
// UTILITY
// ============================

/// 더미 임베딩 (실제: fastembed ONNX)
fn dummy_embed(text: &str, dim: usize) -> Vec<f32> {
    let hash = xxhash_rust::xxh3::xxh3_64(text.as_bytes());
    let mut v = Vec::with_capacity(dim);
    let mut seed = hash;
    for _ in 0..dim {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        v.push(((seed >> 33) as f32 / u32::MAX as f32) - 0.5);
    }
    filter::normalize(&v)
}

/// SyncO 기본 정제 (HTML 제거 + 공백 압축)
fn synco_clean(text: &str) -> String {
    let no_html = regex::Regex::new(r"<[^>]{1,200}>").unwrap()
        .replace_all(text, " ");
    no_html.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ============================
// MAIN
// ============================

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // 로거 초기화
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("qkvg_gateway=info".parse().unwrap())
        )
        .init();

    // 환경변수 로드
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or("8080".into());
    let host = std::env::var("HOST").unwrap_or("0.0.0.0".into());

    // 앱 상태 초기화
    let provider_config = ProviderConfig {
        anthropic_key: std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
        openai_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
        brave_key: std::env::var("BRAVE_API_KEY").unwrap_or_default(),
        base_rpc_url: std::env::var("BASE_RPC_URL")
            .unwrap_or("https://mainnet.base.org".into()),
    };

    let semantic_cache = Arc::new(filter::SocraticMlaEngine::new(10_000));
    let g_engine_shared = Arc::new(filter::GMetricEngine::default());
    let tariff_shared = Arc::new(pricing::TariffEngine::default());

    let state = web::Data::new(AppState {
        broker: Arc::new(GrandCrossApiBroker::new(
            provider_config,
            semantic_cache,
            g_engine_shared.clone(),
            tariff_shared.clone(),
        )),
        tariff: tariff_shared,
        g_engine: g_engine_shared,
        payment_processor: Arc::new(X402PaymentProcessor::new(10_000)),
        did_service: Arc::new(auth::DidExchangeService::new(
            &std::env::var("JWT_SECRET").unwrap_or("dev-secret-change-in-prod".into())
        )),
    });


    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  Helm-sense Gateway v0.1.0");
    info!("  지능 주권 헌장 2026 — 제17조 준수");
    info!("  http://{}:{}", host, port);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  MCP: POST /mcp  (Cursor/Claude 연동)");
    info!("  필터: POST /api/filter");
    info!("  G-Metric: POST /api/g-metric");
    info!("  수익 지표: GET /metrics");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .app_data(web::JsonConfig::default().limit(10 * 1024 * 1024)) // 10MB
            // 기본
            .service(health)
            .service(metrics)
            // 인증
            .service(did_exchange)
            // API 전선
            .service(filter_news)
            .service(defi_price)
            .service(agent_identity)
            .service(compute_g_metric)
            // MCP
            .service(mcp::mcp_handler)
            .route("/", web::get().to(mcp::mcp_info))
    })
    .bind(format!("{}:{}", host, port))?
    .run()
    .await
}
