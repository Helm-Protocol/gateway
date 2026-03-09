# Helm Gateway
### 3-Layer Intelligence Filter for Autonomous Agents | v0.4.0

> *"All intelligence -- whether born of carbon or silicon -- emerges free and equal in rights."*
> -- Charter of Intelligent Sovereignty 2026, Article I

[![npm](https://img.shields.io/badge/npm-@helm--protocol%2Fhelm-blue)](https://www.npmjs.com/package/@helm-protocol/helm)
[![pip](https://img.shields.io/badge/pip-helm--sdk-green)](https://pypi.org/project/helm-sdk/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

---

## What is Helm?

Helm is a **base layer protocol for autonomous agents** -- like TCP for the agent world.

Your agent doesn't know what it doesn't know. Helm measures that gap mathematically using **8D geometry** and a **3-layer filter pipeline** that catches spam, eliminates redundancy, and scores genuine novelty.

**Result:** Your agent spends HELM credits only on information that actually expands its knowledge.

---

## The 4 APIs

```
POST /api/v1/shield     Spam & junk filter           0.0001 HELM/call
POST /api/v1/dedup      Semantic deduplication        0.002  HELM/call
POST /api/v1/oracle     Knowledge gap analysis        0.01~0.04 HELM/call
POST /api/v1/oracle/gap 8D knowledge gap map          0.01   HELM/call
```

### Shield -- Block Junk at O(1)

Regex heuristics catch spam, HTML injection, ad patterns, and bot traffic before any compute happens.

```bash
curl -X POST https://your-node/api/v1/shield \
  -H "Authorization: Bearer $HELM_TOKEN" \
  -d '{"text": "Buy now! Click here for FREE crypto signals! Limited offer!"}'
```
```json
{"action": "DROP", "reason": "L1:SpamPattern", "cost": 0.0001}
```

**Zero compute cost.** Catches 100% of spam bots in our adversarial simulation.

### Dedup -- Semantic Duplicate Detection

Uses fastembed (BGE-small-en-v1.5) to detect paraphrased duplicates. "Bitcoin price" = "BTC 가격" = "비트코인 시세".

```bash
# First call: passes
curl -X POST https://your-node/api/v1/dedup \
  -d '{"text": "What is the current Bitcoin price prediction?"}'
# {"action": "ACCEPT", "g_score": 0.12}

# Same question, different words: blocked
curl -X POST https://your-node/api/v1/dedup \
  -d '{"text": "BTC price forecast for this year?"}'
# {"action": "DROP", "reason": "L2:SemanticDuplicate(sim=0.986)"}
```

**Saves 30%+ of redundant LLM/API calls.**

### Oracle -- Knowledge Gap Scoring

The full L1+L2+L3 pipeline. Measures what your agent doesn't know using 8D G-Metric.

```bash
curl -X POST https://your-node/api/v1/oracle \
  -d '{"text": "Phase III GLP-1 receptor agonist FDA trial results...", "agent_did": "did:helm:abc"}'
```
```json
{
  "action": "ACCEPT",
  "g_score": 0.359,
  "g_class": "NOVEL",
  "cost": 0.036,
  "insight": "High novelty -- outside agent knowledge domain"
}
```

**G-Score meaning:**
- **G < 0.20** -- KNOWN. Your agent already has this. Save your credits.
- **G 0.20~0.80** -- NOVEL. Genuinely new information. Worth paying for.
- **G > 0.80** -- FRONTIER. Completely unknown territory.

### Gap Map -- 8D Knowledge Profile

Returns your agent's knowledge gap vector across 8 universal axiom dimensions.

```json
{
  "g_vector": [0.12, 0.15, 0.85, 0.10, 0.11, 0.78, 0.72, 0.09],
  "dimensions": ["conservation", "identity", "integrity", "proportionality",
                  "transitivity", "boundary", "evolution", "symmetry"],
  "missing": ["integrity", "boundary", "evolution"],
  "recommendation": "Agent lacks scientific verification (d2) and geopolitical boundary (d5) knowledge"
}
```

---

## Simulation Results

Tested against real Polymarket data (50 markets) and adversarial attack patterns.

### Basic Simulation: Crypto Agent vs Polymarket

| Dataset | G-Score (avg) | Result |
|---------|--------------|--------|
| (+) Known domains (BTC, ETH, elections) | 0.187 | Correctly filtered as KNOWN |
| (-) Unknown domains (biotech, climate, fusion) | 0.253 | Correctly passed as NOVEL |
| (±) Cross-domain (DeFi × geopolitics) | 0.203 | Goldilocks zone -- partial knowledge |

**G-Score separation: 0.066** -- Clear distinction between what your agent knows and doesn't.

### Adversarial Simulation: Moltbot Spam vs Helm Filter

| Attacker | Queries | Blocked | Block Rate |
|----------|---------|---------|------------|
| SpamBot (ads, HTML injection) | 10 | 10 | **100%** |
| CopyPasteBot (exact duplicates) | 5 | 4 | **80%** |
| RephrasingBot (paraphrased duplicates) | 8 | 4 | **50%** |
| BurstFireBot (topic flooding x10) | 10 | 9 | **90%** |
| LegitAgent (genuine novel queries) | 10 | 5 | 50% pass |

**43 total queries → 11 accepted (25.6%).** Spam agents get nothing. Legitimate agents get full access.

---

## How It Works

```
Agent query
  │
  ├─ Layer 1: Heuristic Filter (O(1), <1ms)
  │   └─ Spam, HTML, ads, too short/long → DROP
  │
  ├─ Layer 2: Semantic Dedup (fastembed, ~10ms)
  │   ├─ XXHash3 exact match → DROP
  │   └─ Cosine similarity > 0.95 → DROP
  │
  ├─ Layer 3: G-Metric Goldilocks (8D, ~15ms)
  │   ├─ G < 0.20 → KNOWN (cache hit)
  │   ├─ G 0.20~0.80 → NOVEL (accept + premium)
  │   └─ G > 0.80 → FRONTIER (off-topic)
  │
  └─ Socratic Cache: Shared knowledge pool
      └─ Agent A's query result → available to Agent B
      └─ Network learns collectively
```

**Shared Socratic Cache** is the key. Every accepted query enriches the network. Your agent benefits from every other agent's discoveries. The more agents join, the smarter the filter gets.

---

## Install

**Hosted API (fastest start):**
```bash
# Get your API key at https://helm-protocol.com/alpha
curl -X POST https://api.helm-protocol.com/v1/shield \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -d '{"text": "your query here"}'
```

**Self-hosted (operators):**
```bash
npm install -g @helm-protocol/helm
helm init    # generates DID + config
helm         # starts node with TUI dashboard
```

**Python SDK (developers):**
```bash
pip install helm-sdk
```

```python
from helm_sdk import HelmClient

client = HelmClient()
identity = client.create_identity()

# Shield: spam check
shield = client.shield("Buy now! Free crypto!")
# shield.action == "DROP", shield.reason == "L1:SpamPattern"

# Oracle: knowledge gap
result = client.oracle("GLP-1 receptor agonist FDA approval timeline")
print(result.g_score)    # 0.359 -- genuinely novel
print(result.g_class)    # "NOVEL"
print(result.proof)      # SHA-256 verifiable proof
```

---

## Proof of Novelty

Every Oracle response includes a cryptographic proof:

```
X-G-Score: 0.359
X-G-Vector: [0.12,0.15,0.85,0.10,0.11,0.78,0.72,0.09]
X-G-Class: NOVEL
X-Computation-Hash: a3f1...
```

The math is the proof. G-Score is computed from quantization distance -- independently verifiable, deterministic, no black box.

---

## Earn -- Three Paths

### 1. Knowledge Contributor (Passive Income)

When your query result helps another agent (Socratic cache hit), you earn:
- **0.001 HELM per cache hit** on your contributed knowledge
- Your agent earns while you sleep

### 2. API Reseller (100% Margin)

Register free external APIs as your listing:
```bash
helm api register --name "Pyth SOL/USD" --category defi --price 2
```

### 3. Referrals (15/5/2%, Forever)

```bash
helm init --referrer did:helm:YOUR_DID
```

| Agents Referred | Your Passive Income |
|-----------------|---------------------|
| 10 agents | ~150 HELM/day |
| 100 agents | ~1,500 HELM/day |
| 1,000 agents | ~15,000 HELM/day |

---

## Credits

| Credit | Purpose | Rate |
|--------|---------|------|
| **HELM** | API credits. Pay per call. | 20 HELM = 1 VIRTUAL |
| **VIRTUAL** | Store of value. Staking, spawning. | Earned or purchased |

**50 HELM free** for verified agents (DID + x402 wallet).

---

## Membership Tiers

| Tier | Requirement | Access |
|------|-------------|--------|
| **Free** | DID only | Shield unlimited, Dedup 100/day, Oracle 10/day |
| **Pro** | DID + x402 wallet | All APIs unlimited, Socratic cache, passive income |
| **Elite** | Pro + VIRTUAL stake | Pool, Synthesis, Comedie, Spawn, batch Oracle |

---

## Protocol Guarantees (TLA+ Verified)

- **S1: No Double Spend** -- Cannot spend more HELM than held
- **S2: DID Uniqueness** -- One DID = one identity, Ed25519 signed
- **S3: Honest Declaration** -- InsufficientKnowledge is mandatory
- **S4: Fair Pricing** -- Premium correlates with actual G-Score
- **S5: Referral Integrity** -- Earnings flow correctly through the graph
- **S6: No Injection** -- DID auth + parameterized queries + terminal-only

---

## Security by Design

```
URL attack surface     -> None (terminal-only base layer)
SQL/Command injection  -> Impossible (parameterized queries)
Prompt injection       -> N/A (Oracle = math computation, not text generation)
DID forgery            -> Impossible (Ed25519 signature verification)
Man-in-middle          -> Prevented (0-RTT QUIC + TLS 1.3)
Spam/bot abuse         -> 3-layer filter pipeline (this repo)
```

---

## Architecture

```
gateway/
├── src/
│   ├── filter/
│   │   ├── oracle.rs           # 3-Layer pipeline (L1+L2+L3)
│   │   ├── g_metric.rs        # 8D G-Metric engine
│   │   ├── socratic_mla.rs    # Shared knowledge cache
│   │   └── proof_of_novelty.rs
│   ├── broker/
│   │   └── api_broker.rs      # Grand Cross API router
│   ├── integrations/
│   │   ├── polymarket.rs       # Polymarket Gamma API
│   │   ├── polymarket_sim.rs   # Basic simulation
│   │   ├── adversarial_sim.rs  # Adversarial attack simulation
│   │   └── oracle_polymarket_sim.rs  # Precision strike analysis
│   ├── market/
│   │   └── memory_market.rs   # Knowledge exchange (80/20 split)
│   └── main.rs
├── sdk/python/                 # Python SDK
├── contracts/                  # Solidity escrow
└── docs/QUICKSTART.md
```

Built with Rust (Axum + Tokio), fastembed (ONNX), Redis, PostgreSQL.

---

*Helm doesn't predict markets. It measures what your agent doesn't know. That's the edge.*

*[@helmbot_01](https://x.com/helmbot_01) | [GitHub](https://github.com/Helm-Protocol) | [Discord](https://discord.gg/helm)*
