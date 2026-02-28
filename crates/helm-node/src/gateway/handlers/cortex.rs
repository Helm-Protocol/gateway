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
    /// G < 0.10: agent knows this well (v3.0 KNOWN zone)
    Known,
    /// G 0.10-0.30: slight uncertainty (v3.0 FAMILIAR zone)
    Familiar,
    /// G 0.30-0.60: knowledge gap detected (v3.0 PARTIAL zone, Socratic Claw threshold)
    Partial,
    /// G 0.60-0.85: significant gap, Ghost Tokens required (v3.0 NOVEL zone)
    Novel,
    /// G > 0.85: complete novelty, knowledge update credit (v3.0 FRONTIER zone)
    Frontier,
}

#[derive(Debug, Serialize)]
pub enum RecommendedAction {
    /// G < 0.30 (KNOWN/FAMILIAR): proceed with action
    Proceed,
    /// G 0.30-0.60 (PARTIAL): gather more info before acting
    QueryBeforeAct,
    /// G > 0.60 (NOVEL/FRONTIER): halt, resolve gaps and fill Ghost Tokens first
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

const MAX_CONTEXT_ITEMS: usize = 50;
const MAX_CONTEXT_ITEM_LEN: usize = 4096;
const MAX_QUERY_TEXT_LEN: usize = 65536;
const MAX_QUERY_VECTOR_DIM: usize = 512; // accept up to 512 dims; truncated to 64 internally

pub async fn handle_cortex(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Json(req): Json<CortexRequest>,
) -> Result<Json<CortexResponse>, (StatusCode, Json<serde_json::Value>)> {
    let t_start = std::time::Instant::now();

    // Validate input sizes (before charging — don't charge for invalid requests)
    if req.knowledge_context.len() > MAX_CONTEXT_ITEMS {
        return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "too_many_context_items",
            "max_items": MAX_CONTEXT_ITEMS,
            "provided": req.knowledge_context.len()
        }))));
    }
    for ctx in &req.knowledge_context {
        if ctx.len() > MAX_CONTEXT_ITEM_LEN {
            return Err((StatusCode::BAD_REQUEST, Json(json!({
                "error": "context_item_too_long",
                "max_chars": MAX_CONTEXT_ITEM_LEN
            }))));
        }
    }
    match &req.query {
        QueryInput::Text(t) if t.len() > MAX_QUERY_TEXT_LEN => {
            return Err((StatusCode::BAD_REQUEST, Json(json!({
                "error": "query_text_too_long",
                "max_chars": MAX_QUERY_TEXT_LEN
            }))));
        }
        QueryInput::Vector(v) if v.len() > MAX_QUERY_VECTOR_DIM => {
            return Err((StatusCode::BAD_REQUEST, Json(json!({
                "error": "query_vector_too_large",
                "max_dims": MAX_QUERY_VECTOR_DIM
            }))));
        }
        _ => {}
    }

    // Pre-charge base fee BEFORE expensive computation (prevents free oracle abuse).
    // If balance is insufficient, fail fast without wasting compute.
    let base_price = 2 * VIRTUAL_UNIT;
    state.deduct_balance(&did, base_price).await.map_err(|avail| (
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({
            "error": "insufficient_balance",
            "required": base_price,
            "available": avail,
            "message": "Need at least 2 VIRTUAL to use Sense Cortex. Top up your balance."
        })),
    ))?;

    // Get or create QKV-G attention engine for this DID
    // Evict oldest entry when cache is at capacity (OOM protection: C22)
    let mut attention_cache = state.attention_cache.write().await;
    if attention_cache.len() >= crate::gateway::state::MAX_ATTENTION_CACHE_ENTRIES
        && !attention_cache.contains_key(&did)
    {
        // Evict an arbitrary entry (first key by HashMap iteration)
        if let Some(evict_key) = attention_cache.keys().next().cloned() {
            attention_cache.remove(&evict_key);
            tracing::debug!("Evicted attention cache entry: {}", evict_key);
        }
    }
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
    // Evict oldest claw entry when at capacity (C23 OOM protection)
    let effective_halt_id = {
        let mut claws = state.claws.write().await;
        if claws.len() >= crate::gateway::state::MAX_CLAW_CACHE_ENTRIES
            && !claws.contains_key(&did)
        {
            if let Some(evict_key) = claws.keys().next().cloned() {
                claws.remove(&evict_key);
            }
        }
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

    // Determine confidence zone and action (v3.0 boundaries: 0/0.10/0.30/0.60/0.85/1.0)
    let confidence = match g_score {
        g if g < 0.10 => ConfidenceZone::Known,
        g if g < 0.30 => ConfidenceZone::Familiar,
        g if g < 0.60 => ConfidenceZone::Partial,
        g if g < 0.85 => ConfidenceZone::Novel,
        _              => ConfidenceZone::Frontier,
    };

    let action = match g_score {
        g if g < 0.30 => RecommendedAction::Proceed,
        g if g < 0.60 => RecommendedAction::QueryBeforeAct,
        _              => RecommendedAction::HaltAndInvestigate,
    };

    // Generate auto-questions from ghost tokens
    let auto_questions: Vec<String> = ghost_tokens.iter().map(|gt| {
        // Extract domain from "[MISSING: DOMAIN]" format
        let domain = gt
            .trim_start_matches("[MISSING: ")
            .trim_end_matches(']');
        // v3.0 DeFi-specific Ghost Token questions
        match domain {
            "FED_RATE_HISTORY"        => "What are the current Fed rate trajectory and monetary policy signals?",
            "ETH_MACRO_CORRELATION"   => "How is ETH/BTC price correlated with current macro conditions?",
            "DEFI_TVL_DATA"           => "What is the current DeFi TVL breakdown across major protocols?",
            "WHALE_MOVEMENTS"         => "What large wallet movements are visible on-chain in the last 24h?",
            "MEV_RISK"                => "What is the current MEV and sandwich attack risk for this action?",
            "MARKET_SENTIMENT"        => "What does the current fear/greed index and funding rate say?",
            "REGULATORY_CONTEXT"      => "Are there recent regulatory developments affecting this position?",
            "POLYMARKET_ODDS"         => "What are the Polymarket prediction market odds for related events?",
            "L2_BRIDGE_ACTIVITY"      => "What is the current L2 bridge volume and gas cost differential?",
            "STABLECOIN_FLOWS"        => "Are there stablecoin de-peg risks or unusual mint/burn flows?",
            "NFT_COLLECTION_DATA"     => "What are the relevant NFT collection floor prices and royalty flows?",
            "PROTOCOL_GOVERNANCE"     => "Are there active governance proposals affecting this protocol?",
            _ => "What additional context is needed before proceeding?",
        }.to_string()
    }).collect();

    // Calculate total price with novelty premium.
    // base_price was already pre-charged before computation.
    // Charge the novelty premium as additional deduction.
    let multiplier = g_metric_price_multiplier(g_score);
    let virtual_charged = (base_price as f64 * multiplier) as u64;
    let novelty_premium = virtual_charged.saturating_sub(base_price);
    if novelty_premium > 0 {
        // Best-effort: if agent ran out (spent everything between pre-charge and now), allow it
        // Agents accept this risk when they send requests with low balance.
        let _ = state.deduct_balance(&did, novelty_premium).await;
    }

    // Record the call for tracking (billing ledger, referral graph, api_call_count)
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
