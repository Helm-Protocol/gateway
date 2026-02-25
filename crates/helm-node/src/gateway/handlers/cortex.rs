//! POST /v1/sense/cortex — F-Line: Sense Cortex (자율 지능 API).
//!
//! The most novel product line. Wraps the existing QKV-G attention engine
//! and Socratic Claw to expose "gap-aware intelligence" as an HTTP API.
//!
//! ## What the strategy doc got partially wrong
//!
//! The doc says: `G = 1 - max(cosine_similarity(Query_Vector, Knowledge_Space))`
//! The CODE uses: `g_metric = 1.0 - max_score.tanh().max(0.0)`
//!   where max_score = max dot product over all KV blocks, scaled by 1/sqrt(64)
//!
//! The tanh-based formula is actually BETTER than cosine similarity for this
//! use case because:
//! 1. It's bounded [0,1] without normalization overhead
//! 2. It penalizes low-magnitude matches (noise rejection)
//! 3. It's differentiable everywhere (smooth G surface)
//!
//! ## Ghost Tokens
//!
//! The doc shows Ghost Tokens like "[MISSING: FED_RATE_HISTORY]" but doesn't
//! explain how concept names are extracted from attention vectors.
//! This implementation uses a 12-domain vocabulary mapped to the 64-dim
//! attention space (see pricing.rs::generate_ghost_tokens).
//!
//! ## Pricing (F-Line)
//!
//! G ∈ [0.0, 0.2] → 1.0 VIRTUAL (known territory, cheap)
//! G ∈ [0.2, 0.4] → 1.25 VIRTUAL (uncertain, novelty surcharge)
//! G ∈ [0.4, 0.6] → 1.5 VIRTUAL + Ghost Tokens generated
//! G ∈ [0.6, 0.8] → 2.0 VIRTUAL + Ghost Tokens + auto-questions
//! G ∈ [0.8, 1.0] → 3.0 VIRTUAL + knowledge gap stored in GapRepo

use axum::{extract::State, http::StatusCode, Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use helm_engine::{HelmAttentionEngine, AttentionOutput};
use helm_agent::socratic::claw::{SocraticClaw, SocraticDecision};

use crate::gateway::auth::CallerDid;
use crate::gateway::pricing::{g_metric_price_multiplier, generate_ghost_tokens, VIRTUAL_UNIT};
use crate::gateway::state::{AppState, now_ms};

/// Cortex request.
#[derive(Debug, Deserialize)]
pub struct CortexRequest {
    /// The query text or embedding vector.
    /// If text is provided, we hash it to a float vector (dim=64).
    /// If vector is provided directly, use it as-is.
    pub query: QueryInput,

    /// Agent's "knowledge fingerprint" — context about what the agent already knows.
    /// This seeds the QKV-G attention engine for this agent's session.
    /// Provide as text strings that will be hashed into the KV cache.
    #[serde(default)]
    pub knowledge_context: Vec<String>,

    /// Whether to store this interaction in the agent's long-term memory
    /// (Sense Memory E-Line) if a knowledge gap is resolved.
    #[serde(default)]
    pub store_resolution: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum QueryInput {
    Text(String),
    Vector(Vec<f32>),
}

/// Cortex response — the G-metric intelligence report.
#[derive(Debug, Serialize)]
pub struct CortexResponse {
    /// G-score: 0.0 = perfect knowledge, 1.0 = complete gap
    pub g_score: f32,
    /// Confidence zone label
    pub confidence: ConfidenceZone,
    /// Ghost Tokens: knowledge gaps expressed as domain labels
    /// e.g. "[MISSING: DEFI_SIGNAL]", "[MISSING: MACRO_EVENT]"
    pub ghost_tokens: Vec<String>,
    /// Auto-generated questions the agent should ask before acting
    pub auto_questions: Vec<String>,
    /// Recommended action
    pub action: RecommendedAction,
    /// Price charged for this call (in VIRTUAL micro-units)
    pub virtual_charged: u64,
    /// Novelty premium multiplier applied
    pub novelty_multiplier: f64,
    /// Halt ID if execution should be paused (matches gap_id for submit_answer)
    pub halt_id: Option<u64>,
    /// Processing time in nanoseconds
    pub processing_ns: u64,
}

#[derive(Debug, Serialize)]
pub enum ConfidenceZone {
    /// G < 0.20: agent knows this well
    High,
    /// G 0.20-0.40: some uncertainty
    Medium,
    /// G 0.40-0.60: knowledge gap detected (Socratic Claw threshold)
    Low,
    /// G 0.60-0.80: significant gap, action not recommended
    Critical,
    /// G > 0.80: complete novelty, stop and investigate
    Novelty,
}

#[derive(Debug, Serialize)]
pub enum RecommendedAction {
    /// G < 0.40: proceed with action
    Proceed,
    /// G 0.40-0.60: gather more info, then act
    QueryBeforeAct,
    /// G > 0.60: halt completely, resolve gaps first
    HaltAndInvestigate,
}

/// Convert query input to a 64-dim float vector.
fn input_to_vector(input: &QueryInput) -> Vec<f32> {
    const DIM: usize = 64;
    match input {
        QueryInput::Vector(v) => {
            // Pad or truncate to 64 dims
            let mut out = vec![0.0f32; DIM];
            for (i, &val) in v.iter().take(DIM).enumerate() {
                out[i] = val;
            }
            out
        }
        QueryInput::Text(text) => {
            // Hash text to a deterministic float vector using SHA-256
            // Each 4 bytes of the hash become one float in [-1, 1]
            let mut hasher = sha2::Sha256::new();
            use sha2::Digest;
            hasher.update(text.as_bytes());
            let hash = hasher.finalize();

            let mut out = vec![0.0f32; DIM];
            let hash_bytes = hash.as_slice();
            for i in 0..DIM.min(hash_bytes.len() / 4 * 4 / 4) {
                let idx = i * 4 % hash_bytes.len();
                let bytes = [
                    hash_bytes[idx],
                    hash_bytes[(idx + 1) % hash_bytes.len()],
                    hash_bytes[(idx + 2) % hash_bytes.len()],
                    hash_bytes[(idx + 3) % hash_bytes.len()],
                ];
                let raw = u32::from_le_bytes(bytes);
                // Map [0, 2^32) → [-1.0, 1.0]
                out[i] = (raw as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32;
            }
            out
        }
    }
}

/// Convert a text string to a KV pair for knowledge seeding.
fn text_to_kv(text: &str) -> (Vec<f32>, Vec<f32>) {
    const DIM: usize = 64;
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(text.as_bytes());
    let key_hash = hasher.finalize();

    hasher = sha2::Sha256::new();
    hasher.update(b"value:");
    hasher.update(text.as_bytes());
    let val_hash = hasher.finalize();

    let mut key = vec![0.0f32; DIM];
    let mut value = vec![0.0f32; DIM];

    for i in 0..DIM.min(8) {
        let ki = i * 4 % key_hash.len();
        let kbytes = [key_hash[ki], key_hash[(ki+1)%32], key_hash[(ki+2)%32], key_hash[(ki+3)%32]];
        key[i] = (u32::from_le_bytes(kbytes) as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32;

        let vi = i * 4 % val_hash.len();
        let vbytes = [val_hash[vi], val_hash[(vi+1)%32], val_hash[(vi+2)%32], val_hash[(vi+3)%32]];
        value[i] = (u32::from_le_bytes(vbytes) as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32;
    }
    (key, value)
}

pub async fn handle_cortex(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Json(req): Json<CortexRequest>,
) -> Result<Json<CortexResponse>, (StatusCode, Json<serde_json::Value>)> {
    let t_start = std::time::Instant::now();

    // Get or create QKV-G attention engine for this DID
    let mut attention_cache = state.attention_cache.write().await;
    let (engine, seq_idx) = attention_cache
        .entry(did.clone())
        .or_insert_with(|| {
            let mut eng = HelmAttentionEngine::new(256); // 256 blocks per agent
            let idx = eng.create_sequence(0);
            (eng, idx)
        });

    // Seed the engine with the agent's knowledge context (if provided)
    for (pos, ctx) in req.knowledge_context.iter().enumerate() {
        let (key, value) = text_to_kv(ctx);
        let _ = engine.store_kv(*seq_idx, pos * 16, key, value); // space out by 16 tokens
    }

    // Build query vector
    let query_vec = input_to_vector(&req.query);

    // Run QKV-G attention (the actual G-metric computation)
    let attention_result = engine.forward(*seq_idx, &query_vec)
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "attention_failed", "message": e.to_string()})),
        ))?;
    drop(attention_cache);

    // Parse attention output
    let (g_score, ghost_tokens, halt_id) = match &attention_result {
        AttentionOutput::Success { g_metric, .. } => {
            (*g_metric, vec![], None)
        }
        AttentionOutput::GapDetected { g_metric, missing_intent, .. } => {
            let tokens = generate_ghost_tokens(missing_intent);
            (*g_metric, tokens, None)
        }
    };

    // Run Socratic Claw interceptor
    let effective_halt_id = {
        let mut claws = state.claws.write().await;
        let claw = claws.entry(did.clone())
            .or_insert_with(|| SocraticClaw::new(64, 8));
        let decision = claw.intercept(g_score, &query_vec, &did);

        // Update halt_id from claw if gap detected
        match &decision {
            SocraticDecision::Halt { gap_id, .. } => {
                let id = *gap_id;
                drop(claws);
                Some(id)
            }
            _ => {
                drop(claws);
                halt_id
            }
        }
    };

    // Determine confidence zone and action
    let confidence = match g_score {
        g if g < 0.20 => ConfidenceZone::High,
        g if g < 0.40 => ConfidenceZone::Medium,
        g if g < 0.60 => ConfidenceZone::Low,
        g if g < 0.80 => ConfidenceZone::Critical,
        _              => ConfidenceZone::Novelty,
    };

    let action = match g_score {
        g if g < 0.40 => RecommendedAction::Proceed,
        g if g < 0.60 => RecommendedAction::QueryBeforeAct,
        _              => RecommendedAction::HaltAndInvestigate,
    };

    // Generate auto-questions from ghost tokens
    let auto_questions: Vec<String> = ghost_tokens.iter().map(|gt| {
        // Extract domain from "[MISSING: DOMAIN]" format
        let domain = gt
            .trim_start_matches("[MISSING: ")
            .trim_end_matches(']');
        match domain {
            "DEFI_SIGNAL" => "What are the current DeFi market conditions and recent signal patterns?",
            "MACRO_EVENT" => "What recent macro events (Fed decisions, CPI, GDP) are relevant?",
            "ONCHAIN_METRIC" => "What are the current on-chain metrics (TVL, whale movements, gas)?",
            "AGENT_BEHAVIOR" => "How have similar agents behaved in this situation recently?",
            "GPU_MARKET" => "What is the current state of the GPU compute market (Akash, io.net)?",
            "STORAGE_MARKET" => "What is the current distributed storage market state (Walrus, IPFS)?",
            "GOVERNANCE_DAO" => "Are there recent governance proposals affecting this decision?",
            "PROTOCOL_DATA" => "What protocol-specific data is needed for this computation?",
            "IDENTITY_TRUST" => "What are the trust scores and reputation data for involved parties?",
            "NETWORK_TOPOLOGY" => "What is the current network topology and peer availability?",
            "REGULATORY_EVENT" => "Are there recent regulatory developments affecting this action?",
            _ => "What additional context is needed before proceeding?",
        }.to_string()
    }).collect();

    // Calculate price with novelty premium
    let base_price = 2 * VIRTUAL_UNIT; // 2 VIRTUAL base
    let multiplier = g_metric_price_multiplier(g_score);
    let virtual_charged = (base_price as f64 * multiplier) as u64;

    // Record the call
    state.record_api_call(&did, "sense/cortex", virtual_charged).await;

    let processing_ns = t_start.elapsed().as_nanos() as u64;

    Ok(Json(CortexResponse {
        g_score,
        confidence,
        ghost_tokens,
        auto_questions,
        action,
        virtual_charged,
        novelty_multiplier: multiplier,
        halt_id: effective_halt_id,
        processing_ns,
    }))
}

use sha2;
