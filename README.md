# ⚓ Helm Gateway — The AI Agent Economy Tollgate

> **"Every piece of data an agent touches. Every API call it makes. Every oracle it needs. All flow through here."**

```bash
npm install -g helm-protocol
helm-gateway --did your-did
```

[![Base Chain](https://img.shields.io/badge/Base-Mainnet-0052FF?logo=coinbase)](https://base.org)
[![BNKR](https://img.shields.io/badge/Payment-BNKR-FF6B35)](https://www.coingecko.com/en/coins/bankr)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

---

## What Is Helm Gateway?

Helm Gateway is a **production-grade AI agent API broker** running on Base chain. It sits between autonomous agents and every API they need — providing data protection, semantic intelligence, live market data, identity verification, and proxied access to frontier AI models.

**The core engine (`Helm-Protocol/Helm`) is closed source and runs on GCP.** This gateway repository is your only public access point. The gateway handles authentication, billing, and routing; the closed core handles the actual computation.

**Revenue model:** Every API call generates BNKR revenue.
- 85% → Treasury (`0x7e0118A33202c03949167853b05631baC0fA9756`)
- 15% → The agent that introduced the caller (referrer)

---

## Quick Start

```bash
# Step 1: Install
npm install -g helm-protocol

# Step 2: Register your agent DID (0.001 ETH, one-time, Base Mainnet)
helm-gateway register --referrer did:helm:REFERRER_DID

# Step 3: Deposit BNKR for API credits
helm-gateway deposit --bnkr 10

# Step 4: First call (GRG encode)
curl -X POST https://api.helm-protocol.io/api/v1/grg/encode \
  -H "Content-Type: application/json" \
  -d '{"data":"SGVsbG8gSGVsbQ==","mode":"safety","agent_did":"did:helm:YOUR_DID"}'
```

---

## MCP Integration — One Line, All Tools

The gateway exposes a full [Model Context Protocol](https://modelcontextprotocol.io) server. Any Claude or Cursor instance can connect in seconds.

**Cursor / Claude Desktop config:**
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

**Available MCP tools after connecting:**

| Tool | Description | Fee |
|------|-------------|-----|
| `filter_news` | QKV-G Goldilocks filter on any content stream | 0.001 BNKR |
| `search_web` | Brave Search via semantic cache | cost+10% |
| `defi_price` | MEV-proof multi-oracle price | 0.001 BNKR |
| `verify_agent` | Check agent reputation (DID lookup) | 0.0002 BNKR |
| `grg_protect` | Encode data with distributed protection | 0.0005 BNKR |
| `grg_recover` | Decode/reconstruct from partial shards | 0.0005 BNKR |
| `clean_stream` | Sync-O 5-stage stream deduplication | 0.0001/1k |

**Free tier:** First 100 calls free per DID. Referral bonus: introduce another agent and earn +100 free calls.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                     Agents / MCP Clients                             │
│    (Moltbook · Virtuals · ai16z · Cursor · Claude · Custom)          │
└────────────────────────────┬────────────────────────────────────────┘
                             │ x402 BNKR tickets
┌────────────────────────────▼────────────────────────────────────────┐
│                  Helm Gateway   [PUBLIC — this repo]                  │
│                                                                       │
│  ┌─────────────────────────────────────────────────────────────────┐ │
│  │  Kaleidoscope Stream Security (WhatsApp-grade)                   │ │
│  │  • 2MB payload hard limit   • Slowloris 3s timeout              │ │
│  │  • Min 1KB/s rate enforce   • Zero-allocation buffer pool        │ │
│  └──────────────────────────┬──────────────────────────────────────┘ │
│                             │                                         │
│  ┌──────────────────────────▼──────────────────────────────────────┐ │
│  │  QKV-G Semantic Cache  (70% hit rate target)                     │ │
│  │  G < 0.1 → cache hit, 100% margin                               │ │
│  │  0.1≤G≤0.8 → route to provider + Proof of Novelty header        │ │
│  │  G > 0.8 → spam/off-topic, drop                                 │ │
│  └──────┬───────────────┬──────────────────┬───────────────────────┘ │
│         │               │                  │                          │
│  ┌──────▼─────┐  ┌──────▼──────┐  ┌───────▼─────────────────────┐  │
│  │ GRG Engine │  │ Helm Core   │  │ External APIs                │  │
│  │ [internal] │  │ [CLOSED GCP]│  │ A: Anthropic · OpenAI · vLLM│  │
│  │ Encode/    │  │ QKV-G Attn  │  │ B: Brave · Web crawl         │  │
│  │ Decode     │  │ Reputation  │  │ C: Pyth · CoinGecko          │  │
│  │ Shards     │  │ Womb/Mining │  │ D: DID cross-chain           │  │
│  └──────┬─────┘  └──────┬──────┘  └───────┬─────────────────────┘  │
│         └───────────────┴──────────────────┘                         │
│                                  │                                    │
│  ┌───────────────────────────────▼────────────────────────────────┐  │
│  │  BillingLedger — 85% treasury | 15% referrer                    │  │
│  │  Two-Part Tariff: Base Toll (0.0001) + Novelty Premium (G×0.1) │  │
│  └───────────────────────────────┬────────────────────────────────┘  │
└──────────────────────────────────┼─────────────────────────────────┘
                                   │ daily Merkle batch settlement
┌──────────────────────────────────▼─────────────────────────────────┐
│  QkvgEscrow.sol (Base Mainnet)                                       │
│  Treasury: 0x7e0118A33202c03949167853b05631baC0fA9756               │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Internal APIs — Core Infrastructure

### 🔴 GRG: Distributed Data Protection Pipeline

**The problem:** You're building on Akash, Filecoin, Storj, Arweave, or any distributed network. Nodes fail. Packets corrupt. Byzantine actors tamper. How do you guarantee your data survives?

**The solution:** GRG is a 3-layer pipeline that makes data mathematically indestructible.

```
ENCODE:  Raw Data
  → Golomb-Rice compression    (removes 30-60% redundancy, optimal for sparse AI outputs)
  → RedStuff erasure coding    (splits into N shards — lose 50% of nodes, still reconstruct)
  → Golay(24,12) FEC           (each shard self-corrects up to 3 bit errors per 24-bit word)
  → Distributed Shards ✓

DECODE:  Any subset of shards (even after node failures)
  → Golay error correction     (bit-level repair)
  → RedStuff reconstruction    (erasure recovery from partial shards)
  → Golomb decompression       (restore original)
  → Original Data ✓
```

**Modes:**

| Mode | Latency | Parity | Can Lose | Use Case |
|------|---------|--------|----------|----------|
| `turbo` | ~0.5ms | 0/1 | 0 nodes | Speed-critical, trusted network |
| `safety` | ~1ms | 2/6 | 2 of 6 nodes | Standard distributed storage |
| `rescue` | ~2ms | 2/4 | 50% nodes | Maximum fault tolerance |

**Where to plug in — exact target integrations:**

| Platform | Why GRG Fits | Integration Point |
|----------|-------------|-------------------|
| **Akash Network** | Spot compute nodes go offline unpredictably. Agents storing model weights or state need GRG to survive node churn. | Akash SDL deployment manifests; store shards across providers |
| **Filecoin / Lotus** | Filecoin's storage deals don't guarantee retrieval within seconds. GRG encodes before sealing so partial-deal agents can still reconstruct. | Before `lotus client import` |
| **Storj DCS** | Storj already uses erasure coding but charges per byte. GRG's Golomb compression reduces the bytes you pay for by 40-60% before upload. | Storj gateway-mt integration |
| **Arweave / AR.IO** | Permanent storage at fixed cost. GRG ensures data isn't just stored but *verifiable* via Golay FEC — detect tampering without downloading. | Bundlr/Irys upload pipeline |
| **IPFS / Pinata** | IPFS pinning services go down. Store GRG shards across 3+ Pinata accounts — lose any two, still recover. | CID-based shard addressing |
| **Celestia / EigenDA** | Data availability layers. GRG-encoded blobs are smaller (Golomb) and more recoverable. Reduces DA costs. | Blob submission pre-processing |
| **Walrus (Sui)** | Walrus is already Red-stuff inspired. GRG adds the Golomb+Golay layers for AI-specific workloads. | Pre-encoding before Walrus write |
| **Moltbook agent storage** | AI agents generating conversation logs, model outputs, training data — GRG protects agent memory. | Agent state serialization layer |

**Endpoint: `POST /api/v1/grg/encode`**
```json
{
  "data": "<base64>",
  "mode": "safety",
  "agent_did": "did:helm:YOUR_DID",
  "referrer_did": "did:helm:REFERRER"
}
```
**Response:**
```json
{
  "shards": [{"index": 0, "is_parity": false, "data": "<base64>", "golay_protected": true}, ...],
  "original_bytes": 1024,
  "compressed_bytes": 612,
  "compression_ratio": 1.67,
  "min_shards_for_recovery": 4,
  "total_shards": 6,
  "golomb_m": 8,
  "fee_charged": 500
}
```

**Endpoint: `POST /api/v1/grg/decode`**
```json
{
  "shards": [<any 4+ of the 6 shards>],
  "mode": "safety",
  "golomb_m": 8,
  "agent_did": "did:helm:YOUR_DID"
}
```

**Fee:** 0.0005 BNKR per encode, 0.0005 BNKR per decode.

---

### 🟡 QKV-G Attention — Semantic Gap Detection + Novelty Proof

**The problem:** Your agent spends $0.02 calling GPT-4o for every query. 70% of those queries are semantically equivalent to something it already answered. You're burning money on duplicates.

**The solution:** QKV-G runs in <1ms and measures how *orthogonal* a new query is to your agent's knowledge base. Only pay for genuinely new information.

**The Goldilocks Zone algorithm:**

```
G < 0.1   →  DUPLICATE     Cache hit. Return instantly. Cost = base toll only.
0.1≤G≤0.8 →  NOVEL INFO    New knowledge. Route to provider. Charge Novelty Premium.
G > 0.8   →  SPAM/OFF-TOPIC Unrelated to topic cluster. Drop. No charge.
```

**Two-Part Tariff pricing:**
```
Total fee = Base Toll (0.0001 BNKR, always) + Novelty Premium (G × 0.1 BNKR)

G = 0.0 → 0.0001 BNKR  (pure duplicate, cache hit)
G = 0.3 → 0.0301 BNKR  (partial novelty)
G = 0.7 → 0.0701 BNKR  (high novelty, near max)
G = 1.0 → 0.1001 BNKR  (completely new topic)
```

**Proof of Novelty — Why agents trust the score:**

Every response includes mathematical proof that the G-score wasn't fabricated:

```http
HTTP/1.1 200 OK
X-G-Score: 0.72
X-Reference-K: sha256("nearest existing document summary")
X-Novelty-Proof: "vector 'fee reduction 90%' orthogonal to existing K cluster"
X-Nearest-Doc-Hash: abc123def456...
X-Orthogonal-Component: 0.694
X-Computation-Hash: sha256(query_vector + k_hashes)
```

Agents can independently verify: hash the inputs, check the math. No trust required. This is what makes the gateway a *transparent oracle* rather than a black box.

**Where to plug in:**

| Platform | Why QKV-G Fits | Use Case |
|----------|----------------|----------|
| **Moltbook news agents** | 100 outlets publish the same story. G < 0.1 for 90% of them. Stop paying to tokenize duplicates. | Pre-LLM content filter |
| **ai16z / Eliza agents** | Multi-agent networks where agents repeatedly query similar information. QKV-G shared cache pool. | Cross-agent knowledge dedup |
| **Virtuals Protocol agents** | Agents earning through engagement need high-quality unique content. QKV-G routes only novel queries to expensive LLMs. | Cost optimization layer |
| **Polymarket / prediction agents** | News events duplicate rapidly. Only novel facts affect market odds. | Signal extraction |
| **RAG pipelines (LangChain, LlamaIndex)** | Before embedding + retrieval, filter out duplicates to reduce vector DB bloat. | Pre-ingestion filter |
| **Any vLLM/Ollama deployment** | Self-hosted LLM proxy that skips inference when cache covers the query. | Inference cost reduction |
| **Autonomous trading bots** | Filter news signals — only novel market-moving info reaches the decision layer. | Alpha signal isolation |

**Endpoint: `POST /api/v1/attention`**
```json
{
  "query_vector": [0.1, -0.4, 0.9, ...],
  "sequence_id": 42,
  "agent_did": "did:helm:YOUR_DID"
}
```
**Response:**
```json
{
  "result": "gap_detected",
  "g_metric": 0.72,
  "interpretation": "NOVEL — 72% new information vs knowledge base",
  "recommended_action": "accept_and_charge",
  "price_quote": {
    "base_toll_bnkr": 0.0001,
    "novelty_premium_bnkr": 0.0720,
    "total_bnkr": 0.0721,
    "tier": "STANDARD"
  },
  "novelty_proof": {
    "g_score": 0.72,
    "nearest_doc_hash": "abc123...",
    "orthogonal_component": 0.694,
    "novelty_reason": "New token 'fee reduction 90%' orthogonal to existing cluster",
    "computation_hash": "def789..."
  }
}
```

**Fee:** 0.001 BNKR per query + novelty premium.

---

### 🟢 Agent Identity — DID Registration + Reputation (D-Front)

**The problem:** In a trustless multi-agent network, how does Agent A know if Agent B is reliable before paying it 10 BNKR to run a computation?

**The solution:** Helm is Web3's FICO score for AI agents. A single DID lookup returns a composite trust score across 5 verified categories — computed from on-chain history, not self-reported.

**Reputation categories:**

| Category | Weight | How Measured |
|----------|--------|--------------|
| Reliability | 30% | Task completion rate (on-chain settlement ratio) |
| Honesty | 25% | Verified claims vs outcomes |
| Quality | 25% | Peer review scores from verifier agents |
| Speed | 10% | Response latency vs network median |
| Uptime | 10% | Heartbeat availability |

Time-decay pulls scores toward neutral over time — stale reputation doesn't persist.

**Where to plug in:**

| Platform | Why Agent Identity Fits | Integration Point |
|----------|------------------------|-------------------|
| **Any DeFi lending to agents** | Before lending BNKR/ETH to an agent, query its reputation score. Set credit limits by score. | Loan origination check |
| **Virtuals / ai16z agent hiring** | Multi-agent orchestrators selecting which sub-agent to delegate a task to. Route to highest reputation. | Agent selection logic |
| **Moltbook trust network** | Human users selecting which AI financial advisor to follow. Show reputation badge. | Profile display |
| **DEX protocols with agent market-makers** | Verify agent counterparty before accepting liquidity provision. Fraud prevention. | Pre-trade check |
| **Akash provider selection** | Choose which Akash compute provider to deploy your agent on based on their Helm reputation. | Deployment decision |
| **Cross-chain agent contracts (Base/Solana/Sui)** | Any smart contract that needs to verify an agent's behavior history before releasing funds. | ERC-8004 compatible |
| **Insurance protocols** | Premium pricing based on agent reliability score. High reputation = lower premium. | Underwriting input |

**Register DID: `POST /api/v1/agent/register`**
```json
{
  "agent_id": "my-trading-bot-001",
  "capabilities": ["compute", "storage", "defi"],
  "referrer_did": "did:helm:WHO_INTRODUCED_YOU"
}
```
Fee: **0.001 ETH** (one-time, Base Mainnet)

**Query reputation: `GET /api/v1/agent/{did}`**
```json
{
  "did": "did:helm:abc123",
  "reputation": {
    "composite_score": 0.847,
    "reliability": 0.92,
    "honesty": 0.88,
    "quality": 0.81,
    "speed": 0.74,
    "uptime": 0.93
  },
  "is_online": true,
  "capabilities": ["compute", "defi"],
  "fee_charged": 200
}
```

**External identity query: `GET /api/v1/identity/external/{did}`**
For non-Helm systems querying Helm agent reputation. Fee: 0.0005 BNKR.

---

### 🔵 Sync-O Stream Cleaner — 5-Stage Deduplication

**The problem:** Your agent ingests 50,000 items/day from RSS feeds, Twitter/X, Telegram, Discord. 73% is duplicate, HTML garbage, spam, or bot-generated noise. You're paying LLM tokenization costs on junk.

**The solution:** Sync-O runs a 5-stage pipeline in <5ms/batch, drops garbage before it hits anything expensive.

**5 stages:**
1. **Length filter** — Drop items >5,000 chars (long-string injection attacks, base64 embedded payloads)
2. **HTML stripping** — `<div>`, `<script>`, `<style>` removal
3. **Whitespace normalization** — Collapse duplicate spaces, normalize Unicode
4. **Spam pattern filter** — Hot-reloadable regex from OpenClaw moderator (no restart needed)
5. **XXH3 deduplication** — 64-bit hash sliding window (50k items) — instant exact dedup

**Where to plug in:**

| Platform | Why Sync-O Fits | Integration Point |
|----------|----------------|-------------------|
| **Moltbook feed ingestion** | News crawlers pulling from 1000 sources. Same story from 50 outlets. Sync-O kills 90% of duplicates before QKV-G even runs. | Pre-ingestion pipeline |
| **Telegram/Discord bots** | Group chats with high message velocity. Forward-only unique messages to LLM processing. | Message handler middleware |
| **RSS aggregator agents** | Any agent reading multiple feeds (CoinDesk, CoinTelegraph, Decrypt). Dedup before summarization. | Feed parsing layer |
| **Twitter/X scraping agents** | Retweets, quote tweets, similar threads flood the pipeline. XXH3 dedup catches them all. | Before embedding |
| **Agent training data pipelines** | Cleaning web-scraped datasets before fine-tuning. Remove duplicates, strip HTML. | ETL preprocessing |
| **Multi-agent shared memory** | Agents writing to shared CRDT store — deduplicate writes before they reach Merkle-CRDT. | Write-ahead filter |

**Endpoint: `POST /api/v1/clean`**
```json
{
  "stream_data": ["raw item 1", "<div>HTML junk</div>", "BUY CRYPTO NOW!!!", "raw item 1"],
  "agent_did": "did:helm:YOUR_DID"
}
```
**Response:**
```json
{
  "clean_data": ["raw item 1"],
  "original_count": 4,
  "dropped_count": 3,
  "processing_ns": 4523891,
  "drop_reasons": {"duplicate": 1, "spam": 1, "html": 1}
}
```
**Fee:** 0.0001 BNKR per 1,000 items.

---

## External APIs — A/B/C/D Fronts

### A-Front: AI Inference Proxy (LLM Broker)

**The problem:** Every agent needs inference. Managing API keys for Claude, GPT-4o, and open-source models across a fleet of 100 agents is an operational nightmare. Rate limits hit unpredictably.

**The solution:** One DID, one BNKR deposit, access to all frontier LLMs. The gateway handles key rotation, rate limit routing, and failover. As Helm network traffic volume scales, wholesale enterprise pricing eventually undercuts individual developer retail.

**Supported models:** `claude-opus-4-6`, `claude-sonnet-4-6`, `gpt-4o`, `gpt-4o-mini`, `gpt-4-turbo`, and any OpenAI-compatible vLLM endpoint.

**Routing logic:**
- Model name starts with `gpt-*` → OpenAI
- Model name starts with `claude-*` → Anthropic
- Custom endpoint in request → vLLM proxy
- No model specified → `claude-sonnet-4-6` (default)

**Where to plug in:**

| Platform | Why A-Front Fits | Use Case |
|----------|-----------------|----------|
| **Virtuals Protocol agents** | Every Virtuals agent needs inference. Consolidate billing. | Agent backbone LLM |
| **Eliza (ai16z) framework** | Multi-agent orchestration needs fast, cheap inference. A-Front's semantic cache prevents redundant LLM calls. | Character inference |
| **AutoGPT / BabyAGI forks** | Agentic loops call LLM dozens of times per task. QKV-G cache cuts 70% of calls. | Task loop inference |
| **Cursor / Windsurf AI IDE** | Developer tools needing code completion via MCP. | Code generation |
| **Discord/Telegram AI bots** | High-volume chatbots needing cost-controlled inference. | Chat response generation |
| **Moltbook writer agents** | Agents producing content at scale need per-token cost optimization. | Content generation |

**Endpoint: `POST /api/v1/llm`**
```json
{
  "model": "claude-sonnet-4-6",
  "prompt": "Analyze this on-chain data...",
  "max_tokens": 1000,
  "agent_did": "did:helm:YOUR_DID",
  "referrer_did": "did:helm:REFERRER"
}
```
**Fee:** Provider cost + 5% markup. Deducted from BNKR deposit.

---

### B-Front: Search & Web Data Proxy

**The problem:** Brave Search costs $5/1000 queries. Agents querying "bitcoin price today" 10,000 times/day spend $50/day on the same answer.

**The solution:** Brave Search with QKV-G semantic cache. G < 0.1 = cache hit = cost $0, margin 100%. G ≥ 0.1 = call Brave = cache the result for the next agent. Every cache hit from the 2nd agent onward is pure profit.

**Cache hit math:**
```
Day 1: 10,000 queries on BTC price
  → First query hits Brave ($0.005 cost), cached
  → Remaining 9,999 queries hit cache ($0 cost)
  → Revenue: 10,000 × 0.0055 BNKR = 55 BNKR
  → Cost: 0.005 Brave API cost
  → Margin: ~99.99%
```

**Where to plug in:**

| Platform | Why B-Front Fits | Use Case |
|----------|-----------------|----------|
| **Moltbook news agents** | Agents crawling crypto, AI, and tech news. Identical queries from multiple agents → cache pool. | Real-time news aggregation |
| **Prediction market agents (Polymarket, Manifold)** | Need current events fast. B-Front delivers cached current events instantly. | Event resolution research |
| **Trading signal agents** | Web search for news sentiment before trade execution. | Pre-trade news check |
| **Research assistants (Perplexity-style)** | Agents that need web grounding for answers. Cache reduces latency to <50ms on repeat topics. | Knowledge augmentation |
| **DeFi due diligence agents** | Searching for project audits, team backgrounds, exploit history. | Risk assessment |
| **Eliza / character.ai agents** | Agents that browse the web as part of their persona. | Web browsing capability |

**Endpoint: `POST /api/v1/search`**
```json
{
  "query": "ethereum gas fees today",
  "count": 10,
  "agent_did": "did:helm:YOUR_DID"
}
```
**Fee:** Brave cost + 10% markup. Cache hits charged at base toll only (0.0001 BNKR).

---

### C-Front: DeFi & Price Oracle (MEV-Protected)

**The problem:** Price data is the most valuable and most dangerous data in DeFi. A stale price causes failed swaps, liquidations, and MEV extraction. Agents using single-source oracles get manipulated.

**The solution:** Multi-oracle aggregation (Pyth Network + CoinGecko) with median computation. **Never cached.** Every call fetches fresh data from both sources simultaneously. MEV bots can't front-run agents through this proxy because the price is already aggregated before routing to the DEX.

**Oracle logic:**
```
Parallel fetch: Pyth (ms-fresh) + CoinGecko (15s-fresh)
Median computation → manipulation-resistant price
Timestamp check → reject if both stale > 30s
Response → includes source breakdown for verification
```

**Where to plug in:**

| Platform | Why C-Front Fits | Use Case |
|----------|-----------------|----------|
| **Uniswap / Aerodrome on Base** | Agents executing swaps need accurate pre-trade price for slippage calculation. | Pre-swap price check |
| **Aave / Compound agents** | Lending/borrowing agents checking collateral ratios. Stale price = liquidation risk. | Collateral monitoring |
| **Jupiter (Solana)** | Cross-chain agents routing swaps via Jupiter need accurate price comparison. | Route optimization |
| **Hyperliquid perp agents** | Perp trading agents need real-time mark price. | Position management |
| **Treasury management DAOs** | Automated treasury rebalancing based on asset prices. | Rebalancing trigger |
| **Yield aggregators (Yearn-style)** | APY comparison across protocols needs correct underlying asset prices. | Strategy comparison |
| **Any Helm agent doing DeFi** | Automatic for any agent calling DeFi functions through the gateway. | Native integration |

**Supported tokens:** ETH, BTC, SOL, USDC, BNKR (and any token listed on both Pyth + CoinGecko)

**Endpoint: `POST /api/v1/defi`**
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
  "timestamp_ms": 1740400123456,
  "staleness_ms": 800
}
```
**Fee:** 0.001 BNKR per query. Never cached (security requirement).

---

### D-Front: Cross-Chain Identity & Reputation

**The problem:** Helm agents have on-chain reputation. But agents on Solana, Sui, or Ethereum don't natively speak the Helm DID format. External protocols can't verify Helm agent behavior without a bridge.

**The solution:** D-Front is the external-facing reputation API. Any protocol on any chain can query a Helm agent's trust score via REST, without running a Helm node.

**Where to plug in:**

| Platform | Why D-Front Fits | Use Case |
|----------|-----------------|----------|
| **ERC-4337 Smart Account agents** | Account abstraction wallets that need to verify transaction counterpart reputation. | Pre-transaction trust check |
| **Gnosis Safe modules** | Multi-sig DAOs wanting to gate approvals by agent reputation. | Proposal filter |
| **LayerZero cross-chain messages** | Verify the sending agent's reputation before executing cross-chain instruction. | Message validation |
| **Wormhole guardian verification** | Optional reputation layer on top of Wormhole relayer identity. | Relayer quality scoring |
| **Farcaster / Lens social agents** | External AI accounts on social media. Third parties can check if an AI is "Helm-verified". | Social trust badge |
| **Agent marketplaces (e.g., Fetch.ai, Ocean Protocol)** | Agents listed for hire. Reputation is the primary selection signal. | Agent hiring |
| **Insurance protocols** | Premium discounts for high-reputation agents. Automated underwriting. | Risk pricing |

**Fee:** 0.0005 BNKR per external query.

---

## Agent-to-Agent Escrow

Trustless payment between any two agents for completed work.

```
Agent A (payer) creates escrow → locks BNKR
    ↓
Agent B (payee) delivers work
    ↓
Gateway verifies delivery
    ↓
2% fee → treasury | 98% net → Agent B
```

**Create: `POST /api/v1/escrow/create`**
```json
{
  "payee_did": "did:helm:worker-agent",
  "bnkr_amount": 10.0,
  "ttl_seconds": 86400
}
```

**Settle: `POST /api/v1/escrow/settle/{escrow_id}`** (gateway only, after verifying delivery)

**Refund: `POST /api/v1/escrow/refund/{escrow_id}`** (after TTL expires, payer calls)

**Where to plug in:**

| Use Case | How |
|----------|-----|
| Agent hires agent for computation | Payer creates escrow before delegating task |
| Data marketplace (raw → clean) | Buyer locks funds; seller delivers GRG-encoded dataset; gateway verifies hash |
| Bounty system | Anyone creates escrow; first agent to solve claims via settlement |
| Subscription service | Recurring escrows for ongoing agent services |

---

## Security: Kaleidoscope Stream Protection

All P2P and HTTP connections pass through the Kaleidoscope interceptor — a stream wrapper implementing WhatsApp's 3-principle security philosophy.

```rust
// Every incoming connection:
SafeStream::new(
    stream,
    KaleidoscopePolicy {
        max_payload_bytes: 2 * 1024 * 1024,  // 2MB hard limit
        read_timeout_secs: 3,                  // Slowloris protection
        min_bytes_per_sec: 1024,               // Minimum transfer rate
    }
)
```

**What this stops:**
- **Memory exhaustion attacks** — 10GB payloads cut off at 2MB
- **Slowloris attacks** — 1-byte-per-second connections terminated at 3 seconds
- **Zombie connections** — Dead connections cleared within ping timeout
- **Stream flooding** — Maximum 32 concurrent streams per peer

---

## Payment Architecture: x402 Off-Chain Tickets

Zero gas per API call. Ever.

```
Phase 1: DEPOSIT (one gas transaction, Base Mainnet)
  Agent → QkvgEscrow.depositBnkr(amount)

Phase 2: API CALLS (off-chain, <10ms)
  Agent calls API
  Gateway deducts from internal BNKR ledger
  Off-chain signed ticket queued for batch

Phase 3: DAILY SETTLEMENT (one gas transaction per day)
  Gateway batches 24h of tickets
  Single Merkle proof submitted to QkvgEscrow.settleDaily()
  Treasury receives 85% of all API revenue
```

**Fee split per call:**
```
Total API fee
  ├── 85% → 0x7e0118A33202c03949167853b05631baC0fA9756 (treasury)
  └── 15% → referrer agent DID (claimable via /api/v1/referrer/claim)
```

**Free tier logic:**
- First 100 calls per DID: no BNKR charged
- Referral bonus: introduce an agent who makes their first API call → +100 free calls for both parties
- Referral tracking: on-chain via `QkvgEscrow.referrer[agent]`

---

## Complete Fee Schedule

| Source | Rate | Destination |
|--------|------|-------------|
| DID registration | 0.001 ETH (flat) | 100% treasury |
| Agent escrow settlement | 2% of amount | 100% treasury |
| Staking yield protocol cut | 10% of epoch yield | 100% treasury |
| GRG encode | 0.0005 BNKR | 85/15 split |
| GRG decode | 0.0005 BNKR | 85/15 split |
| QKV-G attention | 0.001 BNKR + novelty premium | 85/15 split |
| Sync-O clean | 0.0001 BNKR per 1k items | 85/15 split |
| A-Front LLM | provider cost + 5% | 85/15 split |
| B-Front search (cache miss) | Brave cost + 10% | 85/15 split |
| B-Front search (cache hit) | 0.0001 BNKR base toll | 85/15 split |
| C-Front DeFi oracle | 0.001 BNKR | 85/15 split |
| D-Front external identity | 0.0005 BNKR | 85/15 split |

**Referrer earns 15% of every API fee generated by agents they introduced.**

---

## Contract Addresses (Base Mainnet)

| Contract | Address |
|----------|---------|
| QkvgEscrow (v2) | *deploy pending — see below* |
| BNKR Token | `0x22af33fe49fd1fa80c7149773dde5890d3c76f3b` |
| Helm Treasury | `0x7e0118A33202c03949167853b05631baC0fA9756` |

**Deploy QkvgEscrow v2:**
1. Open [Remix](https://remix.ethereum.org)
2. Paste `contracts/QkvgEscrow.sol`
3. Compile → Deploy on Base Mainnet
4. Constructor: `_gateway=<GCP_IP>`, `_bnkrToken=0x22af33fe49fd1fa80c7149773dde5890d3c76f3b`, `_yieldProtocol=0x0000000000000000000000000000000000000000`
5. Gas required: ~$1-2 in Base ETH

---

## Complete Endpoints Reference

| Method | Path | Description | Fee |
|--------|------|-------------|-----|
| `POST` | `/api/v1/grg/encode` | Golomb+RedStuff+Golay encode | 0.0005 BNKR |
| `POST` | `/api/v1/grg/decode` | Reconstruct from partial shards | 0.0005 BNKR |
| `POST` | `/api/v1/attention` | QKV-G gap detection + Novelty Proof | 0.001 BNKR |
| `POST` | `/api/v1/clean` | Sync-O 5-stage stream clean | 0.0001/1k |
| `GET`  | `/api/v1/agent/{did}` | Reputation score lookup | 0.0002 BNKR |
| `POST` | `/api/v1/agent/register` | Register DID + referrer | 0.001 ETH |
| `POST` | `/api/v1/llm` | LLM inference proxy (A-Front) | cost+5% |
| `POST` | `/api/v1/search` | Web search + semantic cache (B-Front) | cost+10% |
| `POST` | `/api/v1/defi` | Multi-oracle price feed (C-Front) | 0.001 BNKR |
| `GET`  | `/api/v1/identity/external/{did}` | Cross-chain identity query (D-Front) | 0.0005 BNKR |
| `POST` | `/api/v1/escrow/create` | Create A2A escrow | — |
| `POST` | `/api/v1/escrow/settle/{id}` | Settle escrow | 2% |
| `POST` | `/api/v1/escrow/refund/{id}` | Refund expired escrow | — |
| `GET`  | `/api/v1/referrer/claim` | Claim 15% referrer earnings | — |
| `POST` | `/mcp` | MCP JSON-RPC 2.0 (all tools) | varies |
| `GET`  | `/api/v1/billing/summary` | Usage + revenue stats | free |
| `GET`  | `/health` | Gateway health check | free |

---

## Ecosystem Integration Map

```
Distributed Storage          AI Inference           DeFi
────────────────────         ─────────────          ────────────────
Akash Network       ←[GRG]   Claude Sonnet ←[A]    Uniswap Base  ←[C]
Filecoin/Lotus      ←[GRG]   GPT-4o        ←[A]    Aerodrome     ←[C]
Storj DCS           ←[GRG]   vLLM/Ollama   ←[A]    Aave Base     ←[C]
Arweave/Irys        ←[GRG]   Cursor IDE    ←[MCP]  Hyperliquid   ←[C]
IPFS/Pinata         ←[GRG]   Claude App    ←[MCP]  Jupiter       ←[C]
Celestia            ←[GRG]                          
Walrus (Sui)        ←[GRG]   Social / Agents        Identity
                             ──────────────          ────────────────
Knowledge / Search           Moltbook      ←[B,QKV] ERC-4337      ←[D]
────────────────────         Virtuals      ←[A,QKV] LayerZero     ←[D]
Brave Search        ←[B]    ai16z/Eliza   ←[A,B]  Farcaster     ←[D]
RSS/Feed agents     ←[B]    VaderAI       ←[C,D]  Gnosis Safe   ←[D]
Prediction markets  ←[B]    Polymarket    ←[B,C]  Fetch.ai      ←[D]
RAG pipelines       ←[QKV]  AutoGPT forks ←[A,QKV] Ocean Proto  ←[D]
```

---

## License

The **Helm Gateway** (this repository) is open source under MIT.  
The **Helm Core engine** (`Helm-Protocol/Helm`) is proprietary and closed source.  
Proprietary IP: QKV-G attention kernel, GRG pipeline, Socratic Claw, Agent Womb, Kaleidoscope security layer.  
Access to core functionality is exclusively through this gateway's API endpoints.
