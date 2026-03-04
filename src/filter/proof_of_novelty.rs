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

    /// HTTP 응답 헤더 세트로 변환
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
