// src/filter/socratic_mla.rs
//
// ═══════════════════════════════════════════════════════════════
// SOCRATIC MLA ENGINE  (Jeff Dean 설계 통합)
// ═══════════════════════════════════════════════════════════════
//
// "단순 키/밸류 캐시가 아닌 Helm-sense 인지 엔진"
//
// 역할:
//   ApiBroker.semantic_cache 로 주입됨
//   외부 API 호출 전 G-Metric으로 캐시 히트 판정
//   G < 0.10 → 캐시 반환 (원가 $0, 마진 100%)
//   G ≥ 0.10 → 외부 호출 필요 (Gap 존재)
//
// 아키텍처:
//   Level 1: XXHash3 완전 일치 (O(1), 나노초)
//   Level 2: G-Metric 의미 유사 (O(n), 밀리초)
//   Level 3: 저장 — 새 지식을 캐시에 압축 적재

use std::collections::HashMap;
use parking_lot::RwLock;
use xxhash_rust::xxh3::xxh3_64;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::g_metric::{cosine_similarity, normalize, GClass, GMetricEngine};

// ============================
// CACHE ENTRY
// ============================

/// 캐시 엔트리 — 지식 단위 1개
#[derive(Debug, Clone)]
struct CacheEntry {
    /// 정제된 응답 텍스트
    response: String,
    /// 임베딩 벡터 (코사인 유사도 비교용)
    embedding: Vec<f32>,
    /// 저장 시각
    stored_at: DateTime<Utc>,
    /// 히트 횟수 (LRU 정책용)
    hit_count: u64,
    /// 이 엔트리로 절감된 외부 API 비용 누적 (BNKR)
    cost_saved_bnkr: f64,
}

// ============================
// ENGINE
// ============================

/// Gap 평가 결과
#[derive(Debug, Clone, Serialize)]
pub struct GapAssessment {
    /// true = Gap 있음 → 외부 API 필요
    /// false = Gap 없음 → 캐시 반환 (CASH COW)
    pub is_gap: bool,
    /// 캐시 히트 시 응답 내용
    pub cached_response: Option<String>,
    /// G-Metric 점수
    pub g_score: f32,
    /// 판정 분류
    pub classification: String,
    /// 절감된 비용 (캐시 히트 시)
    pub cost_saved_bnkr: f64,
}

/// Socratic MLA Engine — ApiBroker의 심장
pub struct SocraticMlaEngine {
    /// Level 1: 완전 일치 캐시 (hash → entry_key)
    exact_cache: RwLock<HashMap<u64, String>>,
    /// Level 2: 의미 캐시 (key → entry)
    semantic_cache: RwLock<HashMap<String, CacheEntry>>,
    /// G-Metric 엔진
    g_engine: GMetricEngine,
    /// 최대 캐시 엔트리 수 (LRU로 관리)
    max_entries: usize,
    /// 전체 히트 횟수
    total_hits: RwLock<u64>,
    /// 전체 미스 횟수
    total_misses: RwLock<u64>,
    /// 절감 누적액 (BNKR)
    total_saved_bnkr: RwLock<f64>,
}

impl SocraticMlaEngine {
    pub fn new(max_entries: usize) -> Self {
        Self {
            exact_cache: RwLock::new(HashMap::new()),
            semantic_cache: RwLock::new(HashMap::new()),
            g_engine: GMetricEngine::default(),
            max_entries,
            total_hits: RwLock::new(0),
            total_misses: RwLock::new(0),
            total_saved_bnkr: RwLock::new(0.0),
        }
    }

    // ============================
    // assess_gap — ApiBroker 진입점
    // ============================

    /// Helm-sense 기반 Gap 평가
    ///
    /// ApiBroker.route()에서 외부 API 호출 전 반드시 호출
    /// is_gap=false → 캐시 반환, 외부 API 우회
    /// is_gap=true  → 외부 API 호출 필요
    pub fn assess_gap(
        &self,
        query: &str,
        query_embedding: &[f32],
        provider_cost_bnkr: f64,
    ) -> GapAssessment {
        // === Level 1: 완전 일치 (O(1)) ===
        let hash = xxh3_64(query.as_bytes());
        {
            let exact = self.exact_cache.read();
            if let Some(key) = exact.get(&hash) {
                let cache = self.semantic_cache.read();
                if let Some(entry) = cache.get(key) {
                    *self.total_hits.write() += 1;
                    *self.total_saved_bnkr.write() += provider_cost_bnkr;
                    tracing::info!(
                        "[SocraticMLA] L1 EXACT HIT | query_hash={:x} cost_saved={:.4} BNKR",
                        hash, provider_cost_bnkr
                    );
                    return GapAssessment {
                        is_gap: false,
                        cached_response: Some(entry.response.clone()),
                        g_score: 0.0,
                        classification: "ExactMatch".into(),
                        cost_saved_bnkr: provider_cost_bnkr,
                    };
                }
            }
        }

        // === Level 2: 의미 유사도 (G-Metric) ===
        let k_vecs: Vec<Vec<f32>> = {
            self.semantic_cache
                .read()
                .values()
                .map(|e| e.embedding.clone())
                .collect()
        };

        let g_result = self.g_engine.compute(query_embedding, &k_vecs);

        match g_result.classification {
            // G < 0.10 → 이미 아는 내용 → 캐시 반환
            GClass::Parallel => {
                // 가장 유사한 엔트리 찾기
                let best_response = self.find_nearest_response(query_embedding);
                *self.total_hits.write() += 1;
                *self.total_saved_bnkr.write() += provider_cost_bnkr;

                tracing::info!(
                    "[SocraticMLA] L2 SEMANTIC HIT | G={:.3} classification=Parallel cost_saved={:.4} BNKR",
                    g_result.g, provider_cost_bnkr
                );

                GapAssessment {
                    is_gap: false,
                    cached_response: best_response,
                    g_score: g_result.g,
                    classification: "SemanticMatch".into(),
                    cost_saved_bnkr: provider_cost_bnkr,
                }
            }

            // 골디락스 or VoidKnowledge → 새로운 정보 → 외부 API 필요
            GClass::Goldilocks | GClass::VoidKnowledge => {
                *self.total_misses.write() += 1;

                tracing::info!(
                    "[SocraticMLA] CACHE MISS | G={:.3} classification={:?} → external API required",
                    g_result.g, g_result.classification
                );

                GapAssessment {
                    is_gap: true,
                    cached_response: None,
                    g_score: g_result.g,
                    classification: format!("{:?}", g_result.classification),
                    cost_saved_bnkr: 0.0,
                }
            }

            // G > 0.80 → 주제 이탈 스팸 → 드롭 (외부 API 호출 안 함)
            GClass::Orthogonal => {
                tracing::warn!(
                    "[SocraticMLA] SPAM DETECTED | G={:.3} → request blocked",
                    g_result.g
                );
                GapAssessment {
                    is_gap: false, // 외부 API 안 부름
                    cached_response: Some("__SPAM_BLOCKED__".into()),
                    g_score: g_result.g,
                    classification: "SpamBlocked".into(),
                    cost_saved_bnkr: 0.0,
                }
            }
        }
    }

    // ============================
    // store_latent — 새 지식 캐시 저장
    // ============================

    /// 외부 API 응답을 캐시에 저장
    /// (다음 에이전트에게 팔기 위해)
    pub fn store_latent(
        &self,
        query: &str,
        response: &str,
        embedding: Vec<f32>,
    ) {
        // LRU: 최대 엔트리 초과 시 오래된 것 제거
        {
            let mut cache = self.semantic_cache.write();
            if cache.len() >= self.max_entries {
                // 가장 오래 전에 접근한 엔트리 제거
                if let Some(oldest_key) = cache
                    .iter()
                    .min_by_key(|(_, e)| e.stored_at)
                    .map(|(k, _)| k.clone())
                {
                    cache.remove(&oldest_key);
                    tracing::debug!("[SocraticMLA] LRU eviction: {}", oldest_key);
                }
            }

            let key = format!("{:x}", xxh3_64(query.as_bytes()));
            cache.insert(key.clone(), CacheEntry {
                response: response.to_string(),
                embedding: normalize(&embedding),
                stored_at: Utc::now(),
                hit_count: 0,
                cost_saved_bnkr: 0.0,
            });

            // Level 1 해시 등록
            let mut exact = self.exact_cache.write();
            exact.insert(xxh3_64(query.as_bytes()), key);
        }

        tracing::info!(
            "[SocraticMLA] STORED: query_len={} entries={}",
            query.len(),
            self.semantic_cache.read().len()
        );
    }

    // ============================
    // STATS
    // ============================

    pub fn stats(&self) -> CacheStats {
        let hits = *self.total_hits.read();
        let misses = *self.total_misses.read();
        let total = hits + misses;
        let hit_rate = if total > 0 {
            hits as f64 / total as f64 * 100.0
        } else {
            0.0
        };

        CacheStats {
            entries: self.semantic_cache.read().len(),
            total_hits: hits,
            total_misses: misses,
            hit_rate_pct: hit_rate,
            total_saved_bnkr: *self.total_saved_bnkr.read(),
        }
    }

    // ============================
    // PRIVATE HELPERS
    // ============================

    fn find_nearest_response(&self, query: &[f32]) -> Option<String> {
        let cache = self.semantic_cache.read();
        cache
            .values()
            .max_by(|a, b| {
                let sim_a = cosine_similarity(query, &a.embedding);
                let sim_b = cosine_similarity(query, &b.embedding);
                sim_a.partial_cmp(&sim_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|e| e.response.clone())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CacheStats {
    pub entries: usize,
    pub total_hits: u64,
    pub total_misses: u64,
    pub hit_rate_pct: f64,
    pub total_saved_bnkr: f64,
}

// ============================
// TESTS
// ============================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_engine() -> SocraticMlaEngine {
        SocraticMlaEngine::new(100)
    }

    fn vec3(x: f32, y: f32, z: f32) -> Vec<f32> {
        normalize(&vec![x, y, z])
    }

    #[test]
    fn test_exact_cache_hit() {
        let engine = make_engine();
        let q = "비트코인 가격";
        let emb = vec3(1.0, 0.0, 0.0);

        // 저장
        engine.store_latent(q, "BTC=$50,000", emb.clone());

        // 완전 일치 → 캐시 히트
        let result = engine.assess_gap(q, &emb, 0.05);
        assert!(!result.is_gap, "완전 일치는 Gap 없음");
        assert_eq!(result.cached_response.unwrap(), "BTC=$50,000");
        assert_eq!(result.classification, "ExactMatch");
    }

    #[test]
    fn test_semantic_miss_novel_query() {
        let engine = make_engine();

        // 지식 베이스: (1, 0, 0) 방향
        let k_emb = vec3(1.0, 0.0, 0.0);
        engine.store_latent("비트코인", "BTC data", k_emb);

        // 45도 각도 벡터 → cos=0.707 → G=0.293 → 골디락스 존 → is_gap=true
        // (완전 직교 G=1.0이면 스팸으로 분류되어 is_gap=false)
        let norm = (2.0_f32).sqrt();
        let q_emb = vec3(1.0 / norm, 1.0 / norm, 0.0); // 45도
        let result = engine.assess_gap("이더리움 덴쿤 업그레이드", &q_emb, 0.05);
        // G≈0.293 → Goldilocks → is_gap=true (외부 API 호출 필요)
        assert!(result.is_gap, "골디락스 존 새로운 정보는 Gap 있음 (G={:.3})", result.g_score);
    }

    #[test]
    fn test_stats_accumulate() {
        let engine = make_engine();
        let emb = vec3(1.0, 0.0, 0.0);
        engine.store_latent("test", "response", emb.clone());

        engine.assess_gap("test", &emb, 0.05);  // hit
        engine.assess_gap("test", &emb, 0.05);  // hit

        let stats = engine.stats();
        assert_eq!(stats.total_hits, 2);
        assert!((stats.total_saved_bnkr - 0.10).abs() < 0.001);
    }

    #[test]
    fn test_lru_eviction() {
        let engine = SocraticMlaEngine::new(3); // 최대 3개

        for i in 0..4 {
            let emb = vec3(i as f32 * 0.1, 1.0, 0.0);
            engine.store_latent(&format!("query_{}", i), "resp", emb);
        }

        // 4개 저장 시도 → LRU로 1개 제거 → 3개만 남음
        assert_eq!(engine.semantic_cache.read().len(), 3);
    }
}
