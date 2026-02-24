// src/auth/types.rs
// DID Passport & Visa 타입 정의
//
// Passport: 외부 글로벌 DID (did:ethr:0xABC...)
// Visa:     내부 로컬 DID  (did:qkvg:agent_777)
//
// 설계 원칙:
//   - 에이전트는 자신의 글로벌 DID를 그대로 사용 (마찰 0)
//   - 내부적으로 Visa로 매핑하여 평판/잔액/히스토리 추적
//   - Visa가 생태계 락인의 핵심

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 글로벌 DID — 에이전트의 여권
/// 외부 표준 (did:ethr, did:key, ENS 등) 모두 수용
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalPassport {
    /// did:ethr:0xABC... 형식
    pub did: String,
    /// Ed25519 서명 (신원 증명)
    pub signature: Vec<u8>,
    /// 서명 대상 메시지 (재사용 공격 방지를 위한 nonce 포함)
    pub signed_message: String,
}

/// 로컬 Visa — 게이트웨이 내부 신원증
/// 모든 내부 연산의 Primary Key
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LocalVisa {
    pub id: Uuid,
    /// did:qkvg:agent_ULID 형식
    pub local_did: String,
    /// 매핑된 글로벌 DID
    pub global_did: String,
    /// 예치 잔액 (BNKR 단위)
    pub balance_bnkr: f64,
    /// 평판 점수 (0~1000, 초기값 100)
    pub reputation_score: i32,
    /// 평균 G-Metric (에이전트가 얼마나 새로운 정보를 요청하는지)
    pub g_score_avg: f64,
    /// 총 API 호출 수
    pub total_calls: i64,
    /// 총 결제액 (BNKR)
    pub total_paid_bnkr: f64,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
}

impl LocalVisa {
    /// 신규 Visa 생성 (기본값)
    pub fn new(local_did: String, global_did: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            local_did,
            global_did,
            balance_bnkr: 0.0,
            reputation_score: 100,  // 초기 신뢰 점수
            g_score_avg: 0.0,
            total_calls: 0,
            total_paid_bnkr: 0.0,
            created_at: now,
            last_active_at: now,
        }
    }

    /// Free Tier 한도 초과 여부
    pub fn is_free_tier_exhausted(&self) -> bool {
        self.total_calls >= 100
    }

    /// 잔액 충분 여부
    pub fn has_sufficient_balance(&self, required: f64) -> bool {
        self.balance_bnkr >= required
    }
}

/// DID Exchange 응답 — 에이전트에게 반환
#[derive(Debug, Serialize, Deserialize)]
pub struct VisaIssuanceResponse {
    pub local_did: String,
    pub session_token: String,
    pub balance_bnkr: f64,
    pub reputation_score: i32,
    pub free_calls_remaining: i64,
    pub message: String,
}

/// API 호출 컨텍스트 — 요청마다 파생
#[derive(Debug, Clone)]
pub struct AgentContext {
    pub local_did: String,
    pub global_did: String,
    pub balance_bnkr: f64,
    pub reputation_score: i32,
    pub is_free_tier: bool,
}

impl From<LocalVisa> for AgentContext {
    fn from(visa: LocalVisa) -> Self {
        let is_free = !visa.is_free_tier_exhausted();
        Self {
            local_did: visa.local_did,
            global_did: visa.global_did,
            balance_bnkr: visa.balance_bnkr,
            reputation_score: visa.reputation_score,
            is_free_tier: is_free,
        }
    }
}

/// 인증 에러
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("서명 검증 실패: {0}")]
    SignatureVerificationFailed(String),

    #[error("DID 형식 오류: {0}")]
    InvalidDidFormat(String),

    #[error("세션 만료")]
    SessionExpired,

    #[error("잔액 부족: 필요 {required} BNKR, 보유 {available} BNKR")]
    InsufficientBalance { required: f64, available: f64 },

    #[error("DB 오류: {0}")]
    DatabaseError(String),

    #[error("Nonce 재사용 공격 감지")]
    NonceReuse,
}
