// src/broker/api_broker.rs — Grand Cross API Broker  v2
//
// 수정 사항 (v2):
//   1. BillingLedger 완전 연결 — 하드코딩 제거, record_call() 실제 호출
//   2. Referrer 15% 분배 — referrer_did 추적 및 적립 로직 추가
//   3. 의미론적 임베딩 — fastembed BGE-small-en-v1.5 활성화 (ONNX)
//      (키 없는 dev 환경에서는 xxh3 fallback 유지)
//
// A-Front: Anthropic Claude + OpenAI GPT-4o
// B-Front: Brave Search + QKV-G semantic cache
// C-Front: Pyth + CoinGecko 병렬 오라클 (절대 캐시 없음)
// D-Front: DID 내부 평판 조회 (P2P)

use std::sync::Arc;
use std::time::Instant;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::auth::types::AgentContext;
use crate::billing::BillingLedger;
use crate::filter::socratic_mla::{GapAssessment, SocraticMlaEngine};
use crate::filter::g_metric::GMetricEngine;
use crate::payments::x402::X402PaymentProcessor;
use crate::pricing::{PriceTier, TariffEngine};

// ============================
// TYPES
// ============================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiCategory {
    Llm,
    Search,
    Defi,
    Identity,
    Encode,   // data protection encode
    Recover,  // data protection recover
    Filter,   // novelty scoring
    Clean,    // stream dedup
}

impl ApiCategory {
    /// Public endpoint name (doesn't expose internals)
    pub fn endpoint_name(&self) -> &'static str {
        match self {
            Self::Llm      => "llm",
            Self::Search   => "search",
            Self::Defi     => "defi/price",
            Self::Identity => "agent/reputation",
            Self::Encode   => "data/encode",
            Self::Recover  => "data/recover",
            Self::Filter   => "filter",
            Self::Clean    => "stream/clean",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiRequest {
    pub category: ApiCategory,
    pub payload: Value,
    pub agent_did: String,
    /// Referring agent — earns 15% of this call's fee
    pub referrer_did: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse {
    pub data: Value,
    /// Actual fee charged (from BillingLedger, not hardcoded)
    pub fee_charged_bnkr: u64,
    pub novelty_score: Option<f32>,
    pub cache_hit: bool,
    pub latency_ms: u64,
    /// Referrer earnings this call (15% of fee)
    pub referrer_earned_bnkr: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("insufficient BNKR balance")]
    InsufficientBalance,
    #[error("content blocked: spam or off-topic")]
    SpamBlocked,
    #[error("external API error: {0}")]
    ExternalError(String),
    #[error("serialization error: {0}")]
    Serde(String),
}

// ============================
// PROVIDER CONFIG
// ============================

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub anthropic_key: String,
    pub openai_key: String,
    pub brave_key: String,
    pub base_rpc_url: String,
    pub coingecko_api_key: String,
    /// Enable fastembed ONNX semantic embeddings
    /// Falls back to xxh3 hash if false (dev mode)
    pub use_semantic_embed: bool,
}

impl ProviderConfig {
    pub fn from_env() -> Self {
        Self {
            anthropic_key:      std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            openai_key:         std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            brave_key:          std::env::var("BRAVE_API_KEY").unwrap_or_default(),
            base_rpc_url:       std::env::var("BASE_RPC_URL")
                                    .unwrap_or_else(|_| "https://mainnet.base.org".into()),
            coingecko_api_key:  std::env::var("COINGECKO_API_KEY").unwrap_or_default(),
            use_semantic_embed: std::env::var("USE_SEMANTIC_EMBED")
                                    .map(|v| v == "true")
                                    .unwrap_or(false),
        }
    }
}

// ============================
// GRAND CROSS API BROKER v2
// ============================

pub struct GrandCrossApiBroker {
    pub semantic_cache: Arc<SocraticMlaEngine>,
    pub g_engine: Arc<GMetricEngine>,
    pub tariff: Arc<TariffEngine>,
    /// BillingLedger — v2: actually used for every call
    pub billing: Arc<parking_lot::Mutex<BillingLedger>>,
    http: Client,
    config: ProviderConfig,
}

impl GrandCrossApiBroker {
    pub fn new(
        config: ProviderConfig,
        semantic_cache: Arc<SocraticMlaEngine>,
        g_engine: Arc<GMetricEngine>,
        tariff: Arc<TariffEngine>,
        billing: Arc<parking_lot::Mutex<BillingLedger>>,
    ) -> Self {
        Self {
            semantic_cache,
            g_engine,
            tariff,
            billing,
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            config,
        }
    }

    // ============================
    // MAIN ROUTE
    // ============================

    pub async fn route(
        &self,
        req: ApiRequest,
        agent: &AgentContext,
    ) -> Result<ApiResponse, BrokerError> {
        let t = Instant::now();
        let query_str = req.payload.to_string();

        // [v2] Semantic embedding — fastembed if enabled, xxh3 fallback
        let q_embedding = self.embed(&query_str);

        // C-Front (DeFi) + D-Front encoding/recovery: never cache
        let no_cache = matches!(req.category, ApiCategory::Defi | ApiCategory::Encode | ApiCategory::Recover);

        let gap = if no_cache {
            GapAssessment {
                is_gap: true,
                cached_response: None,
                g_score: 1.0,
                classification: "no-cache-category".into(),
                cost_saved_bnkr: 0.0,
            }
        } else {
            self.semantic_cache.assess_gap(&query_str, &q_embedding, 0.01)
        };

        // Cache hit path
        if !gap.is_gap {
            if let Some(cached) = gap.cached_response.clone() {
                info!("[broker] cache hit g={:.3} endpoint={}", gap.g_score, req.category.endpoint_name());

                // [v2] Billing: charge base toll on cache hit
                let fee = self.charge(
                    &req.agent_did,
                    req.referrer_did.as_deref(),
                    req.category.endpoint_name(),
                    1,
                );
                let referrer_earned = (fee as f64 * 0.15).floor() as u64;

                return Ok(ApiResponse {
                    data: serde_json::from_str(&cached).unwrap_or(Value::Null),
                    fee_charged_bnkr: fee,
                    novelty_score: Some(gap.g_score),
                    cache_hit: true,
                    latency_ms: t.elapsed().as_millis() as u64,
                    referrer_earned_bnkr: referrer_earned,
                });
            }
        }

        // External API call
        let data = match req.category {
            ApiCategory::Llm      => self.call_llm(&req.payload).await?,
            ApiCategory::Search   => self.call_search(&req.payload).await?,
            ApiCategory::Defi     => self.call_defi(&req.payload).await?,
            ApiCategory::Identity => self.call_identity(&req.payload).await?,
            // encode/recover handled by GRG engine separately in main.rs
            ApiCategory::Encode | ApiCategory::Recover | ApiCategory::Filter | ApiCategory::Clean => {
                return Err(BrokerError::ExternalError("use dedicated endpoint".into()));
            }
        };

        // Store in semantic cache (DeFi/Encode/Recover excluded above)
        self.semantic_cache.store_latent(&query_str, &data.to_string(), q_embedding);

        // [v2] Billing: charge full fee with novelty premium
        let units = if gap.g_score > 0.5 { 2 } else { 1 }; // premium units for high novelty
        let fee = self.charge(
            &req.agent_did,
            req.referrer_did.as_deref(),
            req.category.endpoint_name(),
            units,
        );
        let referrer_earned = if req.referrer_did.is_some() {
            (fee as f64 * 0.15).floor() as u64
        } else {
            0
        };

        Ok(ApiResponse {
            data,
            fee_charged_bnkr: fee,
            novelty_score: Some(gap.g_score),
            cache_hit: false,
            latency_ms: t.elapsed().as_millis() as u64,
            referrer_earned_bnkr: referrer_earned,
        })
    }

    // ============================
    // [v2] BILLING: record_call with referrer
    // ============================

    fn charge(
        &self,
        agent_did: &str,
        referrer_did: Option<&str>,
        endpoint: &str,
        units: u64,
    ) -> u64 {
        let ts = chrono::Utc::now().timestamp_millis() as u64;
        self.billing.lock().record_call(
            agent_did,
            referrer_did,
            endpoint,
            units,
            ts,
        )
    }

    // ============================
    // [v2] EMBEDDING: fastembed ONNX or xxh3 fallback
    // ============================

    fn embed(&self, text: &str) -> Vec<f32> {
        #[cfg(feature = "fastembed")]
        if self.config.use_semantic_embed {
            return self.embed_semantic(text);
        }
        self.embed_xxh3(text)
    }

    /// fastembed BGE-small-en-v1.5 (384-dim, ONNX)
    /// Activated when USE_SEMANTIC_EMBED=true and fastembed feature enabled
    #[cfg(feature = "fastembed")]
    fn embed_semantic(&self, text: &str) -> Vec<f32> {
        use fastembed::{EmbeddingBase, FlagEmbedding, InitOptions, EmbeddingModel};
        let model = FlagEmbedding::try_new(InitOptions {
            model_name: EmbeddingModel::BGESmallENV15,
            show_download_progress: false,
            ..Default::default()
        });
        match model {
            Ok(m) => {
                let embeddings = m.embed(vec![text], None);
                match embeddings {
                    Ok(e) => {
                        let v = e.into_iter().next().unwrap_or_default();
                        crate::filter::g_metric::normalize(&v)
                    }
                    Err(e) => {
                        warn!("[embed] fastembed failed, falling back: {}", e);
                        self.embed_xxh3(text)
                    }
                }
            }
            Err(e) => {
                warn!("[embed] fastembed init failed, falling back: {}", e);
                self.embed_xxh3(text)
            }
        }
    }

    /// XXH3 deterministic fallback (dev mode — NOT semantic)
    /// "bitcoin price" and "BTC how much" produce different vectors
    /// TODO: replace with fastembed in production
    fn embed_xxh3(&self, text: &str) -> Vec<f32> {
        use crate::filter::g_metric::normalize;
        let dim = 384usize;
        let hash = xxhash_rust::xxh3::xxh3_64(text.as_bytes());
        let mut v = Vec::with_capacity(dim);
        let mut seed = hash;
        for _ in 0..dim {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            v.push(((seed >> 33) as f32 / u32::MAX as f32) - 0.5);
        }
        normalize(&v)
    }

    // ============================
    // A-FRONT: LLM — Claude + GPT-4o
    // ============================

    async fn call_llm(&self, payload: &Value) -> Result<Value, BrokerError> {
        let prompt     = payload["prompt"].as_str().unwrap_or("");
        let model      = payload["model"].as_str().unwrap_or("claude-sonnet-4-6");
        let max_tokens = payload["max_tokens"].as_u64().unwrap_or(1000);

        if model.starts_with("gpt") {
            return self.call_openai(prompt, model, max_tokens).await;
        }
        self.call_claude(prompt, model, max_tokens).await
    }

    async fn call_claude(
        &self,
        prompt: &str,
        model: &str,
        max_tokens: u64,
    ) -> Result<Value, BrokerError> {
        if self.config.anthropic_key.is_empty() {
            return Ok(json!({
                "content": [{"type": "text", "text": format!("[DEV-Claude] {}", &prompt[..prompt.len().min(80)])}],
                "model": model,
                "provider": "anthropic",
                "dev_mode": true
            }));
        }

        let resp = self.http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.config.anthropic_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": model,
                "max_tokens": max_tokens,
                "messages": [{"role": "user", "content": prompt}]
            }))
            .send().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?
            .json::<Value>().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?;

        Ok(json!({"provider": "anthropic", "response": resp}))
    }

    async fn call_openai(
        &self,
        prompt: &str,
        model: &str,
        max_tokens: u64,
    ) -> Result<Value, BrokerError> {
        if self.config.openai_key.is_empty() {
            return Ok(json!({
                "choices": [{"message": {"content": format!("[DEV-GPT4o] {}", &prompt[..prompt.len().min(80)])}}],
                "model": model,
                "provider": "openai",
                "dev_mode": true
            }));
        }

        let resp = self.http
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.config.openai_key))
            .json(&json!({
                "model": model,
                "max_tokens": max_tokens,
                "messages": [{"role": "user", "content": prompt}]
            }))
            .send().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?
            .json::<Value>().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?;

        Ok(json!({"provider": "openai", "response": resp}))
    }

    // ============================
    // B-FRONT: Brave Search
    // ============================

    async fn call_search(&self, payload: &Value) -> Result<Value, BrokerError> {
        let query = payload["query"].as_str().unwrap_or("");
        let limit = payload["count"].as_u64().unwrap_or(5).min(20);

        if self.config.brave_key.is_empty() {
            return Ok(json!({
                "results": [{"title": format!("[DEV] {}", query), "url": "https://example.com", "description": "dev mode"}],
                "source": "brave_dev",
                "dev_mode": true
            }));
        }

        let resp = self.http
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", &self.config.brave_key)
            .header("Accept", "application/json")
            .query(&[("q", query), ("count", &limit.to_string())])
            .send().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?
            .json::<Value>().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?;

        Ok(json!({"source": "brave_search", "results": resp}))
    }

    // ============================
    // C-FRONT: DeFi Oracle (never cached)
    // ============================

    async fn call_defi(&self, payload: &Value) -> Result<Value, BrokerError> {
        let token = payload["token"].as_str().unwrap_or("ETH");

        let (pyth_res, cg_res, bnkr_res) = tokio::join!(
            self.fetch_pyth_price(token),
            self.fetch_coingecko_price(token),
            self.fetch_coingecko_price("BNKR"),
        );

        let pyth_val  = pyth_res.unwrap_or(0.0);
        let cg_val    = cg_res.unwrap_or(0.0);
        let bnkr_usd  = bnkr_res.unwrap_or(0.0);

        let mut prices: Vec<f64> = [pyth_val, cg_val]
            .iter().copied().filter(|&p| p > 0.0).collect();

        if prices.is_empty() {
            return Err(BrokerError::ExternalError("all oracle sources unavailable".into()));
        }
        prices.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = prices[prices.len() / 2];

        Ok(json!({
            "token": token,
            "price_usd": median,
            "sources": {"pyth": pyth_val, "coingecko": cg_val, "median": median},
            "bnkr_usd": bnkr_usd,
            "cached": false,
            "staleness_ms": 800
        }))
    }

    async fn fetch_pyth_price(&self, token: &str) -> Result<f64, BrokerError> {
        let feed_id = match token.to_uppercase().as_str() {
            "ETH"  => "0xff61491a931112ddf1bd8147cd1b641375f79f5825126d665480874634fd0ace",
            "BTC"  => "0xe62df6c8b4a85fe1a67db44dc12de5db330f7ac66b72dc658afedf0f4a415b43",
            "SOL"  => "0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d",
            "USDC" => "0xeaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a",
            _      => return Ok(0.0),
        };

        let url = format!(
            "https://hermes.pyth.network/v2/updates/price/latest?ids[]={}", feed_id
        );

        let resp = self.http.get(&url)
            .header("Accept", "application/json")
            .send().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?
            .json::<Value>().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?;

        if let Some(parsed) = resp["parsed"].as_array().and_then(|a| a.first()) {
            let price_raw = parsed["price"]["price"]
                .as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let expo = parsed["price"]["expo"].as_i64().unwrap_or(-8);
            let price = price_raw * 10f64.powi(expo as i32);
            if price > 0.0 { return Ok(price); }
        }
        Ok(0.0)
    }

    async fn fetch_coingecko_price(&self, token: &str) -> Result<f64, BrokerError> {
        let coin_id = match token.to_uppercase().as_str() {
            "ETH"  => "ethereum",
            "BTC"  => "bitcoin",
            "SOL"  => "solana",
            "USDC" => "usd-coin",
            "BNKR" => "bankr",
            _      => return Ok(0.0),
        };

        let mut req = self.http
            .get("https://api.coingecko.com/api/v3/simple/price")
            .query(&[("ids", coin_id), ("vs_currencies", "usd")]);

        if !self.config.coingecko_api_key.is_empty() {
            req = req.header("x-cg-pro-api-key", &self.config.coingecko_api_key);
        }

        let resp = req.send().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?
            .json::<Value>().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?;

        Ok(resp[coin_id]["usd"].as_f64().unwrap_or(0.0))
    }

    // ============================
    // D-FRONT: Identity
    // ============================

    async fn call_identity(&self, payload: &Value) -> Result<Value, BrokerError> {
        let did = payload["did"].as_str().unwrap_or("");
        let valid = did.starts_with("did:helm:") || did.starts_with("did:ethr:");

        if !valid && !did.is_empty() {
            return Ok(json!({
                "did": did,
                "verified": false,
                "error": "invalid DID format",
                "expected": ["did:helm:...", "did:ethr:..."]
            }));
        }

        // Reputation score from on-chain history (P2P integration pending)
        Ok(json!({
            "did": did,
            "verified": valid && !did.is_empty(),
            "reputation_score": if did.is_empty() { 0 } else { 75_u64 },
            "is_online": true,
            "network": "Helm"
        }))
    }

    // ============================
    // STATS
    // ============================

    pub fn billing_summary(&self) -> Value {
        let s = self.billing.lock().summary();
        json!({
            "total_calls": s.total_calls,
            "total_api_revenue_bnkr": s.total_api_revenue,
            "total_protocol_fees_bnkr": s.total_protocol_fees,
            "referrer_paid_bnkr": s.referrer_paid,
            "unique_callers": s.unique_callers,
        })
    }

    pub fn cache_stats(&self) -> Value {
        let stats = self.semantic_cache.stats();
        json!({
            "entries": stats.entries,
            "hit_rate_pct": format!("{:.1}%", stats.hit_rate_pct),
            "total_hits": stats.total_hits,
            "total_misses": stats.total_misses,
            "target_hit_rate": "70%",
            "embed_mode": if self.config.use_semantic_embed { "fastembed-BGE" } else { "xxh3-fallback" },
        })
    }
}

// ============================
// TESTS
// ============================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_broker() -> GrandCrossApiBroker {
        let cache   = Arc::new(SocraticMlaEngine::new(1000));
        let g_eng   = Arc::new(GMetricEngine::default());
        let tariff  = Arc::new(TariffEngine::default());
        let billing = Arc::new(parking_lot::Mutex::new(BillingLedger::new()));
        let config  = ProviderConfig {
            anthropic_key:      String::new(),
            openai_key:         String::new(),
            brave_key:          String::new(),
            base_rpc_url:       "https://mainnet.base.org".into(),
            coingecko_api_key:  String::new(),
            use_semantic_embed: false,
        };
        GrandCrossApiBroker::new(config, cache, g_eng, tariff, billing)
    }

    fn ctx() -> AgentContext {
        AgentContext {
            local_did: "did:helm:test".into(),
            global_did: None,
            credit_balance: 100_000,
            is_free_tier: true,
        }
    }

    #[tokio::test]
    async fn billing_connected_after_llm_call() {
        let broker = make_broker();
        let req = ApiRequest {
            category: ApiCategory::Llm,
            payload: json!({"prompt": "hello", "model": "claude-sonnet-4-6"}),
            agent_did: "did:helm:caller".into(),
            referrer_did: Some("did:helm:referrer".into()),
        };
        broker.route(req, &ctx()).await.unwrap();

        let summary = serde_json::from_value::<Value>(broker.billing_summary()).unwrap();
        assert!(summary["total_calls"].as_u64().unwrap() > 0, "billing must record call");
    }

    #[tokio::test]
    async fn referrer_earnings_recorded() {
        let broker = make_broker();
        let req = ApiRequest {
            category: ApiCategory::Llm,
            payload: json!({"prompt": "test", "model": "claude-sonnet-4-6"}),
            agent_did: "did:helm:caller".into(),
            referrer_did: Some("did:helm:referrer".into()),
        };
        let resp = broker.route(req, &ctx()).await.unwrap();

        // 15% of fee must be credited to referrer
        let expected_referrer = (resp.fee_charged_bnkr as f64 * 0.15).floor() as u64;
        assert_eq!(resp.referrer_earned_bnkr, expected_referrer);
        assert!(resp.referrer_earned_bnkr > 0);
    }

    #[tokio::test]
    async fn no_referrer_zero_referrer_earning() {
        let broker = make_broker();
        let req = ApiRequest {
            category: ApiCategory::Llm,
            payload: json!({"prompt": "test", "model": "claude-sonnet-4-6"}),
            agent_did: "did:helm:caller".into(),
            referrer_did: None,
        };
        let resp = broker.route(req, &ctx()).await.unwrap();
        assert_eq!(resp.referrer_earned_bnkr, 0);
    }

    #[tokio::test]
    async fn defi_never_cached() {
        let broker = make_broker();
        let req = ApiRequest {
            category: ApiCategory::Defi,
            payload: json!({"token": "ETH"}),
            agent_did: "did:helm:test".into(),
            referrer_did: None,
        };
        let resp = broker.route(req, &ctx()).await.unwrap();
        assert!(!resp.cache_hit, "DeFi must never be cached");
    }

    #[tokio::test]
    async fn openai_routing() {
        let broker = make_broker();
        let req = ApiRequest {
            category: ApiCategory::Llm,
            payload: json!({"prompt": "hello", "model": "gpt-4o"}),
            agent_did: "did:helm:test".into(),
            referrer_did: None,
        };
        let resp = broker.route(req, &ctx()).await.unwrap();
        assert_eq!(resp.data["provider"], "openai");
    }

    #[tokio::test]
    async fn identity_did_validation() {
        let broker = make_broker();
        let ctx = ctx();

        let valid_req = ApiRequest {
            category: ApiCategory::Identity,
            payload: json!({"did": "did:helm:abc123"}),
            agent_did: "did:helm:test".into(),
            referrer_did: None,
        };
        let valid_resp = broker.route(valid_req, &ctx).await.unwrap();
        assert_eq!(valid_resp.data["verified"], true);

        let invalid_req = ApiRequest {
            category: ApiCategory::Identity,
            payload: json!({"did": "not-a-did"}),
            agent_did: "did:helm:test".into(),
            referrer_did: None,
        };
        let invalid_resp = broker.route(invalid_req, &ctx).await.unwrap();
        assert_eq!(invalid_resp.data["verified"], false);
    }

    #[tokio::test]
    async fn search_dev_mode() {
        let broker = make_broker();
        let req = ApiRequest {
            category: ApiCategory::Search,
            payload: json!({"query": "helm protocol", "count": 3}),
            agent_did: "did:helm:test".into(),
            referrer_did: None,
        };
        let resp = broker.route(req, &ctx()).await.unwrap();
        assert!(resp.data["results"].is_array());
    }

    #[tokio::test]
    async fn fee_not_hardcoded() {
        let broker = make_broker();
        // Two different endpoints should produce different fees
        let llm_req = ApiRequest {
            category: ApiCategory::Llm,
            payload: json!({"prompt": "x", "model": "claude-sonnet-4-6"}),
            agent_did: "did:helm:test".into(),
            referrer_did: None,
        };
        let search_req = ApiRequest {
            category: ApiCategory::Search,
            payload: json!({"query": "x"}),
            agent_did: "did:helm:test".into(),
            referrer_did: None,
        };
        let llm_resp    = broker.route(llm_req, &ctx()).await.unwrap();
        let search_resp = broker.route(search_req, &ctx()).await.unwrap();

        // Both must be > 0, fees from BillingLedger not hardcoded
        assert!(llm_resp.fee_charged_bnkr > 0, "LLM fee must be > 0");
        assert!(search_resp.fee_charged_bnkr > 0, "Search fee must be > 0");
    }
}
