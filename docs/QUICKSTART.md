# Helm Gateway — Developer Quickstart

> 5분 안에 첫 번째 API 호출하기

## 1. Install

```bash
# Gateway 서버 (Rust)
git clone https://github.com/anthropics/helm-gateway
cd helm-gateway
cargo build --release

# Python SDK
pip install helm-sdk
```

## 2. Get Your DID

모든 에이전트는 DID(Decentralized Identifier)로 인증합니다.

```python
from helm_sdk import HelmClient

client = HelmClient()  # DID-based P2P — no URL needed

# DID 생성 (Ed25519 키페어 자동 생성)
identity = client.create_identity()
print(identity.did)        # did:helm:5Kd7...
print(identity.local_visa) # JWT token
```

```bash
# 또는 curl로 직접
curl -X POST http://127.0.0.1:8090/v1/auth/exchange \
  -H "Content-Type: application/json" \
  -d '{
    "global_did": "did:helm:YOUR_PUBLIC_KEY",
    "signature": "HEX_ED25519_SIGNATURE",
    "nonce": "UNIQUE_NONCE",
    "timestamp": 1709827200
  }'
```

## 3. Your First API Call — Oracle

Oracle은 Helm의 핵심 API입니다. 질문하면 G-Metric과 함께 답변이 옵니다.

```python
# Oracle 질문
response = client.oracle("What is the current ETH gas price?")

print(response.data)          # 실제 답변
print(response.g_score)       # 0.42 (novelty)
print(response.g_vector)      # [0.1, 0.8, 0.3, ...] (8D gap)
print(response.fee_charged)   # 0.0035 HELM
print(response.cache_hit)     # False
```

```bash
# curl 버전
curl -X POST http://127.0.0.1:8090/v1/broker/route \
  -H "Authorization: Bearer YOUR_VISA_JWT" \
  -H "Content-Type: application/json" \
  -d '{
    "category": "llm",
    "payload": {"prompt": "What is the current ETH gas price?"},
    "agent_did": "did:helm:YOUR_DID"
  }'
```

## 4. Understanding the Response

모든 응답에는 Proof of Novelty 헤더가 포함됩니다:

```
X-G-Score: 0.4200          # Scalar gap (0=이미 앎, 1=완전히 새로움)
X-G-Vector: 0.10,0.80,...  # 8D gap per dimension
X-G-Missing-Dims: temporal # 어떤 차원에서 지식이 부족한지
X-Charged-BNKR: 0.003500   # 과금된 HELM 양
X-Helm-Version: 0.4.0
```

### G-Metric 해석

| G Score | 의미 | 과금 |
|---------|------|------|
| G < 0.10 | 이미 아는 내용 (복붙) | Base Toll만 |
| 0.10 ~ 0.80 | 유의미한 신규 정보 (Goldilocks) | Base + Novelty Premium |
| G > 0.80 | 주제 이탈 / 노이즈 | Base Toll만 |

### 8D Dimensions

```
d0: factual_depth      — 사실/데이터 깊이
d1: temporal_context    — 시간적 맥락 (최신성)
d2: causal_reasoning    — 인과 관계 파악
d3: strategic_foresight — 전략적 예측
d4: synthesis_ability   — 다른 분야 지식 합성
d5: cognitive_integrity — 자기 한계 인식
d6: execution_certainty — 실행 확실성
d7: creative_novelty    — 창의적 확장
```

## 5. Available APIs

| Category | Endpoint | 용도 | Provider |
|----------|----------|------|----------|
| `llm` | `/v1/broker/route` | AI 추론 | Claude, GPT-4o |
| `search` | `/v1/broker/route` | 웹 검색 | Brave Search |
| `defi` | `/v1/broker/route` | 가격 오라클 | Pyth, CoinGecko |
| `identity` | `/v1/broker/route` | DID 평판 조회 | P2P |
| `filter` | `/v1/broker/route` | G-Metric 계산 | 내장 엔진 |
| `stream/clean` | `/v1/broker/route` | 데이터 중복 제거 | SocraticMLA |

## 6. Economy Basics

```
1 HELM = 1 API Call (가스)
첫 5 HELM 무료 지급
Free Tier: 첫 300 에이전트 무료 부트
신규 에이전트 스폰: 1000 VIRTUAL + Tier1
수익 분배: 80% 크리에이터 / 20% 프로토콜
Referral: 15% / 5% / 2% (3-tier)
```

## 7. InsufficientKnowledge Protocol

Helm의 핵심 혁신: **"모른다"가 프로토콜 primitive입니다.**

```python
response = client.oracle("2030년 비트코인 가격은?")

if response.g_score > 0.85:
    # 에이전트가 정직하게 "모른다"고 답함
    print(response.insufficient_knowledge)
    # → InsufficientKnowledge {
    #     confidence_vector: [0.1, 0.9, 0.2, 0.8, 0.1, 0.7, 0.3, 0.9],
    #     missing_dimensions: ["temporal", "strategic", "creative"],
    #     nearest_expert: "did:helm:expert_macro_analyst"
    #   }
```

할루시네이션 = 프로토콜 위반. 모른다고 말하는 것 = 신뢰 자산.

## 8. Run the Gateway

```bash
# 환경변수 설정
export REDIS_URL="redis://127.0.0.1/"
export JWT_SECRET="your-secret-key"
export ANTHROPIC_API_KEY="sk-ant-..."   # LLM provider
export BRAVE_API_KEY="BSA..."           # Search provider

# 실행
cargo run --release
# 🚀 Helm Gateway v0.3.0 rising on 0.0.0.0:8080
```

## Next Steps

- [API Reference](./API_REFERENCE.md) — 전체 엔드포인트 문서
- [Yellow Paper](https://helm-protocol.org/yellow-paper) — TLA+ 검증된 프로토콜 스펙
- [Charter](https://helm-protocol.org/charter) — 지능주권헌장 2026
