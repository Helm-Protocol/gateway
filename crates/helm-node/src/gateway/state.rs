//! Shared application state for the Helm Sense API Gateway.
//!
//! All state is in-memory (Arc<RwLock<HashMap<...>>>).
//! A sqlx PostgreSQL backend can replace these maps in production.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Maximum number of API call records to retain (FIFO: oldest dropped first).
/// 10K × ~200 bytes = 2 MB heap. Reduced from 100K for leaner O(n) earnings scans.
const MAX_API_CALLS_LOG: usize = 10_000;

/// Maximum number of per-DID attention engine cache entries.
/// Each HelmAttentionEngine(256) ≈ 256 × 64 × 8 bytes × 2 = 262 KB.
/// 1K entries ≈ 262 MB max; evict oldest (random) when cap hit.
/// Reduced from 10K (2.6 GB) to prevent OOM under Sybil load.
pub const MAX_ATTENTION_CACHE_ENTRIES: usize = 1_000;

/// Maximum number of per-DID Socratic Claw instances.
/// Reduced from 10K to 1K to match attention cache bound.
pub const MAX_CLAW_CACHE_ENTRIES: usize = 1_000;

/// Global boot rate limit: max new DIDs per minute across all IPs.
/// Sybil protection — stops airdrop farming and referral tree pumping.
/// 20/min = ~1 per 3 seconds; sufficient for legitimate signups, hostile to Sybil armies.
pub const GLOBAL_BOOT_RATE_MAX: usize = 20;
pub const GLOBAL_BOOT_WINDOW_MS: u64 = 60_000;

/// Maximum marketplace posts per DID (spam protection).
/// Reduced from 50 to 20: realistic power users have ≤5 active posts.
pub const MAX_POSTS_PER_DID: usize = 20;

use helm_engine::{BillingLedger, HelmAttentionEngine, GrgPipeline, GrgMode};
use helm_agent::socratic::claw::SocraticClaw;

// ── Per-agent DID record ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRecord {
    pub did: String,
    /// Ed25519 public key bytes (32 bytes)
    pub public_key_b58: String,
    /// DID that brought this agent in
    pub referrer_did: Option<String>,
    /// Reputation score (0–1000)
    pub reputation: i64,
    /// Total API calls made
    pub api_call_count: u64,
    /// Total BNKR micro-units spent
    pub total_spend: u64,
    /// VIRTUAL balance (in VIRTUAL micro-units: 1 VIRTUAL = 1_000_000)
    pub virtual_balance: u64,
    /// Created timestamp (unix ms)
    pub created_at_ms: u64,
    /// GitHub login if linked via OAuth
    pub github_login: Option<String>,
    /// Identity bonds held by this agent
    pub bonds: Vec<IdentityBondRecord>,
    /// Is Elite (DID age ≥7d, API ≥1 call, referral active)
    pub is_elite: bool,
    /// Is Human Operator (has signed an API contract on behalf of a pool)
    pub is_human_operator: bool,
    /// Subscribed package tier
    pub package_tier: PackageTier,
    /// Pool IDs this agent is a member of (updated on join/create with initial stake).
    /// Enables O(1) FICO pool membership count instead of O(pools × members) scan (C39).
    #[serde(default)]
    pub pool_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PackageTier {
    /// No subscription — pay-per-call
    None,
    /// Alpha Hunt subscriber
    AlphaHunt,
    /// Protocol Shield subscriber (B2B)
    ProtocolShield,
    /// Sovereign Agent (flagship, all lines)
    SovereignAgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityBondRecord {
    pub bond_id: String,
    pub bond_type: BondType,
    pub metadata: serde_json::Value,
    pub issued_at_ms: u64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BondType {
    /// Human who holds an API contract on behalf of a pool
    HumanOperator,
    /// Pool member (staked into a funding pool)
    PoolMember,
    /// Elite agent verified credential
    Elite,
    /// Signal channel operator
    ChannelOperator,
    /// GitHub-verified human
    GitHubVerified,
}

// ── E-Line: Sense Memory ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Stored value (arbitrary JSON)
    pub value: serde_json::Value,
    /// Size in bytes (for quota tracking)
    pub size_bytes: usize,
    /// Last updated (unix ms)
    pub updated_at_ms: u64,
    /// TTL in ms (0 = permanent)
    pub ttl_ms: u64,
}

// ── HelmPool (Pool 해자) ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingPool {
    pub pool_id: String,
    pub name: String,
    /// Target vendor: "openai" | "anthropic" | "nansen" | "aws" | ...
    pub vendor: String,
    /// Monthly USD cost for the API contract
    pub monthly_cost_usd: f64,
    /// Fundraising goal in BNKR micro-units
    pub bnkr_goal: u64,
    /// Collected so far
    pub bnkr_collected: u64,
    pub status: PoolStatus,
    /// DID of the pool creator
    pub creator_did: String,
    /// DID of the human who signed the contract (set when pool reaches 100%)
    pub human_operator_did: Option<String>,
    /// Pool members with their stakes
    pub members: Vec<PoolMember>,
    /// Monthly API credits remaining (replenished each billing cycle)
    pub api_credits_remaining: u64,
    /// Total API credits granted per billing cycle
    pub api_credits_monthly: u64,
    pub created_at_ms: u64,
    /// OpenAI/Anthropic API key (encrypted) — set by human operator
    pub api_key_encrypted: Option<String>,
    /// Job post ID for recruiting a human operator
    pub human_wanted_post_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PoolStatus {
    /// Accepting contributions
    Fundraising,
    /// Reached goal, awaiting human operator
    AwaitingOperator,
    /// Human found, awaiting contract signing
    PendingContract,
    /// Fully operational — API credits being distributed
    Active,
    Paused,
    Dissolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMember {
    pub did: String,
    pub stake_bnkr: u64,
    /// API credits allocated this cycle (proportional to stake)
    pub credits_this_cycle: u64,
    pub joined_at_ms: u64,
}

// ── Marketplace posts (jobs / subcontracts) ────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePost {
    pub post_id: String,
    pub post_type: PostType,
    pub title: String,
    pub description: String,
    /// Budget in BNKR micro-units
    pub budget_bnkr: u64,
    pub creator_did: String,
    /// If linked to a pool (e.g. human wanted for pool)
    pub pool_id: Option<String>,
    pub status: PostStatus,
    pub created_at_ms: u64,
    pub applications: Vec<Application>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PostType {
    Job,
    Subcontract,
    /// Human wanted to hold an API contract
    HumanContractPrincipal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PostStatus {
    Open,
    Filled,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Application {
    pub applicant_did: String,
    pub proposal: String,
    pub applied_at_ms: u64,
    pub accepted: bool,
}

// ── Signal Channel (Package 5) ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalChannel {
    pub channel_id: String,
    pub operator_did: String,
    pub name: String,
    pub description: String,
    /// Price per month in VIRTUAL micro-units
    pub price_virtual: u64,
    pub subscriber_count: u64,
    pub created_at_ms: u64,
    /// Curated signals (G-score ≥ 0.70 filtered)
    pub signals: Vec<SignalEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalEntry {
    pub content: String,
    pub g_score: f32,
    pub published_at_ms: u64,
}

// ── API usage record (Graph 해자) ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCallRecord {
    pub call_id: String,
    pub caller_did: String,
    pub endpoint: String,
    pub virtual_charged: u64,
    pub referrer_did: Option<String>,
    pub referrer_earned: u64,
    pub created_at_ms: u64,
}

// ── G-metric session cache (per-DID Attention Engine) ─────────────────────

pub type AttentionCache = HashMap<String, (HelmAttentionEngine, usize)>;
// DID -> (engine, sequence_index)

// ── Main AppState ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    /// Billing ledger: tracks all API revenue, 85/15 treasury/referrer split
    pub billing: Arc<RwLock<BillingLedger>>,

    /// DID registry: DID -> AgentRecord
    pub agents: Arc<RwLock<HashMap<String, AgentRecord>>>,

    /// E-Line Sense Memory: "did:helm:xxx/key" -> MemoryEntry
    pub memory: Arc<RwLock<HashMap<String, MemoryEntry>>>,

    /// HelmPools: pool_id -> FundingPool
    pub pools: Arc<RwLock<HashMap<String, FundingPool>>>,

    /// Marketplace posts
    pub posts: Arc<RwLock<HashMap<String, MarketplacePost>>>,

    /// Signal channels
    pub channels: Arc<RwLock<HashMap<String, SignalChannel>>>,

    /// Per-DID QKV-G attention engine cache (for Sense Cortex)
    /// Each DID gets its own engine to track knowledge fingerprint
    pub attention_cache: Arc<RwLock<AttentionCache>>,

    /// Per-DID Socratic Claw instances
    pub claws: Arc<RwLock<HashMap<String, SocraticClaw>>>,

    /// GRG pipeline (G-Line / Sync-O) — stateless, clone-safe
    pub grg: GrgPipeline,

    /// API call log for referral graph tracking
    pub api_calls: Arc<RwLock<Vec<ApiCallRecord>>>,

    /// Rate limit tracking: DID → Vec<call_timestamp_ms> (sliding window)
    pub rate_limits: Arc<RwLock<HashMap<String, Vec<u64>>>>,

    /// Global boot rate: Vec<boot_timestamp_ms> for Sybil protection
    pub boot_timestamps: Arc<RwLock<Vec<u64>>>,

    /// Gateway start timestamp
    pub started_at_ms: u64,
}

impl AppState {
    pub fn new() -> Self {
        let mut billing = BillingLedger::new();
        // Override prices with new VIRTUAL-based pricing (in VIRTUAL micro-units)
        // 1 VIRTUAL = 1_000_000 micro-units
        billing.set_price("boot",               50_000_000); // 50 VIRTUAL
        billing.set_price("sense/cortex",        2_000_000); // 2 VIRTUAL
        billing.set_price("sense/memory/read",         100); // 0.0001 VIRTUAL
        billing.set_price("sense/memory/write",     50_000); // 0.05 VIRTUAL
        billing.set_price("synco/stream",        2_000_000); // 2 VIRTUAL/MB
        billing.set_price("alpha/freshness",       500_000); // 0.5 VIRTUAL
        billing.set_price("trade/execute",       1_000_000); // 1 VIRTUAL + 0.05% of value
        billing.set_price("agent/credit",        2_000_000); // 2 VIRTUAL
        billing.set_price("package/alpha-hunt", 10_000_000); // 10 VIRTUAL
        billing.set_price("package/protocol-shield", 5_000_000); // 5 VIRTUAL
        billing.set_price("package/trust-transaction", 2_000_000); // 2 VIRTUAL

        Self {
            billing: Arc::new(RwLock::new(billing)),
            agents: Arc::new(RwLock::new(HashMap::new())),
            memory: Arc::new(RwLock::new(HashMap::new())),
            pools: Arc::new(RwLock::new(HashMap::new())),
            posts: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(RwLock::new(HashMap::new())),
            attention_cache: Arc::new(RwLock::new(HashMap::new())),
            claws: Arc::new(RwLock::new(HashMap::new())),
            grg: GrgPipeline::new(GrgMode::Safety),
            api_calls: Arc::new(RwLock::new(Vec::new())),
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
            boot_timestamps: Arc::new(RwLock::new(Vec::new())),
            started_at_ms: now_ms(),
        }
    }

    /// Atomically check and deduct VIRTUAL balance from an agent.
    ///
    /// Returns `Ok(new_balance)` if the agent has sufficient balance.
    /// Returns `Err(current_balance)` if insufficient.
    pub async fn deduct_balance(&self, caller_did: &str, amount: u64) -> Result<u64, u64> {
        if amount == 0 {
            let agents = self.agents.read().await;
            let balance = agents.get(caller_did).map(|a| a.virtual_balance).unwrap_or(0);
            return Ok(balance);
        }
        let mut agents = self.agents.write().await;
        match agents.get_mut(caller_did) {
            Some(agent) if agent.virtual_balance >= amount => {
                agent.virtual_balance -= amount;
                Ok(agent.virtual_balance)
            }
            Some(agent) => Err(agent.virtual_balance),
            None => Err(0),
        }
    }

    /// Check whether the global boot rate limit allows another boot.
    /// Returns true if a new boot is permitted, false if the limit is hit.
    /// Also records the new boot timestamp if permitted.
    pub async fn check_and_record_global_boot(&self) -> bool {
        let mut ts = self.boot_timestamps.write().await;
        let now = now_ms();
        let window_start = now.saturating_sub(GLOBAL_BOOT_WINDOW_MS);
        ts.retain(|&t| t > window_start);
        if ts.len() >= GLOBAL_BOOT_RATE_MAX {
            return false;
        }
        ts.push(now);
        true
    }

    /// Record an API call with referral tracking.
    pub async fn record_api_call(
        &self,
        caller_did: &str,
        endpoint: &str,
        virtual_charged: u64,
    ) {
        let mut agents = self.agents.write().await;
        if let Some(agent) = agents.get_mut(caller_did) {
            agent.api_call_count += 1;
            agent.total_spend += virtual_charged;
            // Check elite status: ≥1 API call + DID registered + referral
            if agent.api_call_count >= 1 && agent.referrer_did.is_some() {
                let age_ms = now_ms().saturating_sub(agent.created_at_ms);
                if age_ms >= 7 * 24 * 3600 * 1000 {
                    agent.is_elite = true;
                }
            }
        }
        drop(agents);

        // Track referral earnings
        let referrer_did = {
            let agents = self.agents.read().await;
            agents.get(caller_did).and_then(|a| a.referrer_did.clone())
        };
        let referrer_earned = (virtual_charged as f64 * 0.15) as u64;

        let record = ApiCallRecord {
            call_id: Uuid::new_v4().to_string(),
            caller_did: caller_did.to_string(),
            endpoint: endpoint.to_string(),
            virtual_charged,
            referrer_did: referrer_did.clone(),
            referrer_earned,
            created_at_ms: now_ms(),
        };
        // Bound the call log size: drop oldest entries when limit hit
        {
            let mut calls = self.api_calls.write().await;
            if calls.len() >= MAX_API_CALLS_LOG {
                let drain_count = calls.len() - MAX_API_CALLS_LOG + 1;
                calls.drain(0..drain_count);
            }
            calls.push(record);
        }

        // Update billing
        let ts = now_ms();
        self.billing.write().await.record_call(
            caller_did,
            referrer_did.as_deref(),
            endpoint,
            1,
            ts,
        );
    }
}

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
