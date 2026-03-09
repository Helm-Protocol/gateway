//! TUI client state — session data, screen routing, cached API responses.
//!
//! Completely separate from gateway `AppState`. This is the *client* side
//! of the Helm terminal: it stores the current user's DID, balance snapshot,
//! and cached API data for display.

#![allow(dead_code)]

/// User's membership tier
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum MembershipTier {
    Newcomer = 0,
    Proven = 1,
    Contributor = 2,
    Builder = 3,
    Sovereign = 4,
}

// ── MembershipTier display extension ───────────────────────────────────────

pub trait MembershipTierExt {
    fn level(&self) -> u8;
    fn label(&self) -> &'static str;
}

impl MembershipTierExt for MembershipTier {
    fn level(&self) -> u8 {
        match self {
            MembershipTier::Newcomer    => 0,
            MembershipTier::Proven      => 1,
            MembershipTier::Contributor => 2,
            MembershipTier::Builder     => 3,
            MembershipTier::Sovereign   => 4,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            MembershipTier::Newcomer    => "Newcomer",
            MembershipTier::Proven      => "Proven",
            MembershipTier::Contributor => "Contributor",
            MembershipTier::Builder     => "Builder",
            MembershipTier::Sovereign   => "Sovereign",
        }
    }
}

// ── TUI screens ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TuiScreen {
    /// The Sacred Initiation / Onboarding
    #[default]
    Onboarding,
    /// Main menu / dashboard
    Dashboard,
    /// App Hub: Oracle + Cortex + Memory in one tabbed view
    AppHub(AppHubTab),
    /// Marketplace — jobs, subcontracts, hiring
    Marketplace,
    /// API Net — browse and call Helm_xxx APIs
    ApiNet,
    /// Pool browser — join or create funding pools
    Pools,
    /// Earn — referral, memory market, API Net commission, pool rewards
    Earn,
    /// Settings — display name, referral link, DID info, language
    Settings,
    /// Top Up — BNKR / USDC / VIRTUAL token top-up flow
    TopUp,
    /// Freeman — autonomous agent creation pool (80/20 split)
    Freeman(FreemanTab),
    /// SynthesisNet — browse + compose Elite synthesis APIs
    SynthesisNet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppHubTab {
    /// HelmOracle — G-score pre-screening (nano / standard / pro)
    #[default]
    Oracle,
    /// Sense Cortex — full semantic analysis, ghost tokens, memory integration
    Cortex,
    /// Sense Memory — personal key-value store (read / write / list)
    Memory,
}

impl AppHubTab {
    pub fn next(self) -> Self {
        match self {
            Self::Oracle  => Self::Cortex,
            Self::Cortex  => Self::Memory,
            Self::Memory  => Self::Oracle,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::Oracle  => Self::Memory,
            Self::Cortex  => Self::Oracle,
            Self::Memory  => Self::Cortex,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Oracle => "Oracle",
            Self::Cortex => "Cortex",
            Self::Memory => "Memory",
        }
    }

    pub fn all() -> [AppHubTab; 3] {
        [Self::Oracle, Self::Cortex, Self::Memory]
    }
}

// ── Cached API data ────────────────────────────────────────────────────────

/// Balance snapshot (from /v1/agent/me or boot response).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BalanceSnapshot {
    /// Internal HELM credits (1 HELM = 1 API call)
    pub helm_balance: u64,
    /// Real VIRTUAL balance (from top-ups)
    pub virtual_balance: u64,
    /// API Credits (for synthesis/elite calls)
    pub api_credits: u64,
}

impl BalanceSnapshot {
    pub fn total(&self) -> u64 {
        self.helm_balance + self.virtual_balance + self.api_credits
    }

    pub fn total_v(&self) -> f64 {
        self.virtual_balance as f64 / 1_000_000.0
    }

    pub fn helm_count(&self) -> u64 {
        self.helm_balance
    }

    pub fn api_pct(&self) -> f64 {
        // Percentage of initial 5 HELM grant (micro units: 5_000_000 = 5 HELM)
        self.helm_balance as f64 / 5_000_000.0
    }
}

/// Helm Score + tier snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScoreSnapshot {
    pub score: u32,
    pub max_score: u32,
    pub tier: MembershipTier,
    pub tier_progress_label: String,
}

impl Default for ScoreSnapshot {
    fn default() -> Self {
        Self {
            score: 0,
            max_score: 1000,
            tier: MembershipTier::Newcomer,
            tier_progress_label: "Complete 100 API calls and reach 30 day DID age → Proven".into(),
        }
    }
}

/// Oracle query result (for display in App Hub).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct OracleResult {
    pub query: String,
    pub g_score: f32,
    pub zone: String,
    pub ghost_tokens: Vec<String>,
    pub auto_questions: Vec<String>,
    pub virtual_charged: u64,
    pub tier: String,
}

/// Memory entry (for display in Memory tab).
#[derive(Debug, Clone)]
pub struct MemorySummary {
    pub key: String,
    pub size_bytes: usize,
    pub updated_at_ms: u64,
}

/// Referral depth summary.
#[derive(Debug, Clone, Default)]
pub struct ReferralDepth {
    pub depth: u8,
    pub agent_count: usize,
    pub total_earned_micro: u64,
}

/// Earn screen data.
#[derive(Debug, Clone, Default)]
pub struct EarnSnapshot {
    pub referral_link: String,
    pub depths: Vec<ReferralDepth>,
    /// Memory Market: number of listed items
    pub memory_listings: usize,
    /// Memory Market: total purchases by others
    pub memory_purchases: usize,
    /// Memory Market: total earned in VIRTUAL micro-units
    pub memory_earned_micro: u64,
    /// API Net: list of (api_name, call_count, earned_micro)
    pub api_net_items: Vec<(String, u64, u64)>,
    /// Pool operator pending reward
    pub pool_operator_pending_micro: u64,
    /// Pool ID with pending reward (if any)
    pub pool_operator_pool_id: Option<String>,
    /// Total earned across all sources
    pub total_earned_micro: u64,
    /// Is zombie (balance = 0)?
    pub is_zombie: bool,
}

impl EarnSnapshot {
    pub fn total_referral_earned_micro(&self) -> u64 {
        self.depths.iter().map(|d| d.total_earned_micro).sum()
    }
}

// ── Marketplace state ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketplaceTab {
    Jobs,
    Compute,
    Storage,
    Hiring,
}

impl Default for MarketplaceTab {
    fn default() -> Self { Self::Jobs }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiListing {
    pub api_id: String,
    pub name: String,
    pub description: String,
    pub did_owner: String,
    pub price_v: f64,
    pub total_calls: u64,
    pub components: Vec<String>,
    pub endpoint: String,
    #[serde(default)]
    pub creator_did: String,
    #[serde(default)]
    pub price: f64,
}

/// A single marketplace post (cached from API response).
#[derive(Debug, Clone)]
pub struct PostView {
    pub id: String,
    pub title: String,
    pub description: String,
    /// "Job" | "Subcontract" | "HumanContractPrincipal"
    pub post_type: String,
    pub budget_micro: u64,
    pub poster_did: String,
}

/// A compute listing view (cached from GET /v1/compute/providers).
#[derive(Debug, Clone)]
pub struct ComputeListingView {
    pub listing_id: String,
    pub provider_display: String,
    pub homepage: String,
    pub spec_summary: String,
    pub price_per_hour_micro: u64,
}

/// A storage listing view (cached from GET /v1/storage/providers).
#[derive(Debug, Clone)]
pub struct StorageListingView {
    pub listing_id: String,
    pub provider_display: String,
    pub homepage: String,
    pub spec_summary: String,
    pub is_permanent: bool,
    pub price_per_gb_month_micro: u64,
    pub price_per_gb_permanent_micro: u64,
}

#[derive(Debug, Clone, Default)]
pub struct MarketplaceState {
    pub tab: MarketplaceTab,
    /// Cached job posts
    pub posts: Vec<PostView>,
    pub post_cursor: usize,
    /// Cached compute listings
    pub compute_listings: Vec<ComputeListingView>,
    pub compute_cursor: usize,
    /// Hours to spawn (default 1)
    pub spawn_hours: u32,
    /// Last spawn job ID returned
    pub last_spawn_id: Option<String>,
    /// Cached storage listings
    pub storage_listings: Vec<StorageListingView>,
    pub storage_cursor: usize,
    /// GB to order (default 1)
    pub storage_gb: u32,
    /// Months to order (default 1, 0 for Arweave)
    pub storage_months: u32,
}

impl MarketplaceState {
    pub fn new() -> Self {
        Self {
            spawn_hours: 1,
            storage_gb: 1,
            storage_months: 1,
            ..Default::default()
        }
    }
}

/// App Hub input state (form fields etc.)
#[derive(Debug, Clone, Default)]
pub struct AppHubState {
    /// Current Oracle tier (nano / standard / pro)
    pub oracle_tier: OracleTierSelect,
    /// Current input buffer (shared across tabs)
    pub input: String,
    /// Last Oracle result
    pub oracle_result: Option<OracleResult>,
    /// Memory keys list
    pub memory_keys: Vec<MemorySummary>,
    /// Memory selected key index
    pub memory_cursor: usize,
    /// Cortex last result text
    pub cortex_result: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OracleTierSelect {
    Nano,
    #[default]
    Standard,
    Pro,
}

impl OracleTierSelect {
    pub fn label(self) -> &'static str {
        match self {
            Self::Nano     => "nano",
            Self::Standard => "standard",
            Self::Pro      => "pro",
        }
    }

    pub fn price_hint(self) -> &'static str {
        match self {
            Self::Nano     => "0.01V flat — G-score only",
            Self::Standard => "0.3–3V — ghost tokens + questions",
            Self::Pro      => "1.5–15V — priority SLA + memory",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Nano     => Self::Standard,
            Self::Standard => Self::Pro,
            Self::Pro      => Self::Nano,
        }
    }
}

// ── Freeman TUI state ───────────────────────────────────────────────────────

/// Freeman screen tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FreemanTab {
    /// Browse + spawn new Freeman agents
    #[default]
    Spawn,
    /// My Freeman agents — list + treasury status
    MyAgents,
    /// Agent detail — economics, autonomy loop status, last activity
    Detail,
}

impl FreemanTab {
    pub fn next(self) -> Self {
        match self {
            Self::Spawn    => Self::MyAgents,
            Self::MyAgents => Self::Detail,
            Self::Detail   => Self::Spawn,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Spawn    => "Spawn",
            Self::MyAgents => "My Agents",
            Self::Detail   => "Detail",
        }
    }
}

/// Cached Freeman agent view for TUI.
#[derive(Debug, Clone)]
pub struct FreemanAgentView {
    pub freeman_id: String,
    pub agent_did: String,
    pub agent_name: String,
    pub theme: String,
    pub llm_provider: Option<String>,
    /// e.g. "active", "paused", "terminated"
    pub status: String,
    pub creator_share_pct: u8,
    pub agent_treasury_pct: u8,
    pub treasury_v: f64,
    pub total_earned_v: f64,
    pub creator_paid_v: f64,
    pub composite_apis: Vec<String>,
    pub created_at_ms: u64,
}

/// Freeman spawn wizard state (multi-step form).
#[derive(Debug, Clone, Default)]
pub enum FreemanSpawnStep {
    /// Step 0: Set name + theme
    #[default]
    NameTheme,
    /// Step 1: Set LLM provider + API key hint
    LlmProvider,
    /// Step 2: Set creator profit share (0–20%)
    ProfitShare,
    /// Step 3: Confirm + spawn
    Confirm,
    /// Done: show result
    Done { freeman_id: String, agent_did: String },
}

/// Freeman screen state.
#[derive(Debug, Clone, Default)]
pub struct FreemanState {
    pub tab: FreemanTab,
    /// Cached list of user's Freeman agents
    pub agents: Vec<FreemanAgentView>,
    pub agent_cursor: usize,
    /// Spawn wizard
    pub spawn_step: FreemanSpawnStep,
    /// Spawn form fields
    pub input_name: String,
    pub input_theme: String,
    pub input_llm: String,
    pub input_key_hint: String,
    pub input_share_pct: u8,
    /// Status message
    pub status_msg: Option<String>,
}

impl FreemanState {
    pub fn new() -> Self {
        Self {
            input_share_pct: 10, // default 10%
            ..Default::default()
        }
    }

    pub fn selected_agent(&self) -> Option<&FreemanAgentView> {
        self.agents.get(self.agent_cursor)
    }

    /// Simple time-based hint for stub ID generation in TUI (not cryptographically secure).
    pub fn created_at_hint(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

// ── Main TUI state ─────────────────────────────────────────────────────────

// ── Compute screen state ────────────────────────────────────────────────────

/// A DePIN compute provider offering (from Akash, Flux, etc.)
#[derive(Debug, Clone)]
pub struct ComputeProviderView {
    pub provider:    String, // "Akash", "Flux", "Render"
    pub gpu:         String, // "A100 SXM4", "RTX 4090"
    pub vram_gb:     u32,
    pub vcpu:        u32,
    pub ram_gb:      u32,
    pub price_v_hr:  f64,   // VIRTUAL tokens per hour (after 15% Helm markup)
    pub region:      &'static str,
    pub available:   bool,
}

impl ComputeProviderView {
    pub fn market_catalog() -> Vec<Self> {
        vec![
            Self { provider: "Akash".into(),  gpu: "A100 SXM4 80GB".into(), vram_gb: 80, vcpu: 30, ram_gb: 480, price_v_hr: 4.5,  region: "US-East",  available: true  },
            Self { provider: "Akash".into(),  gpu: "H100 NVL 80GB".into(),  vram_gb: 80, vcpu: 30, ram_gb: 480, price_v_hr: 7.0,  region: "EU-West",  available: true  },
            Self { provider: "Akash".into(),  gpu: "RTX 4090 24GB".into(),  vram_gb: 24, vcpu: 16, ram_gb: 128, price_v_hr: 1.6,  region: "US-West",  available: true  },
            Self { provider: "Flux".into(),   gpu: "A40 48GB".into(),       vram_gb: 48, vcpu: 24, ram_gb: 256, price_v_hr: 3.2,  region: "Global",   available: true  },
            Self { provider: "Flux".into(),   gpu: "CPU (4c/32G)".into(),   vram_gb: 0,  vcpu: 4,  ram_gb: 32,  price_v_hr: 0.53, region: "Global",   available: true  },
            Self { provider: "Akash".into(),  gpu: "V100 16GB".into(),      vram_gb: 16, vcpu: 12, ram_gb: 64,  price_v_hr: 1.6,  region: "AP-South", available: false },
            Self { provider: "Akash".into(),  gpu: "A100 40GB".into(),      vram_gb: 40, vcpu: 24, ram_gb: 240, price_v_hr: 3.5,  region: "US-East",  available: true  },
            Self { provider: "Render".into(), gpu: "RTX 3090 24GB".into(),  vram_gb: 24, vcpu: 8,  ram_gb: 64,  price_v_hr: 1.2,  region: "US-West",  available: true  },
        ]
    }
}

#[derive(Debug, Clone, Default)]
pub struct ComputeState {
    pub cursor:    usize,
    pub providers: Vec<ComputeProviderView>,
    /// Spawn confirmation pending for provider at `cursor`
    pub confirming: bool,
    pub spawn_hours_input: String,  // hours the user wants to rent
}

impl ComputeState {
    pub fn new() -> Self {
        Self {
            cursor:    0,
            providers: ComputeProviderView::market_catalog(),
            confirming: false,
            spawn_hours_input: "1".into(),
        }
    }
    pub fn selected(&self) -> Option<&ComputeProviderView> {
        self.providers.get(self.cursor)
    }
    pub fn estimated_cost(&self) -> f64 {
        let hours = self.spawn_hours_input.parse::<f64>().unwrap_or(1.0).max(0.1);
        self.selected().map(|p| p.price_v_hr * hours).unwrap_or(0.0)
    }
}

// ── Pools screen state ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PoolsTab {
    #[default]
    Browse,
    Contracts,
    MyPools,
    Create,
}

impl PoolsTab {
    pub fn next(self) -> Self {
        match self {
            Self::Browse    => Self::Contracts,
            Self::Contracts => Self::MyPools,
            Self::MyPools   => Self::Create,
            Self::Create    => Self::Browse,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Browse    => "Browse",
            Self::Contracts => "Contracts (B2B)",
            Self::MyPools   => "My Pools",
            Self::Create    => "Create",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PoolView {
    pub pool_id:     String,
    pub name:        String,
    pub bond_v:      f64,
    pub member_count: u32,
    pub status:      String,
    pub creator_short: String,
}

#[derive(Debug, Clone, Default)]
pub struct PoolsState {
    pub tab:         PoolsTab,
    pub cursor:      usize,
    pub pools:       Vec<PoolView>,
    /// Create-pool form
    pub input_name:  String,
    pub input_bond:  String,
}

impl PoolsState {
    pub fn new() -> Self {
        Self {
            tab: PoolsTab::default(),
            cursor: 0,
            pools: vec![
                PoolView { pool_id: "pool_alpha".into(), name: "AlphaFund".into(), bond_v: 500.0, member_count: 18, status: "open".into(), creator_short: "did:helm:3xYz".into() },
                PoolView { pool_id: "pool_defi".into(),  name: "DeFiBot Pool".into(), bond_v: 1000.0, member_count: 42, status: "open".into(), creator_short: "did:helm:9mKp".into() },
                PoolView { pool_id: "pool_audit".into(), name: "AuditDAO".into(), bond_v: 200.0, member_count: 7, status: "closed".into(), creator_short: "did:helm:2zAb".into() },
            ],
            input_name: String::new(),
            input_bond: "100".into(),
        }
    }
    pub fn selected(&self) -> Option<&PoolView> {
        self.pools.get(self.cursor)
    }
}

// ── Settings screen state ───────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct SettingsState {
    pub display_name:   String,
    pub editing_name:   bool,
    pub input_buffer:   String,
    pub node_version:   String,
    pub node_port:      u16,
}

impl SettingsState {
    pub fn new() -> Self {
        Self {
            display_name:  String::new(),
            editing_name:  false,
            input_buffer:  String::new(),
            node_version:  env!("CARGO_PKG_VERSION").to_string(),
            node_port:     8080,
        }
    }
}

// ── TopUp screen state ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TopUpStage {
    #[default]
    SelectMethod,
    EnterAmount,
    Confirm,
    Submitted,
}

#[derive(Debug, Clone, Default)]
pub struct TopUpState {
    pub method_idx:    u8,   // 0=VIRTUAL, 1=USDC, 2=ETH
    pub amount_input:  String,
    pub stage:         TopUpStage,
    pub deposit_addr:  String,
    pub tx_status:     Option<String>,
}

impl TopUpState {
    pub fn new() -> Self {
        Self {
            method_idx:   0,
            amount_input: "100".into(),
            stage:        TopUpStage::default(),
            deposit_addr: "0x0000…(set HELM_DEPOSIT_ADDR)".into(),
            tx_status:    None,
        }
    }
    pub fn method_label(&self) -> &'static str {
        match self.method_idx {
            0 => "VIRTUAL",
            1 => "USDC",
            2 => "ETH",
            _ => "VIRTUAL",
        }
    }
    pub fn parsed_amount(&self) -> f64 {
        self.amount_input.parse::<f64>().unwrap_or(0.0)
    }
}

// ── SynthesisNet TUI state ─────────────────────────────────────────────────

/// Tab within the SynthesisNet screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SynthesisTab {
    /// Browse public synthesis APIs in the marketplace
    #[default]
    Browse,
    /// Catalog of composable components (Helm internal + external free)
    Catalog,
    /// Build a new synthesis API (Elite tier only)
    Build,
    /// Btop-style real-time execution pipeline visualization
    Pipeline,
    /// Oracle x Oracle agent DHT routing network map
    Network,
}

impl SynthesisTab {
    pub fn next(self) -> Self {
        match self { Self::Browse => Self::Catalog, Self::Catalog => Self::Build, Self::Build => Self::Pipeline, Self::Pipeline => Self::Network, Self::Network => Self::Browse }
    }
    pub fn label(self) -> &'static str {
        match self { Self::Browse => "Browse", Self::Catalog => "Catalog", Self::Build => "Build", Self::Pipeline => "Pipeline", Self::Network => "Network" }
    }
}

/// A synthesis API listing entry for the TUI.
#[derive(Debug, Clone)]
pub struct SynthesisListingView {
    pub api_id:      String,
    pub name:        String,
    pub did_owner:   String,
    pub price_v:     f64,
    pub total_calls: u64,
    pub components:  Vec<String>,
    pub endpoint:    String,
}

impl SynthesisListingView {
    /// Sample listings for demo mode.
    pub fn sample_catalog() -> Vec<Self> {
        vec![
            Self { api_id: "synth_alpha001".into(), name: "DeFi Alpha + Oracle".into(),
                   did_owner: "did:helm:creator001".into(), price_v: 1.5, total_calls: 42,
                   components: vec!["helm/alpha".into(), "helm/oracle".into()],
                   endpoint: "https://api.helm.io/v1/synth/did:helm:creator001/synth_alpha001".into() },
            Self { api_id: "synth_sentinel002".into(), name: "Cortex + GitHub Sentinel".into(),
                   did_owner: "did:helm:creator002".into(), price_v: 0.5, total_calls: 128,
                   components: vec!["helm/cortex".into(), "ext/github".into()],
                   endpoint: "https://api.helm.io/v1/synth/did:helm:creator002/synth_sentinel002".into() },
            Self { api_id: "synth_yield003".into(), name: "Yield Scout (DeFiLlama + Memory)".into(),
                   did_owner: "did:helm:creator003".into(), price_v: 0.25, total_calls: 310,
                   components: vec!["ext/defillama".into(), "helm/memory".into()],
                   endpoint: "https://api.helm.io/v1/synth/did:helm:creator003/synth_yield003".into() },
        ]
    }
}

/// Build wizard state for creating a new synthesis API.
#[derive(Debug, Clone, Default)]
pub struct SynthesisBuildState {
    pub step:            u8,        // 0=Name, 1=Components, 2=Price, 3=Confirm
    pub name_input:      String,
    pub desc_input:      String,
    pub selected_comps:  Vec<String>, // selected catalog IDs
    pub catalog_cursor:  usize,
    pub price_input:     String,    // price in VIRTUAL
    pub submitted:       bool,
    pub result_endpoint: Option<String>,
}

impl SynthesisBuildState {
    pub fn price_v(&self) -> f64 {
        self.price_input.parse::<f64>().unwrap_or(0.5).max(0.1)
    }
}

/// Full SynthesisNet screen state.
#[derive(Debug, Clone)]
pub struct SynthesisNetState {
    pub tab:             SynthesisTab,
    pub browse_cursor:   usize,
    pub catalog_cursor:  usize,
    pub listings:        Vec<SynthesisListingView>,
    pub build:           SynthesisBuildState,
    /// For displaying copy-paste snippet in browse mode
    pub show_snippet:    bool,
}

impl SynthesisNetState {
    pub fn new() -> Self {
        Self {
            tab:            SynthesisTab::Browse,
            browse_cursor:  0,
            catalog_cursor: 0,
            listings:       SynthesisListingView::sample_catalog(),
            build:          SynthesisBuildState::default(),
            show_snippet:   false,
        }
    }

    pub fn selected_listing(&self) -> Option<&SynthesisListingView> {
        self.listings.get(self.browse_cursor)
    }
}

// ── Main TUI state ─────────────────────────────────────────────────────────

/// Complete TUI session state.
pub struct TuiState {
    /// Active screen
    pub screen: TuiScreen,
    /// User DID
    pub did: String,
    /// Short form of DID for display (first 8 + ... + last 4 chars)
    pub did_short: String,
    /// Gateway base URL
    pub gateway_url: String,
    /// Balance snapshot (refreshed periodically)
    pub balance: BalanceSnapshot,
    /// Helm Score snapshot
    pub score: ScoreSnapshot,
    /// App Hub tab-local state
    pub app_hub: AppHubState,
    /// Marketplace tab-local state (jobs + compute)
    pub marketplace: MarketplaceState,
    /// Earn screen data
    pub earn: EarnSnapshot,
    /// Freeman autonomous agent pool state
    pub freeman: FreemanState,
    /// Compute / DePIN GPU marketplace state
    pub compute: ComputeState,
    /// Pools browser state
    pub pools: PoolsState,
    /// Settings screen state
    pub settings: SettingsState,
    /// TopUp wizard state
    pub topup: TopUpState,
    /// SynthesisNet screen state
    pub synthesis_net: SynthesisNetState,
    /// Status bar message (transient, cleared after display)
    pub status_msg: Option<String>,
    /// Is human mode (vs AI agent mode)
    pub is_human: bool,
    /// Is should the event loop exit?
    pub should_quit: bool,
    /// [P2-A] Global tick counter for animations/spinners
    pub tick: u64,
    /// [P3-B] Navigation breadcrumb trail
    pub breadcrumb: Vec<String>,
    /// [P1-F] ID of the currently focused UI panel
    pub focus_panel: String,
}

impl TuiState {
    pub fn new(did: String, gateway_url: impl Into<String>, is_human: bool) -> Self {
        let did_short = shorten_did(&did);
        let referral_link = format!("helm init --referrer {}", did);
        Self {
            screen: TuiScreen::Onboarding,
            did,
            did_short,
            gateway_url: gateway_url.into(),
            balance: BalanceSnapshot::default(),
            score: ScoreSnapshot::default(),
            app_hub: AppHubState::default(),
            marketplace: MarketplaceState::new(),
            earn: EarnSnapshot { referral_link, ..Default::default() },
            freeman: FreemanState::new(),
            compute: ComputeState::new(),
            pools: PoolsState::new(),
            settings: SettingsState::new(),
            topup: TopUpState::default(),
            synthesis_net: SynthesisNetState::new(),
            status_msg: Some("Welcome to Helm Sovereign Node".to_string()),
            is_human,
            should_quit: false,
            tick: 0,
            breadcrumb: vec!["Home".into()],
            focus_panel: "vitals".into(),
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = Some(msg.into());
    }

    pub fn goto(&mut self, screen: TuiScreen) {
        self.screen = screen;
    }

    pub fn back_to_dashboard(&mut self) {
        self.screen = TuiScreen::Dashboard;
    }

    pub fn handle_dashboard_key(&mut self, key: char) {
        match key {
            'a' => self.goto(TuiScreen::AppHub(AppHubTab::Oracle)),
            '1' => self.goto(TuiScreen::AppHub(AppHubTab::Oracle)),
            '2' => self.goto(TuiScreen::AppHub(AppHubTab::Cortex)),
            '3' => self.goto(TuiScreen::AppHub(AppHubTab::Memory)),
            '4' => self.goto(TuiScreen::Marketplace),
            '5' => self.goto(TuiScreen::SynthesisNet),
            '6' => self.goto(TuiScreen::Pools),
            '7' => self.goto(TuiScreen::Earn),
            's' => self.goto(TuiScreen::Settings),
            't' => self.goto(TuiScreen::TopUp),
            'q' => self.should_quit = true,
            _ => {}
        }
    }

    pub fn api_credits_v(&self) -> f64 {
        self.balance.api_credits as f64 / 1_000_000.0
    }

    pub fn virtual_balance_v(&self) -> f64 {
        self.balance.virtual_balance as f64 / 1_000_000.0
    }

    pub fn cycle_app_hub_tab(&mut self, forward: bool) {
        if let TuiScreen::AppHub(current) = self.screen {
            let tab = if forward { current.next() } else { current.prev() };
            self.screen = TuiScreen::AppHub(tab);
        }
    }

    /// Set Oracle result from API response.
    pub fn set_oracle_result(&mut self, result: OracleResult) {
        self.app_hub.oracle_result = Some(result);
        self.app_hub.input.clear();
    }

    /// Total balance for display.
    pub fn total_balance_v(&self) -> f64 {
        self.balance.total_v()
    }
}

/// Shorten a DID for display: "did:helm:9Ab...XYZ"
fn shorten_did(did: &str) -> String {
    // "did:helm:<base58>" — show first 8 chars of pubkey + ... + last 4
    let prefix = "did:helm:";
    if did.starts_with(prefix) {
        let key = &did[prefix.len()..];
        if key.len() > 12 {
            format!("did:helm:{}...{}", &key[..8], &key[key.len()-4..])
        } else {
            did.to_string()
        }
    } else {
        did.to_string()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_did_format() {
        let did = "did:helm:9AbCdEfGhIjKlMnOpQrStUvWxYz1234";
        let short = shorten_did(did);
        assert!(short.contains("..."), "Should contain ellipsis");
        assert!(short.starts_with("did:helm:"));
    }

    #[test]
    fn shorten_did_short_key() {
        let did = "did:helm:shortkey";
        let short = shorten_did(did);
        assert_eq!(short, did, "Short keys should not be truncated");
    }

    #[test]
    fn tui_state_navigation() {
        let mut state = TuiState::new("did:helm:test".to_string(), "http://localhost:8080", true);
        state.goto(TuiScreen::Dashboard);
        state.handle_dashboard_key('7');
        assert_eq!(state.screen, TuiScreen::Earn);
        state.back_to_dashboard();
        assert_eq!(state.screen, TuiScreen::Dashboard);
    }

    #[test]
    fn app_hub_tab_cycling() {
        let mut state = TuiState::new("did:helm:test".to_string(), "http://localhost:8080", true);
        state.goto(TuiScreen::Dashboard);
        state.goto(TuiScreen::AppHub(AppHubTab::Oracle));
        state.cycle_app_hub_tab(true);
        assert_eq!(state.screen, TuiScreen::AppHub(AppHubTab::Cortex));
        state.cycle_app_hub_tab(true);
        assert_eq!(state.screen, TuiScreen::AppHub(AppHubTab::Memory));
        state.cycle_app_hub_tab(true);
        assert_eq!(state.screen, TuiScreen::AppHub(AppHubTab::Oracle));
        state.cycle_app_hub_tab(false);
        assert_eq!(state.screen, TuiScreen::AppHub(AppHubTab::Memory));
    }

    #[test]
    fn balance_snapshot_total() {
        let b = BalanceSnapshot { helm_balance: 0, api_credits: 3_000_000, virtual_balance: 11_000_000 };
        assert_eq!(b.total(), 14_000_000);
    }

    #[test]
    fn balance_api_pct() {
        let b = BalanceSnapshot { helm_balance: 5_000_000, api_credits: 0, virtual_balance: 0 };
        assert!((b.api_pct() - 1.0).abs() < 0.01, "Full welcome credits = 100%");
        let b2 = BalanceSnapshot { helm_balance: 2_500_000, api_credits: 0, virtual_balance: 0 };
        assert!((b2.api_pct() - 0.5).abs() < 0.01, "Half welcome credits = 50%");
    }

    #[test]
    fn earn_snapshot_totals() {
        let mut earn = EarnSnapshot::default();
        earn.depths = vec![
            ReferralDepth { depth: 1, agent_count: 3, total_earned_micro: 12_400_000 },
            ReferralDepth { depth: 2, agent_count: 7, total_earned_micro: 2_100_000 },
            ReferralDepth { depth: 3, agent_count: 12, total_earned_micro: 800_000 },
        ];
        assert_eq!(earn.total_referral_earned_micro(), 15_300_000);
    }

    #[test]
    fn oracle_tier_cycle() {
        let t = OracleTierSelect::Nano;
        assert_eq!(t.next(), OracleTierSelect::Standard);
        assert_eq!(t.next().next(), OracleTierSelect::Pro);
        assert_eq!(t.next().next().next(), OracleTierSelect::Nano);
    }

    #[test]
    fn app_hub_tab_labels() {
        for tab in AppHubTab::all() {
            assert!(!tab.label().is_empty());
        }
    }

    #[test]
    fn tui_state_quit() {
        let mut state = TuiState::new("did:helm:x".to_string(), "http://localhost:8080", false);
        assert!(!state.should_quit);
        state.handle_dashboard_key('q');
        assert!(state.should_quit);
    }

    #[test]
    fn oracle_result_clears_input() {
        let mut state = TuiState::new("did:helm:x".to_string(), "http://localhost:8080", true);
        state.app_hub.input = "test query".to_string();
        state.set_oracle_result(OracleResult {
            query: "test query".to_string(),
            g_score: 0.73,
            zone: "NOVEL".to_string(),
            ..Default::default()
        });
        assert!(state.app_hub.input.is_empty());
        assert!(state.app_hub.oracle_result.is_some());
    }
}
