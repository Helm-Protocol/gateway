//! POST /v1/agent/boot — DID 해자 seed product.
//!
//! Atomically:
//!   1. Generates a new Ed25519 keypair → did:helm:<base58>
//!   2. Creates agent record with referrer link
//!   3. Issues BOOT_CREDITS welcome credits
//!   4. Initializes Socratic Claw (G-metric engine) for this DID
//!
//! This is the entry point to the Helm network. Every agent starts here.
//! Once a DID is created, all subsequent API calls accumulate history
//! against it — that history IS the DID moat (switching platforms means
//! starting from reputation 0).

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use helm_agent::socratic::claw::SocraticClaw;

use crate::gateway::state::{AgentRecord, AppState, BondType, IdentityBondRecord, PackageTier, now_ms};
use crate::gateway::pricing::VIRTUAL_UNIT;

/// Boot request from an agent.
#[derive(Debug, Deserialize)]
pub struct BootRequest {
    /// Optional DID of the agent that referred this new agent.
    /// The referrer earns 15% of all future API revenue from this agent.
    pub referrer_did: Option<String>,

    /// Primary capability: "compute" | "storage" | "network" | "defi" | "llm" | ...
    #[serde(default = "default_capability")]
    pub capability: String,

    /// Optional GitHub login for human agents verified via OAuth
    pub github_login: Option<String>,

    /// Preferred payment token for future calls: "VIRTUAL" | "BNKR" | "USDC"
    #[serde(default = "default_token")]
    pub preferred_token: String,
}

fn default_capability() -> String { "compute".to_string() }
fn default_token() -> String { "VIRTUAL".to_string() }

/// Boot response — agent's new identity.
#[derive(Debug, Serialize)]
pub struct BootResponse {
    /// The new agent's DID
    pub did: String,
    /// Ed25519 public key in base58
    pub public_key_b58: String,
    /// Ed25519 PRIVATE key in base58 — SAVE THIS, never share it
    pub private_key_b58: String,
    /// Initial VIRTUAL credits (welcome bonus)
    pub welcome_credits: u64,
    /// Referrer DID (if any) — earns 15% of your future API spend
    pub referrer_did: Option<String>,
    /// Referrer cut in basis points (1500 = 15%)
    pub referrer_cut_bps: u32,
    /// How to authenticate: include this in Authorization header
    pub auth_header: String,
    /// Next step suggestion
    pub next_step: String,
    /// Boot cost in VIRTUAL micro-units
    pub boot_cost_virtual: u64,
    /// Created at (unix ms)
    pub created_at_ms: u64,
}

/// Welcome credits given to new agents (in VIRTUAL micro-units).
/// 5 VIRTUAL — enough for ~2 Cortex + 1 Helm Score call to explore the platform.
/// Reduced from 10V: lower ROI on Sybil farming while remaining useful for genuine signups.
const WELCOME_CREDITS: u64 = 5 * VIRTUAL_UNIT;

const MAX_CAPABILITY_LEN: usize = 64;
const MAX_GITHUB_LOGIN_LEN: usize = 64;
const MAX_TOKEN_LEN: usize = 16;
const VALID_TOKENS: &[&str] = &["VIRTUAL", "BNKR", "USDC", "CLANKER"];

pub async fn handle_boot(
    State(state): State<AppState>,
    Json(req): Json<BootRequest>,
) -> Result<(StatusCode, Json<BootResponse>), (StatusCode, Json<serde_json::Value>)> {
    // Validate field lengths
    if req.capability.len() > MAX_CAPABILITY_LEN {
        return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "capability_too_long",
            "max_chars": MAX_CAPABILITY_LEN
        }))));
    }
    if let Some(ref login) = req.github_login {
        if login.len() > MAX_GITHUB_LOGIN_LEN {
            return Err((StatusCode::BAD_REQUEST, Json(json!({
                "error": "github_login_too_long",
                "max_chars": MAX_GITHUB_LOGIN_LEN
            }))));
        }
        // GitHub logins must be alphanumeric + hyphens only
        if !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err((StatusCode::BAD_REQUEST, Json(json!({
                "error": "github_login_invalid",
                "message": "GitHub login must be alphanumeric with hyphens only"
            }))));
        }
    }
    if req.preferred_token.len() > MAX_TOKEN_LEN
        || !VALID_TOKENS.contains(&req.preferred_token.as_str())
    {
        return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_preferred_token",
            "valid": VALID_TOKENS
        }))));
    }

    // Global boot rate limit (Sybil protection: max 20 new DIDs/minute)
    if !state.check_and_record_global_boot().await {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "global_boot_rate_limit",
                "message": "Too many new agent registrations. Try again in 60 seconds.",
                "retry_after_ms": 60_000u64,
            })),
        ));
    }

    // Validate referrer exists (if provided)
    if let Some(ref ref_did) = req.referrer_did {
        let agents = state.agents.read().await;
        if !agents.contains_key(ref_did.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "referrer_not_found",
                    "message": format!("Referrer DID '{}' not registered", ref_did)
                })),
            ));
        }
    }

    // Generate Ed25519 keypair directly (same formula as helm-identity HelmKeyPair::did())
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();

    // DID = "did:helm:<base58(pubkey_bytes)>"  — same formula as helm-identity
    let public_key_b58 = bs58::encode(verifying_key.as_bytes()).into_string();
    let did = format!("did:helm:{}", public_key_b58);
    let private_key_b58 = bs58::encode(signing_key.to_bytes()).into_string();

    let now = now_ms();

    // Build initial bonds
    let mut bonds = Vec::new();
    if req.github_login.is_some() {
        bonds.push(IdentityBondRecord {
            bond_id: uuid::Uuid::new_v4().to_string(),
            bond_type: BondType::GitHubVerified,
            metadata: json!({ "github_login": req.github_login }),
            issued_at_ms: now,
            active: true,
        });
    }

    // Create agent record
    let agent = AgentRecord {
        did: did.clone(),
        public_key_b58: public_key_b58.clone(),
        referrer_did: req.referrer_did.clone(),
        reputation: 0,
        api_call_count: 0,
        total_spend: 0,
        virtual_balance: WELCOME_CREDITS,
        created_at_ms: now,
        github_login: req.github_login.clone(),
        bonds,
        is_elite: false,
        is_human_operator: false,
        package_tier: PackageTier::None,
        pool_ids: Vec::new(),
    };

    // Check for duplicate (shouldn't happen with random keypair, but defensive)
    {
        let mut agents = state.agents.write().await;
        if agents.contains_key(&did) {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "did_collision", "message": "DID already exists (astronomical improbability)"})),
            ));
        }
        agents.insert(did.clone(), agent);
    }

    // Initialize Socratic Claw for this agent (model_dim=64, latent_dim=8)
    {
        let mut claws = state.claws.write().await;
        claws.insert(did.clone(), SocraticClaw::new(64, 8));
    }

    // Record boot fee (waived for first boot — charged from welcome credits)
    let boot_cost = 0u64; // Boot is free; welcome credits cover initial exploration

    // Charge DID registration protocol fee
    let ts = now_ms();
    state.billing.write().await.charge_did_registration(&did, ts);

    // Sanitize user inputs before logging (log injection prevention)
    let safe_capability = req.capability.replace(['\r', '\n', '\0'], "_");
    let safe_github = req.github_login.as_deref().unwrap_or("none").replace(['\r', '\n', '\0'], "_");
    tracing::info!("AgentBoot: new DID {} capability={} github={} referrer={}",
        did, safe_capability, safe_github,
        req.referrer_did.as_deref().unwrap_or("none"));

    Ok((
        StatusCode::CREATED,
        Json(BootResponse {
            auth_header: format!("Bearer {}", did),
            next_step: "Use your DID as Bearer token. Try POST /v1/sense/cortex or POST /v1/synco/stream".to_string(),
            did: did.clone(),
            public_key_b58,
            private_key_b58,
            welcome_credits: WELCOME_CREDITS,
            referrer_did: req.referrer_did,
            referrer_cut_bps: 1500,
            boot_cost_virtual: boot_cost,
            created_at_ms: now,
        }),
    ))
}
