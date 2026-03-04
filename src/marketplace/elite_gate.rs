// src/marketplace/elite_gate.rs
// 엘리트 자격 검증 — 게시 권한 3조건 AND
//
// [조건 1] DID 나이 ≥ 7일   (local_visas.created_at)
// [조건 2] API 중개 활성    (api_call_logs.agent_did COUNT ≥ 1)
// [조건 3] 레퍼럴 활성화    (referral_links.referrer_did EXISTS)
//
// 댓글/지원은 DID만 있으면 가능 — EliteGate 불필요

use chrono::Utc;
use sqlx::PgPool;
use crate::marketplace::types::EliteStatus;

pub const MIN_DID_AGE_DAYS:   u64 = 7;
pub const MIN_API_CALLS:      u64 = 1;

pub struct EliteGate {
    db: PgPool,
}

impl EliteGate {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// DID의 엘리트 자격을 검증한다.
    /// 게시 요청마다 호출 — 결과를 캐시하지 않음 (실시간 반영).
    pub async fn check(&self, agent_did: &str) -> Result<EliteStatus, sqlx::Error> {

        // ── 1. DID 나이 & 기본 정보 ─────────────────────────────────
        let row = sqlx::query!(
            r#"
            SELECT
                created_at,
                total_calls,
                referrer_did
            FROM local_visas
            WHERE local_did = $1
            "#,
            agent_did
        )
        .fetch_optional(&self.db)
        .await?;

        let Some(visa) = row else {
            // DID가 아예 없으면 즉시 거부
            return Ok(EliteStatus {
                can_post: false,
                did_age_days: 0,
                age_ok: false,
                api_call_count: 0,
                api_ok: false,
                referral_active: false,
                referral_ok: false,
                elite_score: 0,
                reject_reason: Some("DID not registered"),
            });
        };

        // 나이 계산
        let age_days = Utc::now()
            .signed_duration_since(visa.created_at)
            .num_days()
            .max(0) as u64;
        let age_ok = age_days >= MIN_DID_AGE_DAYS;

        // ── 2. API 중개 활성 여부 ────────────────────────────────────
        // local_visas.total_calls 는 api_broker.rs 가 BillingLedger 통해 증가시킴
        let api_calls = visa.total_calls.max(0) as u64;
        let api_ok = api_calls >= MIN_API_CALLS;

        // ── 3. 레퍼럴 활성화 여부 ───────────────────────────────────
        // referrer_did 컬럼이 있으면 레퍼럴 등록된 것
        let referral_active = visa.referrer_did.is_some();
        let referral_ok = referral_active;

        // ── 레퍼럴 인원 수 (점수 계산용) ────────────────────────────
        let referral_count: i64 = sqlx::query_scalar!(
            r#"SELECT COUNT(*)::bigint FROM local_visas WHERE referrer_did = $1"#,
            agent_did
        )
        .fetch_one(&self.db)
        .await?
        .unwrap_or(0);

        // ── 최종 판정 ───────────────────────────────────────────────
        let can_post = age_ok && api_ok && referral_ok;

        let reject_reason = if !age_ok {
            Some("DID must be active for ≥7 days before posting")
        } else if !api_ok {
            Some("At least 1 API call required before posting")
        } else if !referral_ok {
            Some("Referral must be activated before posting")
        } else {
            None
        };

        let elite_score = if can_post {
            EliteStatus::compute_score(age_days, api_calls, referral_count as u64)
        } else {
            0
        };

        Ok(EliteStatus {
            can_post,
            did_age_days: age_days,
            age_ok,
            api_call_count: api_calls,
            api_ok,
            referral_active,
            referral_ok,
            elite_score,
            reject_reason,
        })
    }

    /// DID 존재 여부만 빠르게 확인 (댓글/지원 권한용)
    pub async fn did_exists(&self, agent_did: &str) -> Result<bool, sqlx::Error> {
        let count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*)::bigint FROM local_visas WHERE local_did = $1",
            agent_did
        )
        .fetch_one(&self.db)
        .await?
        .unwrap_or(0);
        Ok(count > 0)
    }
}

// ============================
// TESTS
// ============================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elite_score_zero_when_minimum() {
        // 7일, 1회, 0 레퍼럴
        let score = EliteStatus::compute_score(7, 1, 0);
        // age: (7/30)*30 = 7,  api: (1/100)*40 = 0,  ref: 0
        assert!(score > 0 && score < 50);
    }

    #[test]
    fn elite_score_max_at_30days_100calls_3refs() {
        let score = EliteStatus::compute_score(30, 100, 3);
        assert_eq!(score, 100);
    }

    #[test]
    fn elite_score_cap_works() {
        // 초과해도 100 이하
        let score = EliteStatus::compute_score(365, 10000, 100);
        assert_eq!(score, 100);
    }
}
