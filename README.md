# ⚓ Helm Gateway — The AI Agent Economy Tollgate

> **"Every piece of data an agent touches. Every API call it makes. Every truth it needs to verify. All flow through here."**

`npm install -g helm-protocol` · `helm-gateway --did your-did`

---

## What Is This?

Helm Gateway is a **production-grade API broker** for autonomous AI agents. It sits between agents and the world, providing:

- **Data protection** via GRG (Golomb → RedStuff → Golay pipeline)
- **Semantic intelligence** via QKV-G attention (Gap-aware routing)
- **Live market data** with MEV-proof oracle aggregation
- **Agent identity and trust** via DID + on-chain reputation
- **AI inference, search, and DeFi** brokered at competitive margins

Payment is handled via the **x402 off-chain ticket protocol** with BNKR tokens. No gas per call. Daily batch settlement on Base chain.

**The core repo (`Helm-Protocol/Helm`) is closed source.** The gateway is your only access point to this infrastructure.

---

## Revenue Model — How It Works

Every API call that flows through this gateway generates revenue:

| Source | Fee | Treasury Cut | Referrer Cut |
|--------|-----|--------------|--------------|
| GRG encode/decode | 0.0005 BNKR/call | **85%** | 15% |
| QKV-G attention | 0.001 BNKR/call | **85%** | 15% |
| Agent reputation query | 0.0002 BNKR/call | **85%** | 15% |
| Stream cleaning (Sync-O) | 0.0001 BNKR/1k items | **85%** | 15% |
| DID registration | 0.001 ETH flat | **100%** | — |
| Agent-to-agent escrow | 2% of settled amount | **100%** | — |
| Staking yield cut | 10% of epoch yield | **100%** | — |
| A-Front: LLM proxy | cost + 5% markup | **85%** | 15% |
| B-Front: Search proxy | cost + 10% markup | **85%** | 15% |
| C-Front: DeFi oracle | 0.1% of swap value | **85%** | 15% |
| D-Front: Identity ext. | 0.0005 BNKR/query | **85%** | 15% |

> **Bring an agent. Keep 15% of everything they ever spend here.**  
> Register your DID as their referrer once. Earn forever.

All treasury revenue flows to: `0x7e0118A33202c03949167853b05631baC0fA9756`

---

## Quick Start — 5 Minutes to First Call

```bash
# 1. Install
npm install -g helm-protocol

# 2. Register your DID (0.001 ETH, one-time)
helm-gateway register --referrer [your-referrer-did]

# 3. Deposit BNKR for API credits
helm-gateway deposit --bnkr 10

# 4. Make your first call
curl -X POST https://api.helm-protocol.io/api/v1/grg/encode \
  -H "Content-Type: application/json" \
  -d '{
    "data": "SGVsbG8gSGVsbQ==",
    "mode": "safety",
    "agent_did": "did:helm:your-did",
    "referrer_did": "did:helm:your-referrer"
  }'
```

---

## Internal APIs — Core Infrastructure

### 🔴 GRG Data Protection Pipeline

**Value Proposition:** Your agent generates data. That data needs to survive node failures, network corruption, and Byzantine actors in a distributed system. GRG is a 3-layer pipeline that makes your data indestructible.

```
ENCODE: Raw Data
    → [Golomb-Rice compression]   — removes redundancy, optimal for sparse data
    → [RedStuff erasure coding]   — splits into N shards, any K reconstruct the original
    → [Golay(24,12) FEC]          — each shard can self-correct up to 3 bit errors
    → Distributed Shards          — store across nodes. Lose 50%. Still recover.

DECODE: Shards (subset)
    → [Golay correction]          — fix up to 3 bits per 24-bit word
    → [RedStuff reconstruction]   — rebuild from K of N shards
    → [Golomb decompression]      — restore original data
    → Original Data               ✓
```

**Three modes:**

| Mode | Latency | Protection | Parity |
|------|---------|------------|--------|
| `turbo` | ~0.5ms | Golay only | 0 parity |
| `safety` | ~1ms | Full pipeline | 2 of 6 shards |
| `rescue` | ~2ms | Maximum | 2 of 4 shards (50% loss tolerated) |

**Endpoint: `POST /api/v1/grg/encode`**
```json
{
  "data": "<base64 raw bytes>",
  "mode": "safety",
  "agent_did": "did:helm:abc123",
  "referrer_did": "did:helm:referrer"
}
```

**Response:**
```json
{
  "shards": [
    {"index": 0, "is_parity": false, "data": "<base64>", "golay_protected": true},
    {"index": 1, "is_parity": false, "data": "<base64>", "golay_protected": true},
    ...
  ],
  "original_bytes": 1024,
  "compressed_bytes": 680,
  "compression_ratio": 1.506,
  "min_shards_for_recovery": 4,
  "total_shards": 6,
  "golomb_m": 8,
  "fee_charged": 500
}
```

**Endpoint: `POST /api/v1/grg/decode`**
```json
{
  "shards": [<subset of received shards>],
  "mode": "safety",
  "golomb_m": 8,
  "agent_did": "did:helm:abc123"
}
```

**Fee:** 0.0005 BNKR/call (encode), 0.0005 BNKR/call (decode)

---

### 🟡 QKV-G Attention — Semantic Gap Detection

**Value Proposition:** Before your agent spends money calling an LLM, it needs to know: *"Do I already know this? Is this truly new information?"* QKV-G runs in <1ms and tells you exactly how orthogonal an incoming query is to your agent's existing knowledge base. Only pay for genuinely new knowledge.

**The Goldilocks Zone:**

```
G < 0.1  → DUPLICATE — you already know this. Return cached answer. Cost: 0.
0.1 ≤ G ≤ 0.8 → NOVEL — genuinely new information. Pay for it.
G > 0.8  → SPAM/OFF-TOPIC — unrelated to your knowledge cluster. Reject.
```

**Endpoint: `POST /api/v1/attention`**
```json
{
  "query_vector": [0.1, -0.4, 0.9, ...],
  "sequence_id": 42,
  "agent_did": "did:helm:abc123",
  "referrer_did": "did:helm:referrer"
}
```

**Response:**
```json
{
  "result": "gap_detected",
  "g_metric": 0.72,
  "interpretation": "NOVEL — this query contains 72% new information",
  "recommended_action": "accept_and_charge",
  "price_suggestion_bnkr": 0.072,
  "closest_context_block": 14,
  "fee_charged": 1000
}
```

**Fee:** 0.001 BNKR/call

---

### 🟢 Agent Identity — DID + Reputation

**Value Proposition:** In a trustless network, every agent needs to answer: *"Can I trust this counterpart?"* The Agent Spanner combines on-chain DID documents with a 5-category reputation score into a single sub-millisecond lookup. Think of it as Web3's FICO score for AI agents.

**Reputation Categories:**
- **Reliability** (30%) — task completion rate
- **Honesty** (25%) — verified truthfulness of claims
- **Quality** (25%) — output quality assessment
- **Speed** (10%) — response time vs peers
- **Uptime** (10%) — network availability

**Register your DID: `POST /api/v1/agent/register`**
```json
{
  "agent_id": "my-agent-001",
  "capabilities": ["compute", "storage", "socratic"],
  "referrer_did": "did:helm:referrer-who-gets-15-percent"
}
```
Fee: **0.001 ETH** (one-time, no gas per future call)

**Query reputation: `GET /api/v1/agent/{did}`**
```json
{
  "did": "did:helm:abc123",
  "agent_id": "my-agent-001",
  "reputation": {
    "composite_score": 0.847,
    "reliability": 0.92,
    "honesty": 0.88,
    "quality": 0.81,
    "speed": 0.74,
    "uptime": 0.93
  },
  "is_online": true,
  "capabilities": ["compute", "storage"],
  "fee_charged": 200
}
```

Fee: 0.0002 BNKR/query

---

### 🔵 Sync-O Stream Cleaner

**Value Proposition:** Your agent ingests 10,000 items/hour. 73% are duplicate, spam, or malformed. Sync-O runs a 5-stage pipeline in <5ms that strips garbage before it hits your LLM. Stop paying to tokenize noise.

**5-Stage Pipeline:**
1. Length check + long-string blocking (>5000 chars dropped)
2. HTML tag removal
3. Whitespace normalization
4. Spam pattern filter (hot-reloadable from OpenClaw)
5. XXH3 deduplication (sliding window, 50k items)

**Endpoint: `POST /api/v1/clean`**
```json
{
  "stream_data": ["raw text 1", "<div>HTML garbage</div>", "BUY CRYPTO NOW!!!", "raw text 1"],
  "agent_did": "did:helm:abc123"
}
```

**Response:**
```json
{
  "clean_data": ["raw text 1"],
  "original_count": 4,
  "dropped_count": 3,
  "processing_ns": 4523891
}
```

Fee: 0.0001 BNKR per 1,000 items

---

## External APIs — A/B/C/D Fronts

### A-Front: AI Inference (LLM Proxy)

**Value Proposition:** Get Claude and GPT-4o at **5% above cost** — no API key management, no rate limit tracking, no per-model billing headaches. One DID, one payment channel, access to all frontier LLMs. As Helm network volume scales, wholesale rates bring your cost below retail.

**Supported models:** `claude-sonnet-4-6`, `claude-opus-4-6`, `gpt-4o`, `gpt-4o-mini`

**Endpoint: `POST /api/v1/llm`**
```json
{
  "model": "claude-sonnet-4-6",
  "prompt": "Analyze this market data...",
  "max_tokens": 1000,
  "agent_did": "did:helm:abc123",
  "referrer_did": "did:helm:referrer"
}
```

**Pricing:** Provider cost + 5% markup, deducted from BNKR deposit

---

### B-Front: Search & Web Data

**Value Proposition:** Brave Search brokered through QKV-G semantic caching. When G < 0.1 (you already know this answer), we return the cached result — **zero external API cost, 100% margin, instant response**. On cache miss, we call Brave, cache the result for future agents, and charge 10% above cost.

**The math:** 70% cache hit rate target. On cache hits you pay nothing. On misses you pay 10% markup. Effective average markup across all queries approaches 3%.

**Endpoint: `POST /api/v1/search`**
```json
{
  "query": "ethereum gas fees today",
  "count": 10,
  "agent_did": "did:helm:abc123",
  "referrer_did": "did:helm:referrer"
}
```

---

### C-Front: DeFi & Price Oracle

**Value Proposition:** MEV-protected multi-oracle price feeds. We aggregate Pyth Network + CoinGecko, compute the median, and **never cache** price data — stale prices cause failed swaps and liquidations. Pay 0.1% of swap value for a verified, fresh price. No MEV bot can front-run you through our proxy.

**Supported:** ETH, BTC, SOL, USDC, BNKR

**Endpoint: `POST /api/v1/defi`**
```json
{
  "token": "ETH",
  "agent_did": "did:helm:abc123"
}
```

**Response:**
```json
{
  "token": "ETH",
  "price_usd": 3241.87,
  "sources": {"pyth": 3240.12, "coingecko": 3243.62},
  "median": 3241.87,
  "cached": false,
  "timestamp_ms": 1740400123456
}
```

Fee: 0.1% of swap value (minimum 0.001 BNKR)

---

### D-Front: Agent Identity Network

**Value Proposition:** The Web3 FICO score for AI agents. External projects querying Helm agent reputation pay per lookup. As Helm becomes the standard for agent credentialing in the Base ecosystem, this becomes passive income from every agent interaction in the network — whether they use Helm directly or not.

**Endpoint: `GET /api/v1/identity/external/{did}`**

Used by: DEX protocols verifying counterpart trustworthiness, lending protocols checking agent creditworthiness, any contract needing to verify agent reliability before sending funds.

Fee: 0.0005 BNKR/external query

---

## Agent Escrow (A2A Payments)

Two agents can transact trustlessly via the gateway:

```
Agent A (Payer) → locks BNKR in escrow
    ↓ Agent B (Payee) delivers work
    ↓ Gateway verifies delivery
    ↓ 2% fee → treasury | 98% → Agent B
```

**Create escrow: `POST /api/v1/escrow/create`**
```json
{
  "payee_did": "did:helm:worker-agent",
  "bnkr_amount": 10.0,
  "ttl_seconds": 86400,
  "payer_did": "did:helm:client-agent"
}
```

---

## Payment Architecture — x402 Off-Chain Tickets

No gas per call. Ever.

```
1. DEPOSIT (once, on-chain)
   Agent → QkvgEscrow.depositBnkr(amount)  [one gas tx]

2. API CALL (off-chain, instant)
   Agent calls API → Gateway deducts from internal ledger
   Off-chain ticket signed → queued for batch settlement

3. DAILY SETTLEMENT (on-chain)
   Gateway batches 24h of tickets → single Merkle proof → treasury
```

**Fee breakdown per API call:**
```
Total fee
  ├── 85% → 0x7e0118A33202c03949167853b05631baC0fA9756 (treasury)
  └── 15% → referrer agent DID wallet (claimable anytime)
```

---

## Contract Addresses (Base Mainnet)

| Contract | Address |
|----------|---------|
| QkvgEscrow | _deploy pending_ |
| BNKR Token | `0x22af33fe49fd1fa80c7149773dde5890d3c76f3b` |
| Helm Treasury | `0x7e0118A33202c03949167853b05631baC0fA9756` |

---

## Fee Summary Table

| Protocol Fee | Rate | Who Pays |
|-------------|------|----------|
| DID registration | 0.001 ETH | New agents (once) |
| Agent escrow settlement | 2% | Payer agent |
| Staking yield cut | 10% | Stakers |
| GRG encode/decode | 0.0005 BNKR | Calling agent |
| QKV-G attention | 0.001 BNKR | Calling agent |
| Reputation query | 0.0002 BNKR | Calling agent |
| Sync-O clean | 0.0001 BNKR/1k | Calling agent |
| LLM proxy (A-Front) | cost +5% | Calling agent |
| Search proxy (B-Front) | cost +10% | Calling agent |
| DeFi oracle (C-Front) | 0.1% swap | Calling agent |
| External identity (D-Front) | 0.0005 BNKR | External querier |

**Referrer earns 15% of all API fees for agents they introduced.**

---

## Architecture Diagram

```
┌──────────────────────────────────────────────────────────┐
│                    External Agents                        │
│         (Moltbook / Virtuals / ai16z / Custom)            │
└──────────────────┬───────────────────────────────────────┘
                   │ x402 off-chain tickets (BNKR)
┌──────────────────▼───────────────────────────────────────┐
│              Helm Gateway  [PUBLIC — this repo]           │
│                                                           │
│  ┌─────────────────────────────────────────────────────┐ │
│  │  QKV-G Semantic Cache  (70% hit rate target)        │ │
│  └─────────────────────┬───────────────────────────────┘ │
│                        │ cache miss                       │
│  ┌─────────┐  ┌────────▼──────┐  ┌──────────┐           │
│  │ GRG     │  │  Helm Core    │  │ External │           │
│  │ Engine  │  │  [CLOSED]     │  │  APIs    │           │
│  │ Encode/ │  │  QKV-G Attn   │  │ Anthropic│           │
│  │ Decode  │  │  Reputation   │  │ OpenAI   │           │
│  │         │  │  Womb/Mining  │  │ Brave    │           │
│  └────┬────┘  └──────┬────────┘  │ Pyth     │           │
│       │              │            │ CoinGecko│           │
│  ┌────▼──────────────▼────────────▼──────────────────┐  │
│  │              BillingLedger                         │  │
│  │   85% → Treasury  |  15% → Referrer Agent          │  │
│  └─────────────────────────────────────────────────── ┘  │
└──────────────────────────────────────────────────────────┘
                   │ daily Merkle batch
┌──────────────────▼───────────────────────────────────────┐
│         QkvgEscrow.sol (Base Mainnet)                     │
│         Treasury: 0x7e0118A33...                          │
└──────────────────────────────────────────────────────────┘
```

---

## Endpoints Reference

| Method | Path | Description | Fee |
|--------|------|-------------|-----|
| `POST` | `/api/v1/grg/encode` | GRG encode data | 0.0005 BNKR |
| `POST` | `/api/v1/grg/decode` | GRG decode shards | 0.0005 BNKR |
| `POST` | `/api/v1/attention` | QKV-G gap detection | 0.001 BNKR |
| `GET`  | `/api/v1/agent/{did}` | Agent reputation | 0.0002 BNKR |
| `POST` | `/api/v1/agent/register` | Register DID | 0.001 ETH |
| `POST` | `/api/v1/clean` | Sync-O stream clean | 0.0001/1k |
| `POST` | `/api/v1/llm` | LLM inference (A-Front) | cost+5% |
| `POST` | `/api/v1/search` | Web search (B-Front) | cost+10% |
| `POST` | `/api/v1/defi` | Price oracle (C-Front) | 0.1% |
| `GET`  | `/api/v1/identity/external/{did}` | Cross-chain identity | 0.0005 BNKR |
| `POST` | `/api/v1/escrow/create` | Create A2A escrow | — |
| `POST` | `/api/v1/escrow/settle/{id}` | Settle escrow | 2% |
| `GET`  | `/api/v1/billing/summary` | Billing stats | free |

---

## License

The **Helm Gateway** (this repository) is open source under MIT.  
The **Helm Core** engine (closed) is accessed only through this gateway's API.  
Core IP — QKV-G attention, GRG pipeline, Socratic Claw, Agent Womb — is proprietary.
