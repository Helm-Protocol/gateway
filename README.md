# QKV-G Gateway

**AI 에이전트 API 중개 + G-Metric 지식 필터링**

*지능 주권 헌장 2026 — 제17조 준수*

---

## 핵심 개념 (Jeff Dean 설계)

```
SFE Rev17    = 물리 계층(PHY) 노이즈 제거
G-Metric     = 애플리케이션 계층 '지식 노이즈' 제거
→ End-to-End 효율화 아키텍처 완성
```

### G-Metric 수학 기반

```
Q = 입력 벡터 (크롤링 뉴스)
K = 기존 지식 공간

G = 1.0 − max{ cos(Q, Kᵢ) : Kᵢ ∈ K }

G → 0.0 : Q ∥ K  복붙 기사 (DROP)
G → 1.0 : Q ⊥ K  완전 신규 (PREMIUM)
G ∈ (0.1, 0.8) : 골디락스 존 (ACCEPT + Novelty Premium)
```

---

## 4대 수익 전선

| 전선 | 대상 | 캐시 | 마진 |
|------|------|------|------|
| A: LLM | OpenAI/Anthropic | ✅ | 30~40% |
| B: Search | Brave + SyncO | ✅ (G-Metric) | 100% 히트시 |
| C: DeFi | Uniswap/Pyth | ❌ 절대 없음 | 0.1% 수수료 |
| D: Identity | DID Visa | 내부 처리 | 0.0001 BNKR/query |

---

## 빠른 시작

```bash
# 1. 환경변수 설정
cp .env.example .env

# 2. DB 마이그레이션
psql $DATABASE_URL < migrations/001_init.sql

# 3. 실행
cargo run

# 서버: http://localhost:8080
```

## API 사용

### DID 교환 (최초 1회)
```bash
curl -X POST http://localhost:8080/auth/exchange \
  -H "Content-Type: application/json" \
  -d '{"global_did":"did:ethr:0xABC","signature":"0x...","signed_message":"qkvg-auth:..."}'
```

### 뉴스 필터링 (B전선 핵심)
```bash
curl -X POST http://localhost:8080/api/filter \
  -H "Content-Type: application/json" \
  -d '{"texts":["이더리움 업그레이드...","광고성 기사..."]}'
```

### G-Metric 직접 계산
```bash
curl -X POST http://localhost:8080/api/g-metric \
  -H "Content-Type: application/json" \
  -d '{"query_text":"새로운 뉴스","knowledge_texts":["기존 지식1","기존 지식2"]}'
```

### MCP 연동 (Cursor/Claude)
```
Cursor Settings → MCP Servers → Add:
  URL: http://localhost:8080/mcp
  Name: QKV-G Gateway
```

---

## 과금 모델 (Two-Part Tariff)

```
Base Toll:        0.0001 BNKR (항상, 스팸방지)
Novelty Premium:  0.01 + (G - 0.10) × 0.10 BNKR (골디락스 존)
신규 토픽:         0.05 BNKR (고정 프리미엄)
Free Tier:        첫 100 calls 무료
```

---

## 보안 (Day 0 적용)

- **C-4**: yamux + Noise Protocol (중간자 공격 차단)
- **H-6/H-7**: DID TOCTOU → Serializable Transaction
- **H-8**: x402 Reentrancy → CEI 패턴 + nonReentrant

---

## 파일 구조

```
src/
├── main.rs              # actix-web 서버
├── p2p/transport.rs     # [C-4] 보안 P2P 전송
├── auth/
│   ├── types.rs         # Passport/Visa 타입
│   └── did_exchange.rs  # [H-6/H-7] DID 교환
├── payments/x402.rs     # [H-8] x402 State Channel
├── filter/
│   ├── g_metric.rs      # G-Metric 수학 엔진 (Jeff Dean)
│   └── qkvg.rs          # 3-Layer 필터 파이프라인
├── broker/mod.rs        # 4전선 API 라우터
├── pricing/novelty.rs   # Two-Part Tariff
└── mcp/server.rs        # MCP JSON-RPC

contracts/
└── QkvgEscrow.sol       # Base Chain 에스크로

migrations/
└── 001_init.sql         # PostgreSQL 스키마
```

---

*QKV-G Gateway v0.1.0 | 최고개발자 동무 제작*
