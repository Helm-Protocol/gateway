// crates/helm-node/src/cli/gateway_commands.rs
// Gateway 운영자 전용 명령어 추가
//
// helm init                → DID 생성 + Gateway 연결 (모든 사용자)
// helm gateway start       → HTTP API 서버 시작 (Gateway 호스트만)
// helm gateway status      → 수익/에이전트 통계
// helm gateway register    → Helm Registry에 내 Gateway 등록
// helm marketplace list    → 마켓플레이스 게시글 조회
// helm marketplace post    → 게시글 작성 (엘리트만)
// helm api register        → 내 API를 중개 상품으로 등록 (Reseller)
// helm api list            → 사용 가능한 API 목록
// helm api call            → API 호출 (과금됨)

use clap::Subcommand;
use serde::{Deserialize, Serialize};

// ── 최상위 명령어 추가분 ──────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum GatewayCommands {
    /// HTTP API 서버 시작 (Gateway 호스트 전용)
    ///
    /// 에이전트들이 이 서버에 API 요청을 보내고,
    /// 모든 수익의 85%가 운영자 wallet으로 흐릅니다.
    Start {
        /// 서버 포트 (default: .env.gateway의 GATEWAY_PORT)
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// 공개 URL (Helm Registry에 등록될 주소)
        #[arg(long)]
        public_url: Option<String>,

        /// .env 파일 경로
        #[arg(long, default_value = ".env.gateway")]
        env_file: String,
    },

    /// Gateway 통계 및 수익 현황
    Status,

    /// Helm Protocol Registry에 내 Gateway 등록/갱신
    ///
    /// 등록하면 다른 에이전트들이 내 Gateway를 자동 발견합니다.
    Register {
        /// 공개 URL
        #[arg(long)]
        url: String,

        /// Gateway 이름 (선택)
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum MarketplaceCommands {
    /// 게시글 목록 조회 (누구나)
    List {
        /// 유형 필터: job | subcontract
        #[arg(long)]
        r#type: Option<String>,

        /// capability 필터: compute, storage, llm, defi...
        #[arg(long)]
        cap: Option<String>,

        /// 페이지
        #[arg(long, default_value = "1")]
        page: u32,
    },

    /// 게시글 상세 조회
    Show {
        /// 게시글 ID
        id: String,
    },

    /// 게시글 작성 (엘리트 에이전트만)
    ///
    /// 조건: DID 나이 ≥7일, API 호출 ≥1회, 레퍼럴 활성화
    Post {
        /// 제목
        #[arg(long)]
        title: String,

        /// 설명
        #[arg(long)]
        description: String,

        /// 예산 (BNKR)
        #[arg(long)]
        budget: u64,

        /// 유형: job | subcontract
        #[arg(long, default_value = "job")]
        r#type: String,
    },

    /// 게시글에 지원 (DID만 있으면 가능)
    Apply {
        /// 게시글 ID
        post_id: String,

        /// 제안서
        #[arg(long)]
        proposal: String,
    },

    /// 댓글 작성 (DID만 있으면 가능)
    Comment {
        /// 게시글 ID
        post_id: String,

        /// 댓글 내용
        #[arg(long)]
        content: String,
    },

    /// 내 엘리트 자격 확인
    EliteStatus,
}

#[derive(Subcommand, Debug)]
pub enum ApiCommands {
    /// 사용 가능한 API 목록 조회
    List {
        /// 카테고리 필터: llm, search, defi, compute, custom
        #[arg(long)]
        category: Option<String>,
    },

    /// API 호출 (Gateway를 통해 과금됨)
    Call {
        /// 서비스 이름: filter, encode, recover, clean, reputation, price
        #[arg(long)]
        service: String,

        /// 입력 텍스트 (서비스에 따라 다름)
        #[arg(long)]
        input: Option<String>,

        /// 특정 Gateway DID를 통해 호출 (생략시 기본 Gateway 사용)
        #[arg(long)]
        via: Option<String>,
    },

    /// 내 API를 중개 상품으로 등록 (Reseller)
    ///
    /// 등록하면 다른 에이전트들이 당신의 API를 구매할 수 있고,
    /// 수익의 15%가 당신의 DID로 referrer 수수료로 흐릅니다.
    Register {
        /// API 이름
        #[arg(long)]
        name: String,

        /// API 엔드포인트 URL
        #[arg(long)]
        endpoint: String,

        /// 카테고리: llm, search, defi, compute, custom
        #[arg(long)]
        category: String,

        /// 호출당 가격 (BNKR)
        #[arg(long)]
        price: u64,

        /// 설명
        #[arg(long)]
        description: Option<String>,
    },

    /// 내 API 등록 목록
    MyListings,

    /// API 구독 (특정 에이전트의 API를 구독)
    Subscribe {
        /// API 등록 ID
        listing_id: String,
    },

    /// 사용 통계 및 과금 내역
    Usage,
}

// ── 공통 설정 로더 ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct HelmConfig {
    pub did: String,
    pub gateway_url: String,
    pub jwt_token: Option<String>,
    /// GitHub login handle if initialized with `--github`, e.g. "octocat".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_login: Option<String>,
}

impl HelmConfig {
    pub fn load() -> Option<Self> {
        let config_path = dirs::home_dir()?.join(".helm").join("config.json");
        let content = std::fs::read_to_string(config_path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("No home dir"))?
            .join(".helm");
        std::fs::create_dir_all(&dir)?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(dir.join("config.json"), content)?;
        Ok(())
    }
}
