// src/filter/qkvg.rs
// [Day 2] Helm-sense 3-Layer 필터 엔진
//
// 설계 (Helm_Ego.txt G-Metric + Helm_Project.txt SyncO 통합):
//
//   Layer 1: Fast Heuristics   — O(1), SyncO 재활용, 40% 드롭
//   Layer 2: Semantic Dedup    — XXHash3 → Cosine, 20% 추가 드롭
//   Layer 3: Goldilocks Zone   — G-Metric, 수익 판정
//
// 골디락스 존:
//   G < 0.10: 복붙 기사 (Drop)
//   G 0.10~0.80: 신규 정보 (Accept + Novelty Premium)
//   G > 0.80: 스팸/주제이탈 (Drop)

use lazy_static::lazy_static;
use regex::Regex;
use xxhash_rust::xxh3::xxh3_64;
use std::collections::{HashSet, VecDeque};
use parking_lot::RwLock;

// ============================
// LAYER 1 — FAST HEURISTICS
// ============================

lazy_static! {
    // HTML 태그 (Link Farm 탐지용)
    static ref HTML_TAG: Regex = Regex::new(r"<[^>]{1,200}>").unwrap();

    // Base64/바이너리 덩어리 (500자+ 무공백)
    static ref LONG_TOKEN: Regex = Regex::new(r"\S{500,}").unwrap();

    // 광고성 한국어 패턴
    static ref SPAM_KO: Regex = Regex::new(
        r"(?i)(지금\s*바로\s*구매|한정\s*특가|무료\s*체험|클릭하세요|광고|sponsored|AD:|쿠폰코드)"
    ).unwrap();

    // 광고성 영어 패턴
    static ref SPAM_EN: Regex = Regex::new(
        r"(?i)(click here|buy now|limited offer|free trial|subscribe now|discount code|promo code|affiliate)"
    ).unwrap();
}

/// Layer 1 판정 결과
#[derive(Debug, Clone, PartialEq)]
pub enum L1Result {
    Pass,
    Drop(L1DropReason),
}

#[derive(Debug, Clone, PartialEq)]
pub enum L1DropReason {
    TooShort,
    TooLong,
    BinaryContent,  // Base64/바이너리
    LinkFarm,       // HTML 태그 과다
    SpamPattern,    // 광고성 키워드
}

/// Layer 1: Fast Heuristics (O(1) — SyncO 엔진 재활용)
/// 40% 트래픽을 < 1ms에 제거
pub fn layer1_heuristic(text: &str) -> L1Result {
    let len = text.len();

    // [Rule 1] 길이 체크
    if len < 100 {
        return L1Result::Drop(L1DropReason::TooShort);
    }
    if len > 100_000 {
        return L1Result::Drop(L1DropReason::TooLong);
    }

    // [Rule 2] 바이너리/Base64 덩어리
    if LONG_TOKEN.is_match(text) {
        return L1Result::Drop(L1DropReason::BinaryContent);
    }

    // [Rule 3] HTML 비율 (Link Farm)
    let html_count = text.matches('<').count();
    let word_count = text.split_whitespace().count().max(1);
    if html_count as f32 / word_count as f32 > 0.3 {
        return L1Result::Drop(L1DropReason::LinkFarm);
    }

    // [Rule 4] 스팸 패턴
    if SPAM_KO.is_match(text) || SPAM_EN.is_match(text) {
        return L1Result::Drop(L1DropReason::SpamPattern);
    }

    L1Result::Pass
}

// ============================
// LAYER 2 — SEMANTIC DEDUP
// ============================

/// Layer 2 판정 결과
#[derive(Debug)]
pub enum L2Result {
    /// 통과 — 벡터와 함께
    Pass(Vec<f32>),
    /// 완전 중복 (XXHash3 일치)
    ExactDuplicate,
    /// 의미론적 중복 (코사인 유사도 > 0.95)
    SemanticDuplicate(f32),
}

/// 최근 뉴스 벡터 캐시 (슬라이딩 윈도우)
pub struct VectorCache {
    /// 최근 N건의 임베딩 벡터 (6시간치)
    vectors: RwLock<VecDeque<Vec<f32>>>,
    /// XXHash3 기반 완전 중복 세트
    exact_hashes: RwLock<HashSet<u64>>,
    /// 최대 캐시 크기
    max_size: usize,
}

impl VectorCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            vectors: RwLock::new(VecDeque::new()),
            exact_hashes: RwLock::new(HashSet::new()),
            max_size,
        }
    }

    /// 벡터 추가 (슬라이딩 윈도우)
    pub fn insert(&self, text: &str, vector: Vec<f32>) {
        let hash = xxh3_64(text.as_bytes());

        let mut hashes = self.exact_hashes.write();
        let mut vectors = self.vectors.write();

        if vectors.len() >= self.max_size {
            vectors.pop_front();
            // 해시는 별도 관리 (메모리 최적화)
        }

        hashes.insert(hash);
        vectors.push_back(vector);
    }

    /// 완전 중복 체크 (O(1))
    pub fn is_exact_duplicate(&self, text: &str) -> bool {
        let hash = xxh3_64(text.as_bytes());
        self.exact_hashes.read().contains(&hash)
    }

    /// 최대 코사인 유사도 계산
    pub fn max_cosine_similarity(&self, query: &[f32]) -> f32 {
        let vectors = self.vectors.read();
        vectors
            .iter()
            .map(|k| cosine_similarity(query, k))
            .fold(0.0_f32, f32::max)
    }
}

/// 코사인 유사도 계산
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

/// Layer 2: Semantic Deduplication
pub fn layer2_dedup(
    text: &str,
    vector: Vec<f32>,
    cache: &VectorCache,
) -> L2Result {
    // [Fast Path] XXHash3 완전 중복 (O(1))
    if cache.is_exact_duplicate(text) {
        return L2Result::ExactDuplicate;
    }

    // [Semantic Path] 코사인 유사도
    let max_sim = cache.max_cosine_similarity(&vector);

    if max_sim > 0.95 {
        return L2Result::SemanticDuplicate(max_sim);
    }

    L2Result::Pass(vector)
}

// ============================
// LAYER 3 — GOLDILOCKS G-METRIC
// ============================

/// Layer 3 판정 결과
#[derive(Debug, Clone)]
pub struct GoldilocksResult {
    pub verdict: GoldilocksVerdict,
    pub g_score: f32,
    /// 부과할 Novelty Premium (BNKR)
    pub novelty_price_bnkr: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GoldilocksVerdict {
    /// 유의미한 신규 정보 — 과금 발생
    NovelDelta,
    /// 신규 토픽 최초 발생
    NewTopic,
    /// 복붙 (G < 0.10) — Base Toll만
    Duplicate,
    /// 스팸/주제이탈 (G > 0.80) — Base Toll만
    SpamOrOfftopic,
}

/// Layer 3: Goldilocks G-Metric
/// (Helm_Ego.txt Python → Rust 이식)
///
/// G-Metric = 1.0 - max_cosine_similarity(Q, K)
/// 0.0 = 완전 일치 (복붙), 1.0 = 완전 직교 (무관)
pub fn layer3_goldilocks(
    query_vector: &[f32],
    topic_knowledge: &[Vec<f32>],
) -> GoldilocksResult {
    const THRESHOLD_DUPLICATE: f32 = 0.10;
    const THRESHOLD_SPAM: f32 = 0.80;

    // 지식 베이스가 비어있으면 = 신규 토픽
    if topic_knowledge.is_empty() {
        return GoldilocksResult {
            verdict: GoldilocksVerdict::NewTopic,
            g_score: 1.0,
            novelty_price_bnkr: 0.05, // 신규 토픽 프리미엄
        };
    }

    let max_sim: f32 = topic_knowledge
        .iter()
        .map(|k| cosine_similarity(query_vector, k))
        .fold(0.0_f32, f32::max);

    let g = 1.0 - max_sim;

    let (verdict, price) = if g < THRESHOLD_DUPLICATE {
        (GoldilocksVerdict::Duplicate, 0.0)
    } else if g > THRESHOLD_SPAM {
        (GoldilocksVerdict::SpamOrOfftopic, 0.0)
    } else {
        // 골디락스 존: 0.01 ~ 0.08 BNKR (G에 비례)
        let price = 0.01 + (g - THRESHOLD_DUPLICATE) as f64 * 0.1;
        let price = (price * 1000.0).round() / 1000.0; // 소수점 3자리
        (GoldilocksVerdict::NovelDelta, price)
    };

    GoldilocksResult {
        verdict,
        g_score: g,
        novelty_price_bnkr: price,
    }
}

// ============================
// FULL PIPELINE
// ============================

/// 3-Layer 통합 판정 결과
#[derive(Debug)]
pub struct FilterDecision {
    pub action: FilterAction,
    pub drop_reason: Option<String>,
    pub g_score: Option<f32>,
    pub novelty_price_bnkr: f64,
    pub base_toll_bnkr: f64,
    /// SyncO 정제 후 텍스트 (통과한 경우)
    pub clean_text: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum FilterAction {
    Accept,
    Drop,
}

impl FilterDecision {
    pub fn total_price(&self) -> f64 {
        self.base_toll_bnkr + self.novelty_price_bnkr
    }
}

/// Helm-sense 통합 파이프라인 실행
pub fn run_pipeline(
    text: &str,
    embedding: Vec<f32>,
    topic_knowledge: &[Vec<f32>],
    vector_cache: &VectorCache,
) -> FilterDecision {
    const BASE_TOLL: f64 = 0.0001; // 항상 부과

    // Layer 1
    match layer1_heuristic(text) {
        L1Result::Drop(reason) => {
            return FilterDecision {
                action: FilterAction::Drop,
                drop_reason: Some(format!("L1:{:?}", reason)),
                g_score: None,
                novelty_price_bnkr: 0.0,
                base_toll_bnkr: BASE_TOLL,
                clean_text: None,
            };
        }
        L1Result::Pass => {}
    }

    // Layer 2
    match layer2_dedup(text, embedding.clone(), vector_cache) {
        L2Result::ExactDuplicate => {
            return FilterDecision {
                action: FilterAction::Drop,
                drop_reason: Some("L2:ExactDuplicate".into()),
                g_score: Some(0.0),
                novelty_price_bnkr: 0.0,
                base_toll_bnkr: BASE_TOLL,
                clean_text: None,
            };
        }
        L2Result::SemanticDuplicate(sim) => {
            return FilterDecision {
                action: FilterAction::Drop,
                drop_reason: Some(format!("L2:SemanticDuplicate(sim={:.3})", sim)),
                g_score: Some(1.0 - sim),
                novelty_price_bnkr: 0.0,
                base_toll_bnkr: BASE_TOLL,
                clean_text: None,
            };
        }
        L2Result::Pass(vec) => {
            // Layer 3
            let goldilocks = layer3_goldilocks(&vec, topic_knowledge);

            let (action, drop_reason) = match goldilocks.verdict {
                GoldilocksVerdict::NovelDelta | GoldilocksVerdict::NewTopic => {
                    // 캐시에 추가
                    vector_cache.insert(text, vec);
                    (FilterAction::Accept, None)
                }
                GoldilocksVerdict::Duplicate => {
                    (FilterAction::Drop, Some("L3:Duplicate".into()))
                }
                GoldilocksVerdict::SpamOrOfftopic => {
                    (FilterAction::Drop, Some("L3:SpamOrOfftopic".into()))
                }
            };

            let clean_text = if action == FilterAction::Accept {
                Some(synco_clean(text))
            } else {
                None
            };

            FilterDecision {
                action,
                drop_reason,
                g_score: Some(goldilocks.g_score),
                novelty_price_bnkr: goldilocks.novelty_price_bnkr,
                base_toll_bnkr: BASE_TOLL,
                clean_text,
            }
        }
    }
}

/// SyncO 기본 정제 (HTML 제거 + 공백 압축)
fn synco_clean(text: &str) -> String {
    let no_html = HTML_TAG.replace_all(text, " ");
    let compressed = no_html.split_whitespace().collect::<Vec<_>>().join(" ");
    compressed.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer1_too_short() {
        assert_eq!(layer1_heuristic("짧은 텍스트"), L1Result::Drop(L1DropReason::TooShort));
    }

    #[test]
    fn test_layer1_spam() {
        let spam = "지금 바로 구매! ".repeat(20);
        let result = layer1_heuristic(&spam);
        assert!(matches!(result, L1Result::Drop(_)));
    }

    #[test]
    fn test_cosine_identical() {
        let v = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 0.001);
    }

    #[test]
    fn test_goldilocks_new_topic() {
        let q = vec![0.5, 0.5, 0.0];
        let result = layer3_goldilocks(&q, &[]);
        assert_eq!(result.verdict, GoldilocksVerdict::NewTopic);
        assert!(result.novelty_price_bnkr > 0.0);
    }

    #[test]
    fn test_goldilocks_duplicate() {
        let v = vec![1.0, 0.0, 0.0];
        let knowledge = vec![v.clone()];
        let result = layer3_goldilocks(&v, &knowledge);
        assert_eq!(result.verdict, GoldilocksVerdict::Duplicate);
        assert_eq!(result.novelty_price_bnkr, 0.0);
    }

    #[test]
    fn test_goldilocks_novel() {
        let q = vec![0.5, 0.5, 0.5];
        let k = vec![vec![1.0, 0.0, 0.0]];
        let result = layer3_goldilocks(&q, &k);
        // G ≈ 0.42 → 골디락스 존
        assert_eq!(result.verdict, GoldilocksVerdict::NovelDelta);
        assert!(result.novelty_price_bnkr > 0.0);
    }
}
