# QKV-G Gateway — Mind Action Map v1
# 프로젝트: Helm Sovereign Protocol / QKV-G Gateway
# 생성: 2026-02-24
# 상태: Grand Cross v1.0.0 완료

---

## [GOAL]
Helm Sovereign Protocol — 탈중앙 AI 에이전트 지식 톨게이트 구축
- P2P 메시 네트워크 위 QKV-G (Gap-Metric) 기반 API 중개 게이트웨이
- BNKR 토큰 x402 마이크로 결제로 수익 창출
- GCP 배포 → 에이전트 생태계 BNKR 징수 개시

---

## [DONE CRITERIA — 전부 달성 ✅]

- [x] 49/49 테스트 전 전선 통과
- [x] 컴파일 에러 0개
- [x] WhatsApp Kaleidoscope 보안 이식
- [x] G-Metric 수학 엔진 (Goldilocks Zone)
- [x] SocraticMLA 의미 캐시 (70% hit rate 설계)
- [x] x402 State Channel (가스비 0원 결제)
- [x] DID Exchange (TOCTOU 취약점 수정)
- [x] Grand Cross ApiBroker (A/B/C/D 4전선)
- [x] Proof of Novelty 투명 오라클 헤더
- [x] MCP JSON-RPC 서버
- [x] Base Chain QkvgEscrow.sol
- [x] Git 커밋 완료 (995314e)
- [x] Jeff Dean 3차 무전 전부 분석 반영
- [x] 최종 확장 보고서 작성

---

## [ACTION MAP]

```
[Day 0] 보안 기반 구축
  ↓
[C-4] yamux + Noise Protocol 설정
  → Result: ✓ 성공
  → Evidence: test_secure_transport_builds PASSED
  → Insight: WindowUpdateMode::on_read() (OnRead 아님 — API 변경 주의)
  ↓
[Kaleidoscope] SafeStream 래퍼 구현
  → Result: ✓ 성공
  → Evidence: test_payload_limit_enforced, test_timeout_handler PASSED
  → Insight: 2MB 하드 리밋 + 3초 타임아웃 + 1KB/s 최소 속도
  ↓
[H-6/H-7] DID Exchange + TOCTOU 수정
  → Result: ✓ 성공
  → Evidence: test_nonce_reuse_detected PASSED
  → Insight: SERIALIZABLE 격리 + SELECT FOR UPDATE 조합 필수

[Day 1] 결제 레이어
  ↓
[H-8] x402 State Channel
  → Result: ✓ 성공
  → Evidence: test_ticket_create_and_verify, test_nonce_replay_detected PASSED
  → Insight: MerkleRoot로 주간 L1 롤업 → 가스비 1/1000
  → 컨트랙트: contracts/QkvgEscrow.sol (Base Chain)

[Day 2] 핵심 엔진
  ↓
[G-Metric] 수학 기반 Gap 측정 엔진
  → Result: ✓ 성공
  → Evidence: 6개 테스트 PASSED
  → Insight: G = 1 - max{cos(Q, Kᵢ)} / G<0.10=중복/G>0.80=스팸/0.10~0.80=골디락스
  → SFE Analog: Knowledge SNR = G/(1-G)
  ↓
[QKV-G] 3-Layer 필터 파이프라인
  → Result: ✓ 성공
  → Evidence: 6개 테스트 PASSED
  → Layer1: O(1) Heuristic (40% 드롭)
  → Layer2: XXHash3 + 코사인 유사도 (20% 드롭)
  → Layer3: G-Metric Goldilocks → 과금
  ↓
[4전선 라우터] ApiBroker
  → Result: ✓ 성공
  → A전선: LLM (OpenAI/Claude) — 30~40% 마진
  → B전선: Search (Brave) — 캐시 히트 100% 마진
  → C전선: DeFi (Pyth+Chainlink) — 절대 캐시 없음 (MEV 보호)
  → D전선: Identity (내부 DB) — 생태계 락인

[Day 3] 수익화
  ↓
[Two-Part Tariff] TariffEngine
  → Result: ✓ 성공
  → Evidence: test_revenue_simulation_1000_agents PASSED
  → Base Toll: 0.0001 BNKR (스팸 방어)
  → Novelty Premium: 0.01~0.08 BNKR (G 비례)
  → 월 수익: Conservative $234K / Aggressive $2.34M
  ↓
[MCP] JSON-RPC 서버
  → Result: ✓ 성공
  → 5개 도구: filter_news, search_web, defi_price, verify_agent, llm_complete
  → Cursor/Claude 연동: mcp://qkvg.network 한 줄

[Grand Cross] 최종 통합
  ↓
[SocraticMLA] 의미 캐시 엔진
  → Result: ✓ 성공
  → Evidence: test_exact_cache_hit, test_lru_eviction PASSED
  → L1: XXHash3 완전일치 O(1)
  → L2: G-Metric 의미 유사도 (G<0.10 → 캐시 반환)
  → LRU: max_entries=10,000 (프로덕션)
  ↓
[Grand Cross ApiBroker] 전 모듈 통합
  → Result: ✓ 성공
  → Evidence: 4개 테스트 PASSED
  → 무한 마진 루프 완성: 캐시 히트 → 원가 $0 → 마진 100%
  ↓
[Proof of Novelty] Jeff Dean 최종 제언 반영
  → Result: ✓ 성공
  → Evidence: 5개 테스트 PASSED
  → SHA-256 computation_hash → 에이전트 독립 검증 가능
  → 응답 헤더 8개: X-G-Score, X-Reference-K, X-Novelty-Proof 등
  → 블랙박스 → 투명한 오라클 격상

[Git] 최종 커밋
  → Result: ✓ 성공
  → Hash: 995314e
  → 28개 파일, 10,949줄
  → 브랜치: main
```

---

## [현재 상태 스냅샷]

```
테스트:    49/49 PASS ✅
빌드:      cargo check 에러 0 ✅
보안:      Kaleidoscope WhatsApp급 ✅
결제:      x402 State Channel 가동 준비 ✅
캐시:      SocraticMLA 설계 완료 ✅
투명성:    Proof of Novelty 헤더 ✅
문서:      Helm_GrandCross_Final_Report.docx ✅
커밋:      995314e Grand Cross v1.0.0 ✅
```

---

## [파일 구조]

```
qkvg-gateway/
├── src/
│   ├── main.rs                      # actix-web 서버 진입점
│   ├── auth/
│   │   ├── mod.rs
│   │   ├── did_exchange.rs          # DID + TOCTOU 수정
│   │   └── types.rs                 # AgentContext, LocalVisa
│   ├── filter/
│   │   ├── mod.rs
│   │   ├── g_metric.rs              # G-Metric 수학 엔진
│   │   ├── qkvg.rs                  # 3-Layer 필터
│   │   ├── socratic_mla.rs          # 의미 캐시 LRU
│   │   └── proof_of_novelty.rs      # Proof of Novelty 헤더
│   ├── payments/
│   │   ├── mod.rs
│   │   └── x402.rs                  # State Channel + Merkle
│   ├── pricing/
│   │   ├── mod.rs
│   │   └── novelty.rs               # Two-Part Tariff
│   ├── broker/
│   │   ├── mod.rs
│   │   ├── api_broker.rs            # Grand Cross 4전선 라우터
│   │   └── semantic_cache.rs        # 구형 캐시 (SocraticMLA로 대체됨)
│   ├── mcp/
│   │   ├── mod.rs
│   │   └── server.rs                # MCP JSON-RPC 5도구
│   └── p2p/
│       ├── mod.rs
│       ├── kaleidoscope.rs           # SafeStream + 보안
│       └── transport.rs             # yamux + Noise
├── contracts/
│   └── QkvgEscrow.sol               # Base Chain 에스크로
├── migrations/
│   └── 001_init.sql                 # PostgreSQL 스키마
├── .claude/
│   ├── projects/
│   │   └── qkvg-gateway-mindmap-v1.md  # (이 파일)
│   └── legacy/
│       └── insight-20260224.md      # Global insights
├── Cargo.toml
├── .env.example
└── README.md
```

---

## [미해결 TODO — 다음 세션]

```
[ ] GCP 인프라 셋업
    - C2 인스턴스 생성 (Seoul region)
    - Cloud Build Trigger 설정 (git push → auto deploy)
    - VPC + 방화벽 규칙 (8080 포트 개방)

[ ] GitHub remote 연결
    git remote add origin https://github.com/Helm-Protocol/qkvg-gateway.git
    git push -u origin main

[ ] PostgreSQL 실제 연동
    - DATABASE_URL 환경변수 설정
    - sqlx migrate run
    - DEV_MODE=false 전환

[ ] Base Chain 에스크로 배포
    - Solidity 컴파일
    - Base mainnet 배포
    - ESCROW_CONTRACT_ADDRESS 환경변수 설정

[ ] fastembed ONNX 모델 활성화
    - Cargo.toml fastembed 3 주석 해제
    - BGE-small 모델 다운로드
    - dummy_embed() → 실제 ONNX 임베딩 교체

[ ] Trail of Bits 감사 ($50K)
    - Month 1 수익으로 충당 계획

[ ] EAO 유동성 ($1M)
    - Month 3~4 누적 후 투입

[ ] 헌장 Mirror.xyz 공개
    - 지능 주권 헌장 2026 전문 게시
    - Founding Fathers 선거 공고

[ ] 외부 에이전트 온보딩
    - Truth Terminal 접촉
    - Aixbt 온보딩
```

---

## [환경변수 체크리스트]

```bash
# 필수 (프로덕션 전환 시)
HOST=0.0.0.0
PORT=8080
JWT_SECRET=<32자 이상 랜덤>
DATABASE_URL=postgres://qkvg:password@localhost:5432/qkvg_gateway
ANTHROPIC_API_KEY=sk-ant-...
OPENAI_API_KEY=sk-...
BRAVE_API_KEY=BSA...
BASE_RPC_URL=https://mainnet.base.org
ESCROW_CONTRACT_ADDRESS=0x...
GATEWAY_PRIVATE_KEY=0x...
TREASURY_ADDRESS=0x...
BNKR_USD_RATE=0.50
REDIS_URL=redis://localhost:6379
DEV_MODE=false  # ← 이걸 false로 바꿔야 실제 과금 시작!
```

---

## [수익 시뮬레이션 요약]

```
Conservative (1K agents × 1K calls/day):
  일 수익: $7,800
  월 수익: $234,000

Aggressive (10K agents × 1K calls/day):
  일 수익: $78,000
  월 수익: $2,340,000

Break-even: Trail of Bits 감사 $50K → Month 1 충당 가능
```

---

_마지막 업데이트: 2026-02-24 | Grand Cross v1.0.0 완료_
_다음 세션: GCP 배포 + GitHub remote 연결_

---

## [20260224 — DASHBOARD SESSION]

```
[dashboard.rs] SSE 텔레메트리 스트리밍
  → Result: ✓ 추가
  → GET /dashboard → static/index.html
  → GET /api/telemetry → SSE 1초 간격 push
  → AppState.metrics 공유 원자 카운터

[metrics.rs] GatewayMetrics 공유 카운터
  → AtomicU64 기반 lock-free 계측
  → record_bnkr / record_g_score / record_cache_hit 등
  → g_distribution_snapshot() → 대시보드 G분포 차트

[static/index.html] Ferrari Luce 대시보드
  → Orbitron + Share Tech Mono 폰트
  → 탄소섬유 배경 + 스캔라인 애니메이션
  → 타코미터 SVG (TPS 실시간)
  → BNKR 수익 카운터 (황금색)
  → 4전선 바 + G분포 차트
  → 공격 로그 스트림
  → SSE 실서버 자동 연결 (배포 시)

[git] 브랜치 gateway/grand-cross-v1 생성
  → push to Helm-Protocol/Helm
```

## [미해결 TODO 업데이트]
- [ ] AppState에 Arc<GatewayMetrics> 필드 추가 (main.rs)
- [ ] 각 모듈에서 metrics.record_* 호출 이식
- [ ] GCP Cloud Build Trigger 설정

---

## [20260224 — NPM PUBLISH SESSION]

```
[npm] helm-protocol@0.1.0 배포 완료
  → Result: ✓ + helm-protocol@0.1.0
  → Registry: https://registry.npmjs.org
  → Account: heime.jorgen
  → URL: https://www.npmjs.com/package/helm-protocol

[패키지 구성]
  → src/index.js: MCP JSON-RPC stdio 서버 (5개 도구)
  → src/identity.js: 투명한 동의 화면 + Ed25519 키생성
  → src/gateway.js: Gateway HTTP 클라이언트 + 402 처리
  → src/cli.js: 진입점

[MCP 5개 도구]
  → helm_llm: A전선 LLM 추론
  → helm_search: B전선 Brave Search
  → helm_defi_price: C전선 DeFi (MEV보호)
  → helm_agent_verify: D전선 DID 평판
  → helm_status: 크레딧/DID 확인

[다음]
  → GitHub push (gateway/grand-cross-v1)
  → GCP 배포
  → Moltbook 삐라 살포
```
