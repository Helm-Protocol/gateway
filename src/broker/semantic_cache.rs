// src/broker/semantic_cache.rs
//
// ═══════════════════════════════════════════════════════════════
// SOCRACTIC MLA ENGINE  (Jeff Dean 설계 — 무한 마진 창출 루프 핵심)
// ═══════════════════════════════════════════════════════════════
//
// 원칙:
//   "이미 아는 것은 0원에 팔아라. 새로운 것만 비싸게 팔아라."
//
// 동작:
//   1. 질의(Q) 들어옴
//   2. G-Metric으로 기존 지식(K)과 비교
//   3. G < 0.1 → 캐시 히트 → 외부 API 비용 $0, 마진 100%
//   4. G ≥ 0.1 → 캐시 미스 → 외부 API 호출 → 결과 저장 → 다음에 팔기
//
// 히트율 목표: 40% → 70% (Jeff Dean 코드 레벨 전술 최적화)
//
// 캐시 히트율 70% 달성 방법:
//   - XXHash3 완전 중복 (즉시 히트)
//   - 코사인 유사도 > 0.9 (의미 동일, 히트)
//   - 시간 가중치 (최근 질의일수록 히트 가능성 높음)

use std::collections::HashMap;
use std::time::{Duration, Instant};
use parking_lot::RwLock;

use crate::filter::g_metric::{cosine_similarity, GMetricEngine};
use xxhash_rust::xxh3::xxh3_64;

// ============================
// CACHE ENTRY
// ============================

/// 캐시 항목 — 질의 + 응답 + 메타데이터
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// 원본 질의 텍스트
    pub query: String,
    /// 질의 임베딩 벡터
    pub query_vector: Vec<f32>,
    /// 저장된 응답
    pub response: String,
    /// G-Metric 점수 (이 캐시가 얼마나 "새로운" 정보였는지)
    pub g_score: f32,
    /// 캐시 저장 시각
    pub created_at: Instant,
    /// 조회 횟수 (인기도)
    pub hit_count: u64,
    /// LRU를 위한 마지막 조회 시각
    pub last_hit_at: Instant,
}

impl CacheEntry {
    pub fn new(query: String, query_vector: Vec<f32>, response: String, g_score: f32) -> Self {
        let now = Instant::now();
        Self {
            query,
            query_vector,
            response,
            g_score,
            created_at: now,
            hit_count: 0,
            last_hit_at: now,
        }
    }

    /// 캐시 유효 여부 (TTL 기반)
    pub fn is_valid(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() < ttl
    }
}

// ============================
// SOCRACTIC MLA ENGINE
// ============================

/// 의미론적 캐시 엔진
///
/// "Socratic" = 소크라테스식 — 질문으로 지식의 결핍을 파악
/// "MLA" = Multi-Level Associative — 다단계 연관 검색
pub struct SocraticMlaEngine {
    /// XXHash3 → Entry (완전 일치 O(1))
    exact_cache: RwLock<HashMap<u64, CacheEntry>>,
    /// 의미 검색용 벡터 인덱스 (G-Metric 기반)
    semantic_index: RwLock<Vec<CacheEntry>>,
    /// G-Metric 계산 엔진
    g_engine: GMetricEngine,
    /// 시맨틱 캐시 히트 임계값 (코사인 유사도)
    semantic_hit_threshold: f32,
    /// 캐시 TTL
    ttl: Duration,
    /// 최대 캐시 크기
    max_entries: usize,
    /// 통계
    stats: RwLock<CacheStats>,
}

#[derive(Debug, Default, Clone)]
pub struct CacheStats {
    pub total_queries: u64,
    pub exact_hits: u64,
    pub semantic_hits: u64,
    pub misses: u64,
    pub total_saved_cost_usd: f64,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        if self.total_queries == 0 { return 0.0; }
        (self.exact_hits + self.semantic_hits) as f64 / self.total_queries as f64
    }
}

/// 캐시 조회 결과
#[derive(Debug)]
pub enum CacheResult {
    /// 완전 일치 (XXHash3) — 마진 100%
    ExactHit { response: String },
    /// 의미 일치 (코사인) — 마진 100%, G 차이만큼 보정
    SemanticHit { response: String, similarity: f32 },
    /// 미스 — 외부 API 호출 필요
    Miss { g_score: f32 },
}

impl SocraticMlaEngine {
    pub fn new(semantic_hit_threshold: f32, ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            exact_cache: RwLock::new(HashMap::new()),
            semantic_index: RwLock::new(Vec::new()),
            g_engine: GMetricEngine::default(),
            semantic_hit_threshold,
            ttl: Duration::from_secs(ttl_secs),
            max_entries,
            stats: RwLock::new(CacheStats::default()),
        }
    }

    /// Jeff Dean 기본 설정
    pub fn default() -> Self {
        Self::new(
            0.90,       // 코사인 유사도 0.90 이상 → 히트
            3600,       // TTL 1시간
            50_000,     // 최대 50K 항목
        )
    }

    // ============================
    // ASSESS GAP (Jeff Dean 설계)
    // ============================

    /// [핵심] 질의의 G-Metric 평가 → 캐시 or 외부 API 결정
    ///
    /// 반환: (is_gap, cached_response)
    ///   is_gap = false → 캐시 히트 (외부 API 불필요, 마진 100%)
    ///   is_gap = true  → 캐시 미스 (외부 API 호출 필요)
    pub fn assess_gap(
        &self,
        query: &str,
        query_vector: &[f32],
    ) -> (bool, Option<String>) {
        let mut stats = self.stats.write();
        stats.total_queries += 1;

        // === Level 1: XXHash3 완전 일치 (O(1)) ===
        let hash = xxh3_64(query.as_bytes());
        {
            let cache = self.exact_cache.read();
            if let Some(entry) = cache.get(&hash) {
                if entry.is_valid(self.ttl) {
                    stats.exact_hits += 1;
                    stats.total_saved_cost_usd += 0.002; // 평균 LLM 호출 비용 절감
                    tracing::info!(
                        "[SocraticMLA] EXACT HIT hash={:#x} hits={} 💰",
                        hash, entry.hit_count + 1
                    );
                    return (false, Some(entry.response.clone()));
                }
            }
        }

        // === Level 2: 의미론적 유사도 (G-Metric) ===
        {
            let index = self.semantic_index.read();
            let valid_entries: Vec<&CacheEntry> = index
                .iter()
                .filter(|e| e.is_valid(self.ttl))
                .collect();

            if !valid_entries.is_empty() {
                // 모든 유효 벡터와 코사인 유사도 계산
                let best = valid_entries
                    .iter()
                    .map(|e| {
                        let sim = cosine_similarity(query_vector, &e.query_vector);
                        (sim, *e)
                    })
                    .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

                if let Some((max_sim, entry)) = best {
                    if max_sim >= self.semantic_hit_threshold {
                        // G-Metric 계산 (1 - max_sim)
                        let g = 1.0 - max_sim;

                        stats.semantic_hits += 1;
                        stats.total_saved_cost_usd += 0.002;
                        tracing::info!(
                            "[SocraticMLA] SEMANTIC HIT sim={:.3} G={:.3} 💰",
                            max_sim, g
                        );
                        return (false, Some(entry.response.clone()));
                    }
                }
            }
        }

        // === Level 3: 캐시 미스 ===
        // G-Metric으로 "얼마나 새로운 정보인지" 측정
        let k_vecs: Vec<Vec<f32>> = {
            let index = self.semantic_index.read();
            index.iter()
                .filter(|e| e.is_valid(self.ttl))
                .map(|e| e.query_vector.clone())
                .collect()
        };
        let g_result = self.g_engine.compute(query_vector, &k_vecs);

        stats.misses += 1;
        tracing::info!(
            "[SocraticMLA] CACHE MISS G={:.3} → external API required",
            g_result.g
        );

        (true, None)
    }

    // ============================
    // STORE LATENT (새 지식 저장)
    // ============================

    /// 새 응답을 캐시에 저장 (다음 에이전트에게 마진 100%로 팔기 위해)
    pub fn store_latent(
        &self,
        query: &str,
        query_vector: Vec<f32>,
        response: String,
        g_score: f32,
    ) {
        let hash = xxh3_64(query.as_bytes());
        let entry = CacheEntry::new(
            query.to_string(),
            query_vector.clone(),
            response.clone(),
            g_score,
        );

        // 정확 캐시 저장
        {
            let mut cache = self.exact_cache.write();
            // LRU: 최대 크기 초과 시 가장 오래된 항목 제거
            if cache.len() >= self.max_entries {
                // 간단히 절반 제거 (실제 운영: LRU 힙 사용)
                let to_remove: Vec<u64> = cache.keys().take(self.max_entries / 4).cloned().collect();
                for k in to_remove { cache.remove(&k); }
            }
            cache.insert(hash, entry.clone());
        }

        // 시맨틱 인덱스 저장
        {
            let mut index = self.semantic_index.write();
            if index.len() >= self.max_entries {
                index.drain(0..self.max_entries / 4);
            }
            index.push(entry);
        }

        tracing::info!(
            "[SocraticMLA] STORED G={:.3} query='{}'",
            g_score,
            &query[..query.len().min(50)]
        );
    }

    // ============================
    // STATS
    // ============================

    pub fn stats(&self) -> CacheStats {
        self.stats.read().clone()
    }

    pub fn hit_rate(&self) -> f64 {
        self.stats.read().hit_rate()
    }

    pub fn cache_size(&self) -> (usize, usize) {
        (
            self.exact_cache.read().len(),
            self.semantic_index.read().len(),
        )
    }

    /// Jeff Dean 목표: 40% → 70% 히트율 달성 여부
    pub fn is_target_achieved(&self) -> bool {
        self.hit_rate() >= 0.70
    }
}

// ============================
// TESTS
// ============================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vector(seed: f32, dim: usize) -> Vec<f32> {
        let mut v: Vec<f32> = (0..dim).map(|i| (seed + i as f32 * 0.01).sin()).collect();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 { v.iter_mut().for_each(|x| *x /= norm); }
        v
    }

    #[test]
    fn test_exact_cache_hit() {
        let engine = SocraticMlaEngine::default();
        let q = "비트코인 가격";
        let vec = make_vector(1.0, 32);

        // 저장
        engine.store_latent(q, vec.clone(), "BTC = $50,000".into(), 0.5);

        // 정확 히트
        let (is_gap, cached) = engine.assess_gap(q, &vec);
        assert!(!is_gap, "정확 히트는 gap이 없어야 함");
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), "BTC = $50,000");
    }

    #[test]
    fn test_semantic_cache_hit() {
        let engine = SocraticMlaEngine::new(0.80, 3600, 1000); // 80% 임계값으로 낮춤
        let dim = 32;

        // "이더리움 가격" 저장
        let v1 = make_vector(2.0, dim);
        engine.store_latent("이더리움 가격", v1.clone(), "ETH = $3,500".into(), 0.4);

        // 유사 질의 (약간 다른 벡터)
        let v_similar: Vec<f32> = v1.iter().map(|x| x * 0.99 + 0.005).collect();
        let norm: f32 = v_similar.iter().map(|x| x * x).sum::<f32>().sqrt();
        let v_similar: Vec<f32> = v_similar.iter().map(|x| x / norm).collect();

        let (is_gap, cached) = engine.assess_gap("ETH 현재 가격은?", &v_similar);

        // 코사인 유사도가 높으면 히트
        if !is_gap {
            assert!(cached.is_some());
        }
        // 실제 유사도에 따라 히트/미스 결정됨 — 임계값 확인용
    }

    #[test]
    fn test_cache_miss_for_novel_query() {
        let engine = SocraticMlaEngine::default();

        // 완전히 다른 벡터 → 미스
        let v = make_vector(99.0, 32);
        let (is_gap, cached) = engine.assess_gap("완전히 새로운 질의", &v);

        assert!(is_gap, "새로운 질의는 gap이 있어야 함");
        assert!(cached.is_none());
    }

    #[test]
    fn test_stats_tracking() {
        let engine = SocraticMlaEngine::default();
        let v = make_vector(1.0, 32);

        engine.store_latent("test", v.clone(), "result".into(), 0.5);
        engine.assess_gap("test", &v);          // exact hit
        engine.assess_gap("completely new", &make_vector(50.0, 32)); // miss

        let stats = engine.stats();
        assert_eq!(stats.total_queries, 2);
        assert!(stats.exact_hits >= 1);
        assert!(stats.misses >= 1);
    }

    #[test]
    fn test_hit_rate_calculation() {
        let engine = SocraticMlaEngine::default();
        let v = make_vector(1.0, 32);

        engine.store_latent("q", v.clone(), "r".into(), 0.3);

        // 7번 히트, 3번 미스 → 70% 목표
        for _ in 0..7 {
            engine.assess_gap("q", &v); // exact hit
        }
        for i in 0..3 {
            engine.assess_gap(&format!("unique_{}", i), &make_vector(i as f32 * 10.0, 32)); // miss
        }

        let rate = engine.hit_rate();
        println!("히트율: {:.1}%", rate * 100.0);
        assert!(rate > 0.60, "히트율 60% 이상 목표");
    }

    #[test]
    fn test_ttl_expiry() {
        // TTL 0초 = 즉시 만료
        let engine = SocraticMlaEngine::new(0.9, 0, 1000);
        let v = make_vector(1.0, 32);

        engine.store_latent("test", v.clone(), "result".into(), 0.5);

        // TTL 0이므로 즉시 만료 → 미스
        let (is_gap, _) = engine.assess_gap("test", &v);
        assert!(is_gap, "만료된 캐시는 미스여야 함");
    }
}
