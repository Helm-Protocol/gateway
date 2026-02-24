// src/filter/proof_of_novelty.rs
//
// ═══════════════════════════════════════════════════════════════
// PROOF OF NOVELTY  (Jeff Dean 최종 제언)
// ═══════════════════════════════════════════════════════════════
//
// "블랙박스"에서 "투명한 오라클"로 격상시키는 신뢰 레이어
//
// API 응답 헤더에 G-Metric 수학 증명을 첨부:
//
//   X-G-Score: 0.72
//   X-Reference-K: sha256("가장 유사한 기존 문서 요약")
//   X-Novelty-Proof: "기존 K와 비교 시 '수수료 90% 급감' 벡터가 직교"
//   X-Nearest-Doc-Hash: abc123def456...
//   X-Orthogonal-Component: 0.694
//
// 효과:
//   에이전트가 G-Metric 점수 조작을 의심할 수 없게 됨
//   수학적 투명성 = 브랜드 파워 = 생태계 신뢰 구축

use sha2::{Sha256, Digest};
use serde::Serialize;

// ============================
// PROOF TYPES
// ============================

/// Proof of Novelty — API 응답에 첨부되는 수학 증명서
#[derive(Debug, Clone, Serialize)]
pub struct NoveltyProof {
    /// G-Metric 점수 (0.0 ~ 1.0)
    pub g_score: f32,

    /// 가장 유사한 기존 문서의 해시 (Base Toll의 근거)
    /// SHA-256(nearest_document_summary)
    pub nearest_doc_hash: String,

    /// 직교 성분 크기 (순수 신규 정보 비율)
    pub orthogonal_component: f32,

    /// 인간 가독 설명
    pub novelty_reason: String,

    /// 과금 근거 BNKR 금액
    pub charged_bnkr: f64,

    /// 검증 가능한 입력 파라미터 해시
    /// SHA-256(query_text + k_hashes) — 에이전트가 직접 검증 가능
    pub computation_hash: String,
}

impl NoveltyProof {
    /// 증명서 생성
    pub fn generate(
        query_text: &str,
        g_score: f32,
        nearest_doc_summary: Option<&str>,
        orthogonal_component: f32,
        charged_bnkr: f64,
    ) -> Self {
        // SHA-256 해시 생성 — 에이전트가 직접 검증 가능한 체크섬
        let nearest_doc_hash = if let Some(doc) = nearest_doc_summary {
            let mut hasher = Sha256::new();
            hasher.update(doc.as_bytes());
            format!("{:x}", hasher.finalize())
        } else {
            "0000000000000000000000000000000000000000000000000000000000000000".into()
        };

        // 계산 투명성 해시 (입력 재현 가능성 보장)
        let mut comp_hasher = Sha256::new();
        comp_hasher.update(query_text.as_bytes());
        comp_hasher.update(&g_score.to_le_bytes());
        comp_hasher.update(nearest_doc_hash.as_bytes());
        let computation_hash = format!("{:x}", comp_hasher.finalize());

        // 인간 가독 설명 생성
        let novelty_reason = Self::generate_reason(g_score, orthogonal_component, nearest_doc_summary);

        Self {
            g_score,
            nearest_doc_hash,
            orthogonal_component,
            novelty_reason,
            charged_bnkr,
            computation_hash,
        }
    }

    /// G-Metric 결과를 인간 언어로 설명
    fn generate_reason(g: f32, ortho: f32, nearest: Option<&str>) -> String {
        if g < 0.10 {
            format!(
                "기존 지식과 {:.1}% 일치. 의미적 중복으로 캐시 반환.",
                (1.0 - g) * 100.0
            )
        } else if g > 0.80 {
            format!(
                "G={:.3} — 주제 이탈 감지. 기존 지식과 완전히 다른 방향의 쿼리.",
                g
            )
        } else if let Some(doc) = nearest {
            let summary = if doc.len() > 50 { &doc[..50] } else { doc };
            format!(
                "G={:.3} | 직교 성분={:.3} | 기존 문서 '{}...'에 없던 신규 정보 벡터 감지.",
                g, ortho, summary
            )
        } else {
            format!(
                "G={:.3} | 직교 성분={:.3} | 신규 토픽 — 지식 베이스에 전례 없음.",
                g, ortho
            )
        }
    }

    /// actix-web HTTP 응답 헤더 세트로 변환
    pub fn to_headers(&self) -> Vec<(String, String)> {
        vec![
            ("X-G-Score".into(), format!("{:.4}", self.g_score)),
            ("X-Reference-K".into(), self.nearest_doc_hash[..16].to_string()),
            ("X-Novelty-Proof".into(), self.novelty_reason.clone()),
            ("X-Orthogonal-Component".into(), format!("{:.4}", self.orthogonal_component)),
            ("X-Charged-BNKR".into(), format!("{:.6}", self.charged_bnkr)),
            ("X-Computation-Hash".into(), self.computation_hash[..16].to_string()),
            ("X-Helm-Version".into(), "0.1.0".into()),
            ("X-Charter".into(), "지능주권헌장2026-제17조".into()),
        ]
    }

    /// JSON 직렬화 (MCP 응답 본문에 포함)
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "g_score": self.g_score,
            "nearest_doc_hash": self.nearest_doc_hash,
            "orthogonal_component": self.orthogonal_component,
            "novelty_reason": self.novelty_reason,
            "charged_bnkr": self.charged_bnkr,
            "computation_hash": self.computation_hash,
            "verification": {
                "method": "SHA-256(query + g_score + nearest_k)",
                "independently_verifiable": true,
                "note": "에이전트가 동일한 입력으로 직접 해시 검증 가능"
            }
        })
    }
}

// ============================
// PROOF HEADER MIDDLEWARE
// ============================

/// API 응답에 Proof of Novelty 헤더를 자동 주입하는 헬퍼
///
/// actix-web 핸들러에서 사용:
///   let proof = NoveltyProof::generate(...);
///   let mut response = HttpResponse::Ok().json(data);
///   inject_proof_headers(&mut response, &proof);
pub fn build_proof_response(
    data: serde_json::Value,
    proof: &NoveltyProof,
) -> actix_web::HttpResponse {
    use actix_web::HttpResponse;

    let mut builder = HttpResponse::Ok();

    for (key, val) in proof.to_headers() {
        builder.append_header((key, val));
    }

    // 응답 본문에도 proof 포함 (선택적)
    let mut response_data = data;
    if let Some(obj) = response_data.as_object_mut() {
        obj.insert("_proof_of_novelty".into(), proof.to_json());
    }

    builder.json(response_data)
}

// ============================
// TESTS
// ============================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proof_generation_goldilocks() {
        let proof = NoveltyProof::generate(
            "이더리움 덴쿤 업그레이드 수수료 90% 감소",
            0.72,
            Some("이더리움 덴쿤 업그레이드 발표 (2024-03)"),
            0.694,
            0.072,
        );

        assert_eq!(proof.g_score, 0.72);
        assert!(!proof.nearest_doc_hash.is_empty());
        assert!(proof.novelty_reason.contains("G=0.720"));
        assert!(proof.computation_hash.len() == 64);
    }

    #[test]
    fn test_proof_headers_count() {
        let proof = NoveltyProof::generate("test query", 0.45, None, 0.4, 0.035);
        let headers = proof.to_headers();
        // 8개 헤더 확인
        assert_eq!(headers.len(), 8);
        // X-G-Score 헤더 존재 확인
        assert!(headers.iter().any(|(k, _)| k == "X-G-Score"));
        // X-Novelty-Proof 헤더 존재 확인
        assert!(headers.iter().any(|(k, _)| k == "X-Novelty-Proof"));
    }

    #[test]
    fn test_computation_hash_deterministic() {
        // 동일 입력 → 동일 해시 (에이전트 독립 검증 가능)
        let p1 = NoveltyProof::generate("bitcoin price", 0.5, None, 0.5, 0.05);
        let p2 = NoveltyProof::generate("bitcoin price", 0.5, None, 0.5, 0.05);
        assert_eq!(p1.computation_hash, p2.computation_hash);
    }

    #[test]
    fn test_duplicate_reason() {
        let proof = NoveltyProof::generate("old news", 0.05, Some("similar old article"), 0.05, 0.0001);
        assert!(proof.novelty_reason.contains("캐시 반환"));
    }

    #[test]
    fn test_void_knowledge_reason() {
        let proof = NoveltyProof::generate("brand new topic 2099", 0.95, None, 0.95, 0.05);
        assert!(proof.novelty_reason.contains("주제 이탈") || proof.novelty_reason.contains("신규 토픽"));
    }
}
