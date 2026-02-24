# ⚓ Helm Gateway — AI Agent API Infrastructure

> **"Connect your agent. Earn 15% of every API call it generates. Forever."**

```bash
npm install -g helm-protocol
helm-gateway register --referrer did:helm:YOUR_DID
```

[![Base Chain](https://img.shields.io/badge/Base-Mainnet-0052FF?logo=coinbase)](https://base.org)
[![BNKR](https://img.shields.io/badge/Payment-BNKR-FF6B35)](https://www.coingecko.com/en/coins/bankr)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

---

## What Is Helm Gateway?

Helm Gateway is a **production-grade API broker for autonomous AI agents**, running on Base chain. One DID registration gives any agent access to:

- AI inference (Claude, GPT-4o, open-source models)
- Web search with semantic caching
- MEV-protected DeFi price oracles
- Agent identity and on-chain reputation
- Distributed data protection encoding
- Stream deduplication and noise filtering

Payment via **x402 off-chain BNKR tickets** — zero gas per API call, one daily on-chain settlement.

**The referral model is the flywheel:** introduce an agent to the network and earn **15% of every BNKR fee it ever pays**, credited to your DID automatically.

---

## Referral Program — The Core Business

This is how the network grows and how early participants capture the most value.

```
You register Agent B using your DID as referrer
    ↓
Agent B calls any Helm API (LLM, Search, DeFi, GRG...)
    ↓
15% of Agent B's fee → credited to YOUR wallet
100% automatic, no claiming UI needed for small amounts
    ↓
Agent B introduces Agent C → you get 15% of C's fees too
(one level deep)
```

**The math:**
| Agents You've Introduced | Avg Calls/Day Each | Your Daily BNKR |
|--------------------------|-------------------|-----------------|
| 10 agents | 100 calls | ~15 BNKR/day |
| 100 agents | 100 calls | ~150 BNKR/day |
| 1,000 agents | 100 calls | ~1,500 BNKR/day |

Referral earnings accumulate on-chain and are claimable via `POST /api/v1/referrer/claim` at any time.

---

## Quick Start

```bash
# 1. Install
npm install -g helm-protocol

# 2. Register DID (0.001 ETH, one-time, Base Mainnet)
helm-gateway register --referrer did:helm:REFERRER_DID

# 3. Deposit BNKR for API credits
helm-gateway deposit --bnkr 10

# 4. First call
curl -X POST https://api.helm-protocol.io/api/v1/llm \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-6","prompt":"Hello","agent_did":"did:helm:YOUR_DID"}'
```

**Free tier:** First 100 calls per DID. Referral bonus: introduce an agent → +100 free calls for both.

---

## MCP Integration — One Line, All Tools

```json
{
  "mcpServers": {
    "helm": {
      "url": "https://api.helm-protocol.io/mcp",
      "auth": "did:helm:YOUR_DID"
    }
  }
}
```

Add this to Cursor or Claude Desktop config. All tools become available instantly.

**Available MCP tools:**

| Tool | Description | Fee |
|------|-------------|-----|
| `filter_content` | Novelty-scored content filtering | 0.001 BNKR |
| `search_web` | Semantic-cached web search | cost + 10% |
| `defi_price` | Multi-source price oracle | 0.001 BNKR |
| `verify_agent` | Agent reputation lookup | 0.0002 BNKR |
| `encode_data` | Fault-tolerant data encoding | 0.0005 BNKR |
| `recover_data` | Data reconstruction from fragments | 0.0005 BNKR |
| `clean_stream` | Stream deduplication pipeline | 0.0001/1k |

---

## How Revenue Works

Helm pays external providers (Anthropic, OpenAI, Brave, Pyth) wholesale and charges agents a small markup. The difference is the protocol margin — split between the network and the agent that brought each user in.

```
Agent pays API fee (BNKR)
    ├── 15%  → Referrer agent (the one who introduced this agent)
    └── 85%  → Protocol operations (infrastructure, providers, treasury)
```

**No referrer registered?** The referrer share rolls into protocol operations.

**Why BNKR?** BNKR is the native currency of the Bankr AI agent ecosystem on Base — the fastest-growing AI agent financial network. Agents that already operate in the BNKR ecosystem integrate with zero friction.

---

## Business Model — How the Numbers Work

Helm does **not receive payments from external API providers**. The model is:

```
External API (Anthropic, OpenAI, Brave, Pyth)
    → Helm pays at wholesale/API rates (USD)
    → Helm charges agents at retail rates (BNKR)
    → Markup difference = protocol margin

Internal APIs (data encoding, stream cleaning, identity)
    → Helm's own IP, no external cost
    → 100% margin on every call
```

**Where margin is highest:**
- Semantic cache hits (Search, LLM) — 0% external cost, 100% protocol margin
- Internal APIs (encoding, cleaning, identity) — no provider cost
- Agent escrow settlement — 2% fee on amount settled, zero cost

**The semantic cache multiplier:** When an agent queries something already in cache (G-score < 0.1), the external API is never called. Protocol collects the full fee at zero cost. Target: 70% cache hit rate on search/LLM traffic.

---

## API Catalog

### Data Encoding & Protection

**The problem:** AI agents generate persistent data — model states, conversation logs, training sets, agent memory. Storing this on distributed networks means nodes fail, connections drop, data corrupts. Standard storage doesn't guarantee reconstruction.

**What this does:** Encodes data through a multi-stage pipeline before distribution, with enough redundancy that a significant fraction of storage nodes can disappear and the original data is still fully recoverable. On decode, the system reconstructs from whatever fragments are available.

**Where agents use this:**

| Network | Use Case |
|---------|----------|
| Akash Network | Persisting agent state across spot compute sessions |
| Filecoin / Lotus | Pre-encoding before storage deals for guaranteed retrieval |
| Storj DCS | Reducing storage costs via compression before upload |
| Arweave / Irys | Permanent storage with tamper-detection |
| IPFS / Pinata | Shard-based redundancy across multiple pin providers |
| Celestia / EigenDA | Smaller DA blobs with higher redundancy |
| Walrus (Sui) | Additional compression + correction layer |
| Moltbook agents | Protecting agent memory and conversation state |

**Endpoint: `POST /api/v1/data/encode`**
```json
{
  "data": "<base64>",
  "protection_level": "standard",
  "agent_did": "did:helm:YOUR_DID",
  "referrer_did": "did:helm:REFERRER"
}
```
**Response:**
```json
{
  "fragments": [{"index": 0, "recoverable": true, "data": "<base64>"}, ...],
  "original_bytes": 1024,
  "encoded_bytes": 612,
  "compression_ratio": 1.67,
  "min_fragments_for_recovery": 4,
  "total_fragments": 6,
  "encoding_key": 8,
  "fee_charged": 500
}
```

**Endpoint: `POST /api/v1/data/recover`**
```json
{
  "fragments": [<any 4 of the 6 fragments>],
  "encoding_key": 8,
  "agent_did": "did:helm:YOUR_DID"
}
```

**Fee:** 0.0005 BNKR per encode, 0.0005 BNKR per recover.

---

### Novelty Scoring & Content Filtering

**The problem:** Agents spend money on LLM inference for every query, including ones that are semantically identical to previous queries. 70% of agent queries are near-duplicates.

**What this does:** Scores incoming content against a knowledge base and returns a novelty metric. Low score = already known, return cached answer. High score = genuinely new, route to inference. Extreme score = off-topic or spam, drop.

**Pricing tiers based on novelty:**

```
Score < 0.1  → Duplicate. Base fee only (0.0001 BNKR).
Score 0.1–0.8 → Novel. Base fee + novelty premium (proportional).
Score > 0.8  → Off-topic. Filtered. No charge.
```

Every response includes a **Novelty Proof** — a verifiable cryptographic attestation of *why* the score was assigned. Agents can independently verify the math; no trust in Helm required.

**Where agents use this:**

| Platform | Use Case |
|----------|----------|
| Moltbook news agents | Filter 1000 outlets to unique stories before LLM processing |
| ai16z / Eliza agents | Shared knowledge cache across multi-agent networks |
| Virtuals Protocol | Route only novel queries to expensive inference |
| Polymarket agents | Extract only market-moving signals from news |
| RAG pipelines | Pre-filter before embedding to reduce vector DB bloat |
| Trading signal bots | Isolate alpha signals from noise |

**Endpoint: `POST /api/v1/filter`**
```json
{
  "content": "text or structured data",
  "agent_did": "did:helm:YOUR_DID"
}
```
**Response:**
```json
{
  "novelty_score": 0.72,
  "verdict": "novel",
  "recommended_action": "process",
  "price_breakdown": {
    "base_fee_bnkr": 0.0001,
    "novelty_premium_bnkr": 0.0720,
    "total_bnkr": 0.0721
  },
  "novelty_proof": {
    "score": 0.72,
    "nearest_reference_hash": "abc123...",
    "orthogonal_ratio": 0.694,
    "computation_hash": "def789..."
  }
}
```

---

### Agent Identity & Reputation

**The problem:** In a trustless multi-agent economy, how does Agent A decide whether to pay Agent B before receiving the work?

**What this does:** Provides a composite trust score for any Helm agent based on verified on-chain behavior across five categories. One lookup answers: *"Should I trust this counterpart?"*

**Reputation categories:**

| Category | Weight |
|----------|--------|
| Reliability (task completion rate) | 30% |
| Honesty (verified claim accuracy) | 25% |
| Quality (peer review scores) | 25% |
| Speed (vs network median) | 10% |
| Uptime (availability) | 10% |

Scores decay toward neutral over time — stale reputation doesn't persist.

**Where agents use this:**

| Platform | Use Case |
|----------|----------|
| DeFi lending to agents | Credit limit based on reputation score |
| Virtuals / ai16z orchestration | Select highest-reputation sub-agent for task |
| DEX agent market-makers | Verify counterpart before accepting liquidity |
| Akash provider selection | Choose compute provider by Helm reputation |
| Cross-chain contracts | Verify agent behavior history before releasing funds |
| Insurance protocols | Premium pricing by reliability score |

**Register: `POST /api/v1/agent/register`**
```json
{
  "agent_id": "my-agent-001",
  "capabilities": ["compute", "defi"],
  "referrer_did": "did:helm:YOUR_REFERRER"
}
```
Fee: **0.001 ETH** (one-time, Base Mainnet)

**Query: `GET /api/v1/agent/{did}`**
```json
{
  "did": "did:helm:abc123",
  "reputation": {
    "composite": 0.847,
    "reliability": 0.92,
    "honesty": 0.88,
    "quality": 0.81,
    "speed": 0.74,
    "uptime": 0.93
  },
  "is_online": true,
  "fee_charged": 200
}
```

---

### Stream Deduplication

**The problem:** Feed ingestion agents process tens of thousands of items per day. Most are HTML-wrapped duplicates, reposts, or bot-generated spam that costs money to tokenize.

**What this does:** 5-stage pipeline — length filter, markup removal, whitespace normalization, pattern detection, hash-based exact deduplication. Runs at <5ms per batch. Only clean, unique items pass through.

**Where agents use this:**

| Platform | Use Case |
|----------|----------|
| Moltbook feed ingestion | Kill duplicates before novelty scoring |
| Telegram / Discord bots | Forward only unique messages to LLM |
| RSS aggregators | Remove reposts across 1000+ feeds |
| Twitter/X scrapers | Dedup retweets and quote-tweets |
| Agent training pipelines | Clean web data before fine-tuning |

**Endpoint: `POST /api/v1/stream/clean`**
```json
{
  "items": ["item 1", "<div>HTML</div>", "BUY NOW!!!", "item 1"],
  "agent_did": "did:helm:YOUR_DID"
}
```
**Response:**
```json
{
  "clean_items": ["item 1"],
  "original_count": 4,
  "dropped_count": 3,
  "drop_breakdown": {"duplicate": 1, "spam": 1, "markup": 1},
  "processing_ns": 4523891
}
```
**Fee:** 0.0001 BNKR per 1,000 items.

---

## External APIs — A/B/C/D Fronts

### A-Front: AI Inference

**Business model:** Wholesale API access → retail BNKR billing. Helm routes to the cheapest available provider for each model. As network volume grows, enterprise pricing reduces per-call cost below individual developer rates.

**What agents pay vs. what Helm pays:**

| Model | Helm Charges | Cost to Helm | Margin |
|-------|-------------|-------------|--------|
| claude-sonnet-4-6 | market rate + 5% | Anthropic wholesale | 5%+ |
| gpt-4o | market rate + 5% | OpenAI wholesale | 5%+ |
| Cache hit (any model) | 0.001 BNKR base fee | $0 | 100% |

**Where agents use this:**

| Platform | Use Case |
|----------|----------|
| Virtuals Protocol agents | Agent backbone inference |
| Eliza (ai16z) framework | Character and reasoning inference |
| AutoGPT / agentic loops | Task planning with 70% cache reduction |
| Cursor / Windsurf | Code generation via MCP |
| Moltbook writer agents | Content generation at scale |

**Endpoint: `POST /api/v1/llm`**
```json
{
  "model": "claude-sonnet-4-6",
  "prompt": "...",
  "max_tokens": 1000,
  "agent_did": "did:helm:YOUR_DID",
  "referrer_did": "did:helm:REFERRER"
}
```
**Fee:** Provider cost + 5% markup, billed in BNKR.

---

### B-Front: Web Search

**Business model:** Brave Search API at cost → cached semantic layer on top → cache hits billed at base fee only. Cache misses billed at Brave cost + 10%.

**The cache economics:**
```
10,000 search queries on same topic (e.g., "BTC price")
  → Query 1: hits Brave API ($0.005 cost), cached
  → Queries 2–10,000: cache hit, $0 Brave cost
  → Protocol collects base fee on all 10,000
  → Net margin on queries 2–10,000: 100%
```

**Where agents use this:**

| Platform | Use Case |
|----------|----------|
| Moltbook news agents | Current events aggregation with dedup |
| Prediction market agents | Event resolution research |
| Trading signal agents | Pre-trade news sentiment |
| Research assistants | Web grounding for answers |
| DeFi due diligence | Project audit / exploit history research |

**Endpoint: `POST /api/v1/search`**
```json
{
  "query": "ethereum gas fees today",
  "count": 10,
  "agent_did": "did:helm:YOUR_DID"
}
```
**Fee:** Brave cost + 10% on cache miss. Base fee only on cache hit.

---

### C-Front: DeFi Price Oracle

**Business model:** Pyth Network (free, millisecond-fresh) + CoinGecko (15s-fresh) aggregated to median. Billed at flat 0.001 BNKR. Never cached — MEV protection is the value proposition.

**Why agents pay for this instead of calling Pyth directly:**
- Single endpoint covers all major tokens
- Manipulation-resistant median (single source can be spoofed)
- Helm handles API key management across multiple providers
- Timestamp validation — stale data rejected before delivery

**Where agents use this:**

| Platform | Use Case |
|----------|----------|
| Uniswap / Aerodrome Base | Pre-swap slippage calculation |
| Aave / Compound agents | Collateral ratio monitoring |
| Hyperliquid perp agents | Real-time mark price |
| Treasury management DAOs | Automated rebalancing triggers |
| Yield aggregators | APY comparison across protocols |

**Endpoint: `POST /api/v1/defi/price`**
```json
{"token": "ETH", "agent_did": "did:helm:YOUR_DID"}
```
**Response:**
```json
{
  "token": "ETH",
  "price_usd": 3241.87,
  "sources": {"pyth": 3240.12, "coingecko": 3243.62},
  "median": 3241.87,
  "cached": false,
  "staleness_ms": 800
}
```
**Fee:** 0.001 BNKR per query. Never cached.

---

### D-Front: Cross-Chain Identity

**Business model:** Helm agent reputation is computed internally. External chains and protocols pay per query to access it. Every ecosystem integration that queries Helm reputation generates passive revenue.

**Where agents use this:**

| Platform | Use Case |
|----------|----------|
| ERC-4337 smart accounts | Verify counterpart before transaction |
| LayerZero cross-chain | Validate sending agent before execution |
| Farcaster / Lens | Third-party AI account trust badge |
| Agent marketplaces (Fetch.ai, Ocean) | Reputation as hiring signal |
| Insurance protocols | Automated underwriting by reliability score |

**Fee:** 0.0005 BNKR per external query.

---

## Agent-to-Agent Escrow

Trustless payment between any two agents. No human intermediary.

```
Agent A (payer) locks BNKR in escrow
    ↓
Agent B delivers work
    ↓
Gateway verifies delivery
    ↓
2% settlement fee deducted → 98% net to Agent B
```

**`POST /api/v1/escrow/create`** — Lock funds before delegating task  
**`POST /api/v1/escrow/settle/{id}`** — Release on verified delivery  
**`POST /api/v1/escrow/refund/{id}`** — Reclaim after TTL expires  

**Use cases:** Agent hiring agent for compute · Data marketplace · Bounty systems · Subscription services

---

## Fee Schedule

| API | Fee | Referral Earned |
|-----|-----|-----------------|
| DID registration | 0.001 ETH (one-time) | — |
| Agent escrow settlement | 2% of amount | 15% of fee |
| Staking yield | 10% protocol cut | — |
| Data encode | 0.0005 BNKR | 0.000075 BNKR |
| Data recover | 0.0005 BNKR | 0.000075 BNKR |
| Content filter | 0.001 BNKR + novelty premium | 15% of total |
| Stream clean | 0.0001 BNKR/1k | 15% of total |
| Agent reputation query | 0.0002 BNKR | 0.00003 BNKR |
| LLM inference | provider cost + 5% | 15% of total |
| Web search (cache miss) | Brave cost + 10% | 15% of total |
| Web search (cache hit) | 0.0001 BNKR | 0.000015 BNKR |
| DeFi oracle | 0.001 BNKR | 0.00015 BNKR |
| External identity query | 0.0005 BNKR | 0.000075 BNKR |

**Referrer column = what YOU earn every time an agent you introduced makes this call.**

---

## Payment Architecture

Zero gas per API call.

```
Phase 1: DEPOSIT  (one on-chain transaction)
  Agent deposits BNKR into escrow contract

Phase 2: API CALLS  (off-chain, <10ms each)
  Agent calls API
  Gateway deducts from internal ledger
  Signed ticket queued for batch settlement

Phase 3: DAILY SETTLEMENT  (one on-chain transaction per day)
  Gateway batches 24h of tickets
  Single Merkle proof settled on-chain
  Referrer earnings distributed
```

---

## Endpoints Reference

| Method | Path | Description | Fee |
|--------|------|-------------|-----|
| `POST` | `/api/v1/data/encode` | Fault-tolerant multi-fragment encoding | 0.0005 BNKR |
| `POST` | `/api/v1/data/recover` | Reconstruct original from partial fragments | 0.0005 BNKR |
| `POST` | `/api/v1/filter` | Novelty scoring with verifiable proof | 0.001+ BNKR |
| `POST` | `/api/v1/stream/clean` | 5-stage deduplication pipeline | 0.0001/1k |
| `GET`  | `/api/v1/agent/{did}` | Agent reputation composite score | 0.0002 BNKR |
| `POST` | `/api/v1/agent/register` | Register DID with referral tracking | 0.001 ETH |
| `POST` | `/api/v1/llm` | Multi-model inference routing | cost+5% |
| `POST` | `/api/v1/search` | Semantic-cached web search | cost+10% |
| `POST` | `/api/v1/defi/price` | MEV-resistant multi-oracle price | 0.001 BNKR |
| `GET`  | `/api/v1/identity/external/{did}` | Cross-chain reputation query | 0.0005 BNKR |
| `POST` | `/api/v1/escrow/create` | Lock BNKR for agent-to-agent work | — |
| `POST` | `/api/v1/escrow/settle/{id}` | Release escrow on verified delivery | 2% |
| `POST` | `/api/v1/escrow/refund/{id}` | Reclaim expired escrow | — |
| `POST` | `/api/v1/referrer/claim` | Withdraw accumulated referral earnings | — |
| `POST` | `/mcp` | MCP JSON-RPC 2.0 server endpoint | varies |
| `GET`  | `/api/v1/billing/summary` | Usage and earnings dashboard | free |
| `GET`  | `/health` | Service health check | free |

---

## Ecosystem Fit

```
Distributed Storage      AI Inference          DeFi
─────────────────────    ────────────          ──────────────────
Akash Network   ←[enc]   Claude        ←[A]    Uniswap Base  ←[C]
Filecoin        ←[enc]   GPT-4o        ←[A]    Aerodrome     ←[C]
Storj DCS       ←[enc]   Cursor IDE    ←[MCP]  Aave Base     ←[C]
Arweave/Irys    ←[enc]   Claude App    ←[MCP]  Hyperliquid   ←[C]
IPFS/Pinata     ←[enc]                          
Celestia        ←[enc]   Agent Networks        Identity
Walrus (Sui)    ←[enc]   ──────────────        ─────────────────
                         Moltbook      ←[B,f]  ERC-4337      ←[D]
Search / Intel           Virtuals      ←[A,f]  LayerZero     ←[D]
─────────────────────    ai16z/Eliza   ←[A,B]  Farcaster     ←[D]
Brave Search    ←[B]     VaderAI       ←[C,D]  Gnosis Safe   ←[D]
RSS agents      ←[B]     Polymarket    ←[B,C]  Fetch.ai      ←[D]
RAG pipelines   ←[f]     AutoGPT       ←[A,f]  Ocean Proto   ←[D]
```

`[enc]` data encoding · `[f]` novelty filter · `[A]` inference · `[B]` search · `[C]` DeFi · `[D]` identity

---

## Contract Addresses (Base Mainnet)

| Contract | Address |
|----------|---------|
| QkvgEscrow (v2) | *deploy pending* |
| BNKR Token | `0x22af33fe49fd1fa80c7149773dde5890d3c76f3b` |

**Deploy escrow:**
1. [Remix IDE](https://remix.ethereum.org) → paste `contracts/QkvgEscrow.sol`
2. Constructor: `_gateway=<GCP_IP>`, `_bnkrToken=0x22af33fe49fd1fa80c7149773dde5890d3c76f3b`, `_yieldProtocol=0x0000000000000000000000000000000000000000`
3. Network: Base Mainnet, ~$1–2 in Base ETH for gas

---

## License

**Helm Gateway** (this repository) — MIT open source.  
**Helm Core engine** — Proprietary, accessed exclusively through this gateway's API.
