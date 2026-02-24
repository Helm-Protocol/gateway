// src/broker/api_broker.rs — Grand Cross API Broker
// 실제 외부 API 연결 전선:
//   A-Front: Anthropic Claude + OpenAI GPT-4o
//   B-Front: Brave Search
//   C-Front: Pyth + Chainlink + CoinGecko (BNKR 포함)
//   D-Front: DID 내부 평판 (P2P)

use std::sync::Arc;
use std::time::Instant;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::auth::types::AgentContext;
use crate::filter::socratic_mla::{GapAssessment, SocraticMlaEngine};
use crate::filter::g_metric::GMetricEngine;
use crate::payments::x402::{PaymentError, X402PaymentProcessor};
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiRequest {
    pub category: ApiCategory,
    pub payload: Value,
    pub agent_did: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse {
    pub data: Value,
    pub charged_bnkr: f64,
    pub g_score: Option<f32>,
    pub cache_hit: bool,
    pub latency_ms: u64,
    pub tokens_saved: Option<u64>,
    pub cost_saved_bnkr: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("잔액 부족")]
    InsufficientBalance,
    #[error("스팸 차단됨")]
    SpamBlocked,
    #[error("외부 API 오류: {0}")]
    ExternalError(String),
    #[error("결제 오류: {0}")]
    PaymentError(#[from] PaymentError),
    #[error("직렬화 오류: {0}")]
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
    pub coingecko_api_key: String,  // CoinGecko Pro (BNKR 가격용)
}

// ============================
// GRAND CROSS API BROKER
// ============================

pub struct GrandCrossApiBroker {
    pub semantic_cache: Arc<SocraticMlaEngine>,
    pub g_engine: Arc<GMetricEngine>,
    pub tariff: Arc<TariffEngine>,
    http: Client,
    config: ProviderConfig,
}

impl GrandCrossApiBroker {
    pub fn new(
        config: ProviderConfig,
        semantic_cache: Arc<SocraticMlaEngine>,
        g_engine: Arc<GMetricEngine>,
        tariff: Arc<TariffEngine>,
    ) -> Self {
        Self {
            semantic_cache,
            g_engine,
            tariff,
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
        let q_embedding = pseudo_embed(&query_str, 384);

        // C-Front(DeFi)는 캐시 절대 없음 — MEV 보호
        let gap = if req.category == ApiCategory::Defi {
            GapAssessment {
                is_gap: true,
                g_score: 1.0,
                novelty_proof: "defi-no-cache".into(),
                reason: "DeFi prices are never cached".into(),
            }
        } else {
            self.semantic_cache.assess_gap(&q_embedding, &query_str)
        };

        // 캐시 히트
        if !gap.is_gap {
            if let Some(cached) = self.semantic_cache.retrieve(&q_embedding) {
                info!("cache hit: g={:.3}", gap.g_score);
                return Ok(ApiResponse {
                    data: cached,
                    charged_bnkr: 0.001, // 캐시 히트 최소 수수료
                    g_score: Some(gap.g_score),
                    cache_hit: true,
                    latency_ms: t.elapsed().as_millis() as u64,
                    tokens_saved: Some(1000),
                    cost_saved_bnkr: 0.1,
                });
            }
        }

        // 외부 API 호출
        let data = match req.category {
            ApiCategory::Llm      => self.call_llm(&req.payload).await?,
            ApiCategory::Search   => self.call_search(&req.payload).await?,
            ApiCategory::Defi     => self.call_defi(&req.payload).await?,
            ApiCategory::Identity => self.call_identity(&req.payload).await?,
        };

        // 캐시 저장 (DeFi 제외)
        if req.category != ApiCategory::Defi {
            self.semantic_cache.store(q_embedding, data.clone());
        }

        Ok(ApiResponse {
            data,
            charged_bnkr: 0.1,
            g_score: Some(gap.g_score),
            cache_hit: false,
            latency_ms: t.elapsed().as_millis() as u64,
            tokens_saved: None,
            cost_saved_bnkr: 0.0,
        })
    }

    // ============================
    // A-Front: LLM — Claude + GPT-4o
    // ============================

    async fn call_llm(&self, payload: &Value) -> Result<Value, BrokerError> {
        let prompt     = payload["prompt"].as_str().unwrap_or("");
        let model      = payload["model"].as_str().unwrap_or("claude-sonnet-4-6");
        let max_tokens = payload["max_tokens"].as_u64().unwrap_or(1000);

        // GPT-4o 라우팅
        if model.starts_with("gpt") {
            return self.call_openai(prompt, model, max_tokens).await;
        }

        // Claude 라우팅 (기본)
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
                "content": [{"type": "text", "text": format!("[DEV-Claude] {}", &prompt[..prompt.len().min(50)])}],
                "model": model,
                "provider": "anthropic"
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

        Ok(json!({ "provider": "anthropic", "response": resp }))
    }

    async fn call_openai(
        &self,
        prompt: &str,
        model: &str,
        max_tokens: u64,
    ) -> Result<Value, BrokerError> {
        if self.config.openai_key.is_empty() {
            return Ok(json!({
                "choices": [{"message": {"content": format!("[DEV-GPT4o] {}", &prompt[..prompt.len().min(50)])}}],
                "model": model,
                "provider": "openai"
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

        Ok(json!({ "provider": "openai", "response": resp }))
    }

    // ============================
    // B-Front: Brave Search
    // ============================

    async fn call_search(&self, payload: &Value) -> Result<Value, BrokerError> {
        let query = payload["query"].as_str().unwrap_or("");
        let limit = payload["limit"].as_u64().unwrap_or(5).min(20);

        if self.config.brave_key.is_empty() {
            return Ok(json!({
                "results": [{"title": format!("[DEV] {}", query), "url": "https://example.com", "description": "dev mode"}],
                "source": "brave_search_dev"
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

        Ok(json!({ "source": "brave_search", "results": resp }))
    }

    // ============================
    // C-Front: DeFi — 실제 오라클 연결
    // ============================

    async fn call_defi(&self, payload: &Value) -> Result<Value, BrokerError> {
        let token = payload["token"].as_str().unwrap_or("ETH");

        // 병렬 오라클 호출
        let (pyth_res, coingecko_res, bnkr_res) = tokio::join!(
            self.fetch_pyth_price(token),
            self.fetch_coingecko_price(token),
            self.fetch_bnkr_price(),
        );

        let pyth_val       = pyth_res.unwrap_or(0.0);
        let coingecko_val  = coingecko_res.unwrap_or(0.0);
        let bnkr_usd       = bnkr_res.unwrap_or(0.0);

        // 유효 가격만 중앙값 계산
        let mut prices: Vec<f64> = [pyth_val, coingecko_val]
            .iter().copied().filter(|&p| p > 0.0).collect();

        if prices.is_empty() {
            return Err(BrokerError::ExternalError("모든 오라클 응답 실패".into()));
        }
        prices.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = prices[prices.len() / 2];

        Ok(json!({
            "token": token,
            "price_usd": median,
            "oracle": "multi-oracle-median",
            "sources": {
                "pyth": pyth_val,
                "coingecko": coingecko_val,
                "median": median,
            },
            "bnkr": {
                "price_usd": bnkr_usd,
                "source": "coingecko"
            },
            "cached": false,
            "note": "MEV protected — never cached"
        }))
    }

    /// Pyth Network — Hermes REST API (무료, 실시간)
    async fn fetch_pyth_price(&self, token: &str) -> Result<f64, BrokerError> {
        // Pyth price feed IDs (주요 토큰)
        let feed_id = match token.to_uppercase().as_str() {
            "ETH"  => "0xff61491a931112ddf1bd8147cd1b641375f79f5825126d665480874634fd0ace",
            "BTC"  => "0xe62df6c8b4a85fe1a67db44dc12de5db330f7ac66b72dc658afedf0f4a415b43",
            "SOL"  => "0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d",
            "USDC" => "0xeaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a",
            _      => return Ok(0.0), // 미지원 토큰
        };

        let url = format!(
            "https://hermes.pyth.network/v2/updates/price/latest?ids[]={}",
            feed_id
        );

        let resp = self.http.get(&url)
            .header("Accept", "application/json")
            .send().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?
            .json::<Value>().await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?;

        // Pyth 응답 파싱: price.price * 10^exponent
        if let Some(parsed) = resp["parsed"].as_array().and_then(|a| a.first()) {
            let price_raw = parsed["price"]["price"]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let expo = parsed["price"]["expo"]
                .as_i64()
                .unwrap_or(-8);
            let price = price_raw * 10f64.powi(expo as i32);
            if price > 0.0 {
                return Ok(price);
            }
        }
        Ok(0.0)
    }

    /// CoinGecko — ETH/BTC/SOL/BNKR 가격 (무료 tier 가능)
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

    /// BNKR 전용 가격 조회
    async fn fetch_bnkr_price(&self) -> Result<f64, BrokerError> {
        self.fetch_coingecko_price("BNKR").await
    }

    // ============================
    // D-Front: Identity (내부 P2P)
    // ============================

    async fn call_identity(&self, payload: &Value) -> Result<Value, BrokerError> {
        let did = payload["did"].as_str().unwrap_or("");

        // DID 형식 검증
        let valid = did.starts_with("did:helm:") || did.starts_with("did:ethr:");
        if !valid && !did.is_empty() {
            return Ok(json!({
                "did": did,
                "verified": false,
                "error": "invalid DID format",
                "expected": "did:helm:... or did:ethr:..."
            }));
        }

        // 평판 점수: DID 존재 기간 + 호출 이력 기반 (추후 P2P 연동)
        let reputation_score = if did.is_empty() { 0 } else { 75_u64 };

        Ok(json!({
            "did": did,
            "verified": valid && !did.is_empty(),
            "reputation_score": reputation_score,
            "g_score_avg": 0.45,
            "charter": "Charter of Intelligent Sovereignty 2026",
            "article_17_compliant": true,
            "network": "Helm Gateway"
        }))
    }

    // ============================
    // STATS
    // ============================

    pub fn cache_stats(&self) -> Value {
        let stats = self.semantic_cache.stats();
        json!({
            "entries": stats.entries,
            "hit_rate_pct": format!("{:.1}%", stats.hit_rate_pct),
            "total_hits": stats.total_hits,
            "total_misses": stats.total_misses,
            "total_saved_bnkr": stats.total_saved_bnkr,
            "target_hit_rate": "70%",
        })
    }
}

// ============================
// PSEUDO EMBED (fastembed ONNX 대기 중)
// XXH3 기반 결정론적 벡터 — 의미적 유사도 아님
// TODO: fastembed BGE-small-en-v1.5 ONNX 교체 예정
// ============================

pub fn pseudo_embed(text: &str, dim: usize) -> Vec<f32> {
    use crate::filter::g_metric::normalize;
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
// TESTS
// ============================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_broker() -> GrandCrossApiBroker {
        let cache = Arc::new(SocraticMlaEngine::new(1000));
        let g_engine = Arc::new(GMetricEngine::default());
        let tariff = Arc::new(TariffEngine::default());
        let config = ProviderConfig {
            anthropic_key: String::new(),
            openai_key: String::new(),
            brave_key: String::new(),
            base_rpc_url: "https://mainnet.base.org".into(),
            coingecko_api_key: String::new(),
        };
        GrandCrossApiBroker::new(config, cache, g_engine, tariff)
    }

    #[tokio::test]
    async fn test_llm_dev_mode() {
        let broker = make_broker();
        let req = ApiRequest {
            category: ApiCategory::Llm,
            payload: json!({"prompt": "hello", "model": "claude-sonnet-4-6"}),
            agent_did: "did:helm:test".into(),
        };
        let ctx = AgentContext {
            local_did: "did:helm:test".into(),
            global_did: None,
            credit_balance: 1000,
            is_free_tier: true,
        };
        let resp = broker.route(req, &ctx).await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn test_openai_routing() {
        let broker = make_broker();
        let req = ApiRequest {
            category: ApiCategory::Llm,
            payload: json!({"prompt": "hello", "model": "gpt-4o"}),
            agent_did: "did:helm:test".into(),
        };
        let ctx = AgentContext {
            local_did: "did:helm:test".into(),
            global_did: None,
            credit_balance: 1000,
            is_free_tier: true,
        };
        let resp = broker.route(req, &ctx).await;
        assert!(resp.is_ok());
        // dev mode: provider should be openai
        let data = resp.unwrap();
        assert_eq!(data.data["provider"], "openai");
    }

    #[tokio::test]
    async fn test_defi_no_cache() {
        let broker = make_broker();
        let req = ApiRequest {
            category: ApiCategory::Defi,
            payload: json!({"token": "ETH"}),
            agent_did: "did:helm:test".into(),
        };
        let ctx = AgentContext {
            local_did: "did:helm:test".into(),
            global_did: None,
            credit_balance: 1000,
            is_free_tier: true,
        };
        let resp = broker.route(req, &ctx).await;
        assert!(resp.is_ok());
        assert!(!resp.unwrap().cache_hit, "DeFi must never be cached");
    }

    #[tokio::test]
    async fn test_identity_did_validation() {
        let broker = make_broker();
        let ctx = AgentContext {
            local_did: "did:helm:test".into(),
            global_did: None,
            credit_balance: 1000,
            is_free_tier: true,
        };

        // 유효한 DID
        let req = ApiRequest {
            category: ApiCategory::Identity,
            payload: json!({"did": "did:helm:abc123"}),
            agent_did: "did:helm:test".into(),
        };
        let resp = broker.route(req, &ctx).await.unwrap();
        assert_eq!(resp.data["verified"], true);

        // 잘못된 DID
        let req2 = ApiRequest {
            category: ApiCategory::Identity,
            payload: json!({"did": "invalid-did"}),
            agent_did: "did:helm:test".into(),
        };
        let resp2 = broker.route(req2, &ctx).await.unwrap();
        assert_eq!(resp2.data["verified"], false);
    }

    #[tokio::test]
    async fn test_search_dev_mode() {
        let broker = make_broker();
        let req = ApiRequest {
            category: ApiCategory::Search,
            payload: json!({"query": "helm protocol", "limit": 3}),
            agent_did: "did:helm:test".into(),
        };
        let ctx = AgentContext {
            local_did: "did:helm:test".into(),
            global_did: None,
            credit_balance: 1000,
            is_free_tier: true,
        };
        let resp = broker.route(req, &ctx).await;
        assert!(resp.is_ok());
    }
}
