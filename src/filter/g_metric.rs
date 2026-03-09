// src/filter/g_metric.rs
//
// ═══════════════════════════════════════════════════════════════
// THE G-METRIC ENGINE  (Jeff Dean 설계 수학 구현)
// ═══════════════════════════════════════════════════════════════
//
// 핵심 통찰 (Jeff Dean):
//   SFE Rev17 = 물리 계층(PHY) 노이즈 제거
//   G-Metric  = 애플리케이션 계층 '지식 노이즈' 제거
//   → 완벽한 End-to-End 효율화 아키텍처
//
// 수학 기반: 직교성(Orthogonality)을 이용한 결핍(Gap) 정의
//
//   Q = 질문 / 입력 데이터 벡터 (방금 크롤링한 뉴스)
//   K = 기존 지식 공간 벡터 집합 (이미 알고 있는 것)
//
//   max_sim = max{ cos(Q, Kᵢ) : Kᵢ ∈ K }
//   G       = 1.0 − max_sim
//
//   해석:
//     G → 0.0  : Q ∥ K  (평행, 이미 아는 내용 → 복붙 기사)
//     G → 1.0  : Q ⊥ K  (직교, 완전히 새로운 정보)
//     G ∈ (0.1, 0.8) : 골디락스 존 → 유의미한 신규 정보
//
// 물리적 직관:
//   신호처리에서 직교 기저(Orthogonal Basis)는 서로 독립적인 정보.
//   G가 높다 = Q가 K의 어떤 기저와도 선형 독립
//           = K로는 설명할 수 없는 새로운 정보 성분 보유
//           = 에이전트의 지식 베이스를 실제로 확장시킴

use serde::{Deserialize, Serialize};

// ============================
// CORE G-METRIC COMPUTATION
// ============================

/// G-Metric 계산 결과 (전체 수학 정보 포함)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GMetricResult {
    /// G = 1 - max_cosine_similarity(Q, K)
    pub g: f32,
    /// K 공간과의 최대 유사도
    pub max_similarity: f32,
    /// 유사한 K 벡터의 인덱스
    pub nearest_k_idx: Option<usize>,
    /// 판정
    pub classification: GClass,
    /// G 성분 분해 (설명 가능성)
    pub decomposition: GDecomposition,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GClass {
    /// G < 0.10: Q ∥ K (복붙, 반복)
    Parallel,
    /// G ∈ [0.10, 0.80]: 골디락스 — 유의미한 신규 정보
    Goldilocks,
    /// G > 0.80: Q ⊥ K 초과 — 주제 이탈 스팸
    Orthogonal,
    /// K 공간 비어있음 — 신규 토픽 최초 발생
    VoidKnowledge,
}

/// G-Metric 성분 분해
/// 어떤 방향에서 새로운 정보가 들어왔는지 분석
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GDecomposition {
    /// K 공간에서 Q의 투영 성분 크기 (기존 지식과 겹치는 부분)
    pub parallel_component: f32,
    /// K 공간 직교 성분 크기 (순수 신규 정보)
    pub orthogonal_component: f32,
    /// 신규 정보 비율 (= G)
    pub novelty_ratio: f32,
}

/// 핵심 G-Metric 계산기
pub struct GMetricEngine {
    pub threshold_parallel: f32,    // G < 이 값 → Parallel (복붙)
    pub threshold_orthogonal: f32,  // G > 이 값 → Orthogonal (스팸)
}

impl Default for GMetricEngine {
    fn default() -> Self {
        Self {
            threshold_parallel: 0.10,
            threshold_orthogonal: 0.80,
        }
    }
}

impl GMetricEngine {
    pub fn new(threshold_parallel: f32, threshold_orthogonal: f32) -> Self {
        assert!(threshold_parallel < threshold_orthogonal, "임계값 순서 오류");
        Self { threshold_parallel, threshold_orthogonal }
    }

    /// G-Metric 계산 메인 함수
    ///
    /// Q: 입력 벡터 (크롤링된 뉴스 임베딩)
    /// K: 기존 지식 벡터 집합
    pub fn compute(&self, q: &[f32], k_space: &[Vec<f32>]) -> GMetricResult {
        // K 공간이 비어있으면 → VoidKnowledge
        if k_space.is_empty() {
            return GMetricResult {
                g: 1.0,
                max_similarity: 0.0,
                nearest_k_idx: None,
                classification: GClass::VoidKnowledge,
                decomposition: GDecomposition {
                    parallel_component: 0.0,
                    orthogonal_component: 1.0,
                    novelty_ratio: 1.0,
                },
            };
        }

        // max_sim = max{ cos(Q, Kᵢ) }
        let (max_sim, nearest_idx) = k_space
            .iter()
            .enumerate()
            .map(|(i, k)| (cosine_similarity(q, k), i))
            .fold((f32::NEG_INFINITY, 0), |(best_sim, best_i), (sim, i)| {
                if sim > best_sim { (sim, i) } else { (best_sim, best_i) }
            });

        let max_sim = max_sim.clamp(0.0, 1.0);
        let g = 1.0 - max_sim;

        // G-Metric 성분 분해
        // cos²(θ) = 투영 성분² / |Q|² (직교 분해)
        let parallel_component = max_sim * max_sim;         // K 방향 성분²
        let orthogonal_component = 1.0 - parallel_component; // 직교 성분²
        let decomposition = GDecomposition {
            parallel_component: parallel_component.sqrt(),
            orthogonal_component: orthogonal_component.sqrt(),
            novelty_ratio: g,
        };

        let classification = self.classify(g);

        GMetricResult {
            g,
            max_similarity: max_sim,
            nearest_k_idx: Some(nearest_idx),
            classification,
            decomposition,
        }
    }

    /// G값 → 분류
    fn classify(&self, g: f32) -> GClass {
        if g < self.threshold_parallel {
            GClass::Parallel
        } else if g > self.threshold_orthogonal {
            GClass::Orthogonal
        } else {
            GClass::Goldilocks
        }
    }

    /// Novelty Premium 가격 계산 (G 기반)
    ///
    /// price(G) = base + (G - G_min) × coeff
    /// G_min=0.10, G_max=0.80, base=0.01 BNKR, coeff=0.10
    pub fn novelty_price(&self, g: f32) -> f64 {
        match self.classify(g) {
            GClass::VoidKnowledge => 0.05,     // 신규 토픽 고정 프리미엄
            GClass::Goldilocks => {
                let price = 0.01 + (g - self.threshold_parallel) as f64 * 0.10;
                (price * 100_000.0).round() / 100_000.0
            }
            _ => 0.0, // Parallel, Orthogonal → Base Toll만
        }
    }

    /// 배치 G-Metric 계산 (여러 Q에 대해 동시)
    pub fn compute_batch(
        &self,
        queries: &[Vec<f32>],
        k_space: &[Vec<f32>],
    ) -> Vec<GMetricResult> {
        queries.iter().map(|q| self.compute(q, k_space)).collect()
    }
}

// ============================
// VECTOR MATH
// ============================

/// 코사인 유사도: cos(θ) = (Q·K) / (|Q||K|)
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len(), "벡터 차원 불일치");

    if a.is_empty() { return 0.0; }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a < f32::EPSILON || norm_b < f32::EPSILON {
        return 0.0;
    }

    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

/// 벡터 L2 정규화
pub fn normalize(v: &[f32]) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < f32::EPSILON {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

/// K 공간에서 Q의 직교 성분 추출
/// (Q에서 K 방향 투영을 제거한 순수 신규 성분)
///
/// 수식: Q_⊥ = Q - (Q·K̂)K̂  (K̂ = K의 단위벡터)
pub fn orthogonal_component(q: &[f32], k: &[f32]) -> Vec<f32> {
    let k_norm = normalize(k);
    let projection_scalar: f32 = q.iter().zip(k_norm.iter()).map(|(a, b)| a * b).sum();

    q.iter()
        .zip(k_norm.iter())
        .map(|(q_i, k_i)| q_i - projection_scalar * k_i)
        .collect()
}

// ============================
// SFE ANALOGY METRICS
// ============================

/// SFE(Signal Forwarding Engine) 유사 지표
/// 물리 계층 SNR ↔ 지식 계층 G-Metric 대응
#[derive(Debug, Serialize)]
pub struct SfeAnalogMetrics {
    /// 신호 대 잡음비 유사 지표 (G / (1-G))
    /// 물리: SNR = P_signal / P_noise
    /// 지식: G / (1-G) = 신규정보 / 기존정보 비율
    pub knowledge_snr: f32,

    /// 대역폭 효율 (1 - drop_rate)
    /// 물리: 실제 데이터 비율 / 전체 전송량
    /// 지식: 통과한 정보 / 전체 크롤링량
    pub bandwidth_efficiency: f32,

    /// 정보 순도 (통과 데이터 중 실제 신규 비율)
    pub information_purity: f32,
}

impl SfeAnalogMetrics {
    pub fn calculate(
        total_docs: u64,
        accepted_docs: u64,
        avg_g: f32,
    ) -> Self {
        let drop_rate = 1.0 - (accepted_docs as f32 / total_docs.max(1) as f32);

        // Knowledge SNR: G/(1-G) (G=0.5 → SNR=1, G=0.8 → SNR=4)
        let snr = if avg_g >= 1.0 {
            f32::INFINITY
        } else {
            avg_g / (1.0 - avg_g)
        };

        Self {
            knowledge_snr: snr,
            bandwidth_efficiency: 1.0 - drop_rate,
            information_purity: avg_g,
        }
    }
}

// ============================
// TESTS
// ============================

#[cfg(test)]
mod tests {
    use super::*;

    fn vec3(x: f32, y: f32, z: f32) -> Vec<f32> { vec![x, y, z] }

    #[test]
    fn test_parallel_vectors() {
        // 완전 평행 → G = 0 → Parallel
        let engine = GMetricEngine::default();
        let q = normalize(&vec3(1.0, 1.0, 0.0));
        let k = vec![normalize(&vec3(1.0, 1.0, 0.0))];
        let result = engine.compute(&q, &k);
        assert!(result.g < 0.001, "평행 벡터: G≈0 기대, 실제={}", result.g);
        assert_eq!(result.classification, GClass::Parallel);
    }

    #[test]
    fn test_orthogonal_vectors() {
        // 완전 직교 → G = 1 → Orthogonal
        let engine = GMetricEngine::default();
        let q = normalize(&vec3(1.0, 0.0, 0.0));
        let k = vec![normalize(&vec3(0.0, 1.0, 0.0))];
        let result = engine.compute(&q, &k);
        assert!((result.g - 1.0).abs() < 0.001, "직교 벡터: G≈1 기대, 실제={}", result.g);
        assert_eq!(result.classification, GClass::Orthogonal);
    }

    #[test]
    fn test_goldilocks_zone() {
        let engine = GMetricEngine::default();
        // 45도 각도 → cos=0.707 → G=0.293 → Goldilocks
        let q = normalize(&vec3(1.0, 1.0, 0.0));
        let k = vec![normalize(&vec3(1.0, 0.0, 0.0))];
        let result = engine.compute(&q, &k);
        assert_eq!(result.classification, GClass::Goldilocks);
        assert!(result.decomposition.novelty_ratio > 0.10);
        assert!(result.decomposition.novelty_ratio < 0.80);
    }

    #[test]
    fn test_void_knowledge() {
        let engine = GMetricEngine::default();
        let q = normalize(&vec3(0.5, 0.5, 0.5));
        let result = engine.compute(&q, &[]);
        assert_eq!(result.classification, GClass::VoidKnowledge);
        assert_eq!(result.g, 1.0);
    }

    #[test]
    fn test_novelty_pricing() {
        let engine = GMetricEngine::default();
        // G=0.10 → price=0.01 (최소)
        let p_min = engine.novelty_price(0.10);
        // G=0.80 → price=0.08 (최대)
        let p_max = engine.novelty_price(0.80);
        assert!(p_min < p_max, "가격이 G에 비례해야 함");
        assert!(p_min >= 0.01, "최소 가격 0.01 BNKR");
        assert!(p_max <= 0.08, "최대 가격 0.08 BNKR");
    }

    #[test]
    fn test_sfe_analog_metrics() {
        let metrics = SfeAnalogMetrics::calculate(1000, 350, 0.45);
        assert!(metrics.knowledge_snr > 0.0);
        assert!(metrics.bandwidth_efficiency > 0.0);
        assert!(metrics.bandwidth_efficiency <= 1.0);
        println!(
            "SFE 유사 지표: SNR={:.2} Efficiency={:.1}% Purity={:.2}",
            metrics.knowledge_snr,
            metrics.bandwidth_efficiency * 100.0,
            metrics.information_purity
        );
    }

    #[test]
    fn test_orthogonal_component_extraction() {
        // Q = (1,1,0), K = (1,0,0) → Q_⊥ = (0,1,0)
        let q = vec![1.0_f32, 1.0, 0.0];
        let k = vec![1.0_f32, 0.0, 0.0];
        let q_perp = orthogonal_component(&q, &k);
        assert!((q_perp[0]).abs() < 0.001, "X성분은 0이어야 함");
        assert!((q_perp[1] - 1.0).abs() < 0.001, "Y성분은 1이어야 함");
    }
}
