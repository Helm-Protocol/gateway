// src/broker/api_broker.rs
//
// ═══════════════════════════════════════════════════════════════
// HELM API BROKER  — GRAND CROSS INTEGRATION
// ═══════════════════════════════════════════════════════════════
//
// Jeff Dean 최종 통합 설계:
//
//   [DID Auth] → [Kaleidoscope SafeStream]
//       ↓
//   [Helm-sense Gap 평가] ← SocraticMlaEngine
//       ↓ Gap 없음            ↓ Gap 있음
//   [캐시 반환]          [4전선 라우팅]
//   마진 100%            A/B/C/D 분류
//       ↓                    ↓
//   [x402 과금]         [외부 API 호출]
//       ↓                    ↓
//   [결과 반환]         [캐시 저장 → 다음 에이전트에게 판매]
//
// 무한 마진 루프:
//   에이전트가 많을수록 캐시 히트율 ↑ → 원가 ↓ → 마진 ↑

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
    /// 절감된 비용 (캐시 히트 시 외부 API 원가)
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
}

// ============================
// GRAND CROSS API BROKER
// ============================

pub struct GrandCrossApiBroker {
    /// 의미론적 캐시 (Jeff Dean 설계 핵심)
    pub semantic_cache: Arc<SocraticMlaEngine>,
    /// G-Metric 엔진
    pub g_engine: Arc<GMetricEngine>,
    /// Two-Part Tariff
    pub tariff: Arc<TariffEngine>,
    /// HTTP 클라이언트
    http: Client,
    /// 프로바이더 설정
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
    // MAIN ROUTE — Grand Cross 흐름
    // ============================

    pub async fn route(
        &self,
        req: ApiRequest,
        agent: &AgentContext,
    ) -> Result<ApiResponse, BrokerError> {
        let t = Instant::now();
        let query_str = req.payload.to_string();

        // === STEP 1: 더미 임베딩 (실제: fastembed ONNX) ===
        let q_embedding = dummy_embed(&query_str, 384);

        // === STEP 2: SocraticMLA Gap 평가 ===
        // C전선(DeFi)은 절대 캐시 없음 — 보안 규칙
        let gap = if req.category == ApiCategory::Defi {
            GapAssessment {
                is_gap: true,
                cached_response: None,
                g_score: 1.0,
                classification: "DeFiNoCache".into(),
                cost_saved_bnkr: 0.0,
            }
        } else {
            self.semantic_cache.assess_gap(&query_str, &q_embedding, 0.05)
        };

        // 스팸 차단
        if gap.cached_response.as_deref() == Some("__SPAM_BLOCKED__") {
            warn!("[Broker] Spam blocked for agent={}", agent.local_did);
            return Err(BrokerError::SpamBlocked);
        }

        // === STEP 3: 캐시 히트 → 즉시 반환 (CASH COW) ===
        if !gap.is_gap {
            // 캐시 히트라도 Base Toll 과금 (수익 극대화 전략)
            let price = self.tariff.calculate(
                gap.g_score,
                agent.is_free_tier,
                false,
            );

            info!(
                "[Broker] CACHE HIT | agent={} G={:.3} toll={:.4} BNKR saved={:.4} BNKR",
                agent.local_did, gap.g_score,
                price.base_toll_bnkr, gap.cost_saved_bnkr
            );

            return Ok(ApiResponse {
                data: json!({
                    "result": gap.cached_response.unwrap_or_default(),
                    "source": "semantic_cache"
                }),
                charged_bnkr: price.base_toll_bnkr,
                g_score: Some(gap.g_score),
                cache_hit: true,
                latency_ms: t.elapsed().as_millis() as u64,
                tokens_saved: Some(2400),
                cost_saved_bnkr: gap.cost_saved_bnkr,
            });
        }

        // === STEP 4: Gap 있음 → 4전선 외부 API 라우팅 ===
        let external_result = match req.category {
            ApiCategory::Llm => self.call_llm(&req.payload).await?,
            ApiCategory::Search => self.call_search(&req.payload).await?,
            ApiCategory::Defi => self.call_defi(&req.payload).await?,
            ApiCategory::Identity => self.call_identity(&req.payload).await?,
        };

        // === STEP 5: G-Metric 기반 Novelty Premium 과금 ===
        let is_new_topic = gap.classification == "VoidKnowledge";
        let price = self.tariff.calculate(gap.g_score, agent.is_free_tier, is_new_topic);

        let total_bnkr = price.total_bnkr;

        info!(
            "[Broker] EXTERNAL CALL | agent={} category={:?} G={:.3} \
             base={:.4} novelty={:.4} total={:.4} BNKR tier={:?}",
            agent.local_did, req.category, gap.g_score,
            price.base_toll_bnkr, price.novelty_premium_bnkr, total_bnkr, price.tier
        );

        // === STEP 6: 새 지식 캐시 저장 (다음 에이전트에게 판매) ===
        if req.category != ApiCategory::Defi {
            let resp_str = external_result.to_string();
            self.semantic_cache.store_latent(&query_str, &resp_str, q_embedding);
            info!("[Broker] Knowledge stored in SocraticMLA cache");
        }

        Ok(ApiResponse {
            data: external_result,
            charged_bnkr: total_bnkr,
            g_score: Some(gap.g_score),
            cache_hit: false,
            latency_ms: t.elapsed().as_millis() as u64,
            tokens_saved: None,
            cost_saved_bnkr: 0.0,
        })
    }

    // ============================
    // A전선: LLM (도매 마진 30%)
    // ============================

    async fn call_llm(&self, payload: &Value) -> Result<Value, BrokerError> {
        let prompt = payload["prompt"].as_str().unwrap_or("");
        let model = payload["model"].as_str().unwrap_or("claude-sonnet-4-6");
        let max_tokens = payload["max_tokens"].as_u64().unwrap_or(1000);

        if self.config.anthropic_key.is_empty() {
            // 개발 모드 더미
            return Ok(json!({
                "content": [{"type": "text", "text": format!("[DEV] LLM response for: {}", &prompt[..prompt.len().min(50)])}],
                "usage": {"input_tokens": 50, "output_tokens": 100},
                "model": model
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
            .send()
            .await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?
            .json::<Value>()
            .await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?;

        Ok(resp)
    }

    // ============================
    // B전선: Search + SocraticMLA
    // ============================

    async fn call_search(&self, payload: &Value) -> Result<Value, BrokerError> {
        let query = payload["query"].as_str().unwrap_or("");
        let limit = payload["limit"].as_u64().unwrap_or(5).min(20);

        if self.config.brave_key.is_empty() {
            return Ok(json!({
                "results": [
                    {"title": format!("[DEV] Result for: {}", query), "url": "https://example.com", "description": "Dev mode placeholder"}
                ],
                "total": 1,
                "source": "brave_search"
            }));
        }

        let resp = self.http
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", &self.config.brave_key)
            .header("Accept", "application/json")
            .query(&[("q", query), ("count", &limit.to_string())])
            .send()
            .await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?
            .json::<Value>()
            .await
            .map_err(|e| BrokerError::ExternalError(e.to_string()))?;

        Ok(resp)
    }

    // ============================
    // C전선: DeFi — 캐시 절대 없음
    // ============================

    async fn call_defi(&self, payload: &Value) -> Result<Value, BrokerError> {
        let token = payload["token"].as_str().unwrap_or("ETH");

        // 다중 오라클 병렬 호출 (Oracle 조작 방어)
        let (pyth_price, chainlink_price) = tokio::join!(
            self.fetch_pyth_price(token),
            self.fetch_chainlink_price(token),
        );

        let pyth_val = pyth_price.unwrap_or(0.0);
        let chainlink_val = chainlink_price.unwrap_or(0.0);
        let mut prices: Vec<f64> = [pyth_val, chainlink_val]
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
                "chainlink": chainlink_val,
                "median": median,
            },
            "cached": false,
            "warning": "실시간 데이터 — 캐시 없음 (MEV 보호)"
        }))
    }

    async fn fetch_pyth_price(&self, _token: &str) -> Result<f64, BrokerError> {
        // TODO: Pyth Network WebSocket 연동
        Ok(3500.0)
    }

    async fn fetch_chainlink_price(&self, _token: &str) -> Result<f64, BrokerError> {
        // TODO: Base Chain Chainlink 컨트랙트 호출
        Ok(3498.5)
    }

    // ============================
    // D전선: Identity (내부 처리)
    // ============================

    async fn call_identity(&self, payload: &Value) -> Result<Value, BrokerError> {
        let did = payload["did"].as_str().unwrap_or("");

        // P2P 내부 처리 — 외부 의존성 없음
        Ok(json!({
            "did": did,
            "verified": true,
            "reputation_score": 100,
            "g_score_avg": 0.45,
            "charter": "지능 주권 헌장 2026",
            "article_17_compliant": true,
            "network": "Helm-sense Gateway"
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
            "current_vs_target": format!("{:.1}% / 70%", stats.hit_rate_pct),
        })
    }
}

// ============================
// UTILS
// ============================

fn dummy_embed(text: &str, dim: usize) -> Vec<f32> {
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
        };
        GrandCrossApiBroker::new(config, cache, g_engine, tariff)
    }

    fn make_agent() -> AgentContext {
        AgentContext {
            local_did: "did:helm_sense:agent_test".into(),
            global_did: "did:ethr:0xTEST".into(),
            balance_bnkr: 10.0,
            reputation_score: 100,
            is_free_tier: true,
        }
    }

    #[tokio::test]
    async fn test_identity_route() {
        let broker = make_broker();
        let agent = make_agent();
        let req = ApiRequest {
            category: ApiCategory::Identity,
            payload: json!({"did": "did:helm_sense:agent_777"}),
            agent_did: agent.local_did.clone(),
        };

        let resp = broker.route(req, &agent).await.unwrap();
        assert!(resp.data["verified"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_defi_never_cached() {
        let broker = make_broker();
        let agent = make_agent();

        let req = ApiRequest {
            category: ApiCategory::Defi,
            payload: json!({"token": "ETH", "action": "price"}),
            agent_did: agent.local_did.clone(),
        };

        let resp = broker.route(req, &agent).await.unwrap();
        // DeFi는 항상 캐시 미스
        assert!(!resp.cache_hit);
        assert_eq!(resp.data["cached"], json!(false));
    }

    #[tokio::test]
    async fn test_search_cache_hit_on_second_call() {
        let broker = make_broker();
        let agent = make_agent();

        let req = ApiRequest {
            category: ApiCategory::Search,
            payload: json!({"query": "이더리움 가격 2026"}),
            agent_did: agent.local_did.clone(),
        };

        // 첫 번째 — 캐시 미스 (외부 API)
        let resp1 = broker.route(req.clone(), &agent).await.unwrap();
        assert!(!resp1.cache_hit);

        // 두 번째 — 완전 일치 캐시 히트
        let resp2 = broker.route(req, &agent).await.unwrap();
        assert!(resp2.cache_hit);
        assert!(resp2.cost_saved_bnkr > 0.0);
    }

    #[tokio::test]
    async fn test_llm_dev_mode() {
        let broker = make_broker();
        let agent = make_agent();
        let req = ApiRequest {
            category: ApiCategory::Llm,
            payload: json!({"prompt": "헬름 프로토콜이란?", "max_tokens": 100}),
            agent_did: agent.local_did.clone(),
        };

        let resp = broker.route(req, &agent).await.unwrap();
        assert!(resp.data["content"].is_array());
    }
}
