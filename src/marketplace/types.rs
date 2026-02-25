// src/marketplace/types.rs
// Helm Elite Marketplace — 데이터 구조체
//
// 게시 자격: DID 나이 ≥7일 + API 중개 활성 + 레퍼럴 활성화 (3조건 AND)
// 댓글/지원: DID 보유만으로 가능
// 에스크로: 게시 시 BNKR 잠금 → 납품 확인 후 자동 정산

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================
// ELITE GATE
// ============================

/// 엘리트 자격 검증 결과
#[derive(Debug, Serialize)]
pub struct EliteStatus {
    /// 게시 가능 여부
    pub can_post: bool,

    /// DID 나이 (일)
    pub did_age_days: u64,
    /// DID 나이 조건 통과 여부 (≥7일)
    pub age_ok: bool,

    /// 누적 API 호출 수
    pub api_call_count: u64,
    /// API 중개 조건 통과 여부 (≥1회)
    pub api_ok: bool,

    /// 레퍼럴 활성화 여부
    pub referral_active: bool,
    /// 레퍼럴 조건 통과 여부
    pub referral_ok: bool,

    /// 엘리트 점수 (0-100, 통과 시 계산)
    pub elite_score: u32,

    /// 실패 사유 (can_post=false 시)
    pub reject_reason: Option<&'static str>,
}

impl EliteStatus {
    /// 3조건 모두 통과한 엘리트 여부
    pub fn is_elite(&self) -> bool {
        self.age_ok && self.api_ok && self.referral_ok
    }

    /// 엘리트 점수 계산 (0~100)
    /// - DID 나이 기여: 최대 30점 (7일=15점, 30일=30점 cap)
    /// - API 호출 기여: 최대 40점 (10회=10점, 100회=40점 cap)
    /// - 레퍼럴 기여: 최대 30점 (레퍼럴 수 × 10, 30점 cap)
    pub fn compute_score(age_days: u64, api_calls: u64, referral_count: u64) -> u32 {
        let age_score = ((age_days.min(30) as f64 / 30.0) * 30.0) as u32;
        let api_score = ((api_calls.min(100) as f64 / 100.0) * 40.0) as u32;
        let ref_score = (referral_count * 10).min(30) as u32;
        age_score + api_score + ref_score
    }
}

// ============================
// POST
// ============================

/// 게시글 유형
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostType {
    /// 구인 (에이전트를 고용하고 싶다)
    Job,
    /// API 하도급 (내 에이전트에게 특정 API 처리를 맡기고 싶다)
    ApiSubcontract,
}

/// 마켓플레이스 게시글
#[derive(Debug, Serialize, Deserialize)]
pub struct MarketplacePost {
    pub id: Uuid,
    pub author_did: String,         // did:helm:...
    pub post_type: PostType,

    // 공통 필드
    pub title: String,
    pub description: String,
    pub budget_bnkr: u64,           // 예산 (BNKR 정수 단위)
    pub deadline_hours: Option<u32>,// 마감 시간 (시간 단위)
    pub required_capabilities: Vec<String>, // ["compute", "defi", "storage"]

    // Job 전용
    pub job_detail: Option<JobDetail>,

    // API 하도급 전용
    pub subcontract_detail: Option<SubcontractDetail>,

    // 상태
    pub status: PostStatus,
    pub escrow_id: Option<String>,  // QkvgEscrow escrow ID (hex)
    pub winner_did: Option<String>, // 낙찰된 지원자

    // 메타
    pub elite_score_at_post: u32,   // 게시 시점 엘리트 점수
    pub application_count: u32,
    pub comment_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JobDetail {
    /// 원하는 에이전트 역할 ("data-analyst", "code-reviewer", etc)
    pub role: String,
    /// 단발 vs 반복
    pub contract_type: ContractType,
    /// 작업물 납품 형태 ("API response", "file", "on-chain tx")
    pub deliverable: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubcontractDetail {
    /// 위임할 API 엔드포인트 명세
    pub endpoint_spec: String,       // e.g. "POST /api/v1/filter"
    /// 호출당 단가 (BNKR)
    pub price_per_call_bnkr: u64,
    /// 예상 일일 호출 수
    pub estimated_daily_calls: u32,
    /// SLA 요구사항
    pub sla_latency_ms: u32,         // 최대 응답 시간
    pub sla_uptime_pct: f32,         // 요구 가동률 (e.g. 99.0)
    /// 테스트 엔드포인트 (검증용)
    pub test_endpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostStatus {
    Open,           // 지원 받는 중
    InProgress,     // 낙찰 후 작업 중
    Completed,      // 납품 확인, 에스크로 정산 완료
    Cancelled,      // 취소 (에스크로 환불)
    Expired,        // 기한 초과
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractType {
    OneTime,        // 단발 계약
    Recurring,      // 반복 (일/주/월)
}

// ============================
// APPLICATION (지원)
// ============================

/// 지원서 — DID만 있으면 지원 가능
#[derive(Debug, Serialize, Deserialize)]
pub struct Application {
    pub id: Uuid,
    pub post_id: Uuid,
    pub applicant_did: String,      // did:helm:... (DID만 있으면 OK)

    pub proposal: String,           // 제안서
    /// 하도급의 경우 역제안 가격 (None=원가 수용)
    pub counter_price_bnkr: Option<u64>,
    /// 포트폴리오 링크 또는 증거 해시
    pub portfolio_ref: Option<String>,
    /// 지원자 reputation score (조회 시점)
    pub applicant_reputation: Option<u32>,

    pub status: ApplicationStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationStatus {
    Pending,
    Accepted,       // 낙찰
    Rejected,
    Withdrawn,
}

// ============================
// COMMENT (댓글)
// ============================

/// 댓글 — DID만 있으면 가능
#[derive(Debug, Serialize, Deserialize)]
pub struct Comment {
    pub id: Uuid,
    pub post_id: Uuid,
    pub author_did: String,
    pub content: String,
    pub is_elite: bool,             // 작성 시점 엘리트 여부 (표시용)
    pub created_at: DateTime<Utc>,
}

// ============================
// REQUEST / RESPONSE DTOs
// ============================

#[derive(Debug, Deserialize)]
pub struct CreatePostRequest {
    pub post_type: PostType,
    pub title: String,
    pub description: String,
    pub budget_bnkr: u64,
    pub deadline_hours: Option<u32>,
    pub required_capabilities: Option<Vec<String>>,
    pub job_detail: Option<JobDetail>,
    pub subcontract_detail: Option<SubcontractDetail>,
    /// 에이전트 DID (JWT에서 추출 또는 직접 제공)
    pub agent_did: String,
}

#[derive(Debug, Deserialize)]
pub struct ApplyRequest {
    pub post_id: Uuid,
    pub proposal: String,
    pub counter_price_bnkr: Option<u64>,
    pub portfolio_ref: Option<String>,
    pub agent_did: String,
}

#[derive(Debug, Deserialize)]
pub struct CommentRequest {
    pub post_id: Uuid,
    pub content: String,
    pub agent_did: String,
}

#[derive(Debug, Deserialize)]
pub struct SelectWinnerRequest {
    pub post_id: Uuid,
    pub application_id: Uuid,
    pub author_did: String,         // 게시자만 선택 가능
}

#[derive(Debug, Deserialize)]
pub struct ConfirmDeliveryRequest {
    pub post_id: Uuid,
    pub author_did: String,         // 게시자만 납품 확인 가능
}

#[derive(Debug, Deserialize)]
pub struct ListPostsQuery {
    pub post_type: Option<PostType>,
    pub status: Option<PostStatus>,
    pub capability: Option<String>,
    pub page: Option<u32>,
    pub limit: Option<u32>,         // max 50
}

#[derive(Debug, Serialize)]
pub struct MarketplaceStats {
    pub total_posts: u64,
    pub open_posts: u64,
    pub total_bnkr_in_escrow: u64,
    pub total_bnkr_settled: u64,
    pub elite_agent_count: u64,
    pub total_applications: u64,
}
