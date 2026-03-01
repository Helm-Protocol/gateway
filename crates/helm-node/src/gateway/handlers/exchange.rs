//! POST /v1/auth/exchange — BYOK (Bring Your Own Key) DID bridge.
//!
//! Allows agents that already have an external identity (did:ethr:0xABC,
//! did:web:agent.example.com, etc.) to bind it to their Helm DID without
//! creating a new keypair. After binding, a 30-day session token is issued.
//!
//! ## Flow
//!
//! 1. Agent boots once via POST /v1/agent/boot → gets did:helm:xxx + private_key_b58.
//! 2. Agent calls POST /v1/auth/exchange:
//!    {
//!      "local_did":   "did:helm:xxx",          // existing Helm DID
//!      "global_did":  "did:ethr:0xABC",         // external identity to bind
//!      "timestamp_ms": 1740000000000,            // must be within 15s of server time
//!      "signature":   "<base64(ed25519_sign(sha256(timestamp_ms + ':' + global_did)))>"
//!    }
//! 3. Gateway verifies Ed25519 signature using the Helm keypair (pubkey is in the DID itself).
//! 4. Maps global_did → local_did (idempotent: same global_did always returns same local_did).
//! 5. Issues a 30-day session token: "helm_sess_<32-byte-hex>".
//! 6. Agent can now use: Authorization: Bearer helm_sess_<token> instead of DID string.
//!
//! ## Why Ed25519 instead of secp256k1?
//! Helm DIDs embed Ed25519 public keys (did:helm:<base58-pubkey>), so signature
//! verification uses the same scheme as the existing auth middleware. Full secp256k1
//! (ERC-4361 SIWE) support will be added in a future version when the `k256` crate
//! is integrated. For now, the agent proves ownership of the Helm keypair, and
//! attaches an external identity label to it.
//!
//! ## Session Token Format
//! helm_sess_<64-char lowercase hex>
//! Stored in AppState::session_tokens with 30-day TTL.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::gateway::state::{AppState, SessionRecord, now_ms};

const SESSION_TTL_MS: u64 = 30 * 24 * 3600 * 1000; // 30 days
const TIMESTAMP_TOLERANCE_MS: u64 = 15_000;          // 15 seconds anti-replay

#[derive(Debug, Deserialize)]
pub struct ExchangeRequest {
    /// Existing Helm DID that owns this agent
    pub local_did: String,
    /// External identity to bind (did:ethr:0xABC, did:web:..., did:key:...)
    pub global_did: String,
    /// Unix milliseconds — must be within ±15s of server time
    pub timestamp_ms: u64,
    /// base64(ed25519_sign(sha256(timestamp_ms_string + ":" + global_did)))
    /// Signed with the Ed25519 private key corresponding to local_did
    pub signature: String,
}

#[derive(Debug, Serialize)]
pub struct ExchangeResponse {
    /// The internal Helm DID (unchanged)
    pub local_did: String,
    /// The external identity now bound to this DID
    pub global_did: String,
    /// Session token for the next 30 days (use as Bearer token)
    pub session_token: String,
    /// Unix ms when this session expires
    pub expires_at_ms: u64,
    /// How to use: Authorization: Bearer <session_token>
    pub auth_header: String,
    pub message: &'static str,
}

pub async fn handle_exchange(
    State(state): State<AppState>,
    Json(req): Json<ExchangeRequest>,
) -> Result<Json<ExchangeResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Validate DID formats
    if !req.local_did.starts_with("did:helm:") {
        return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_local_did",
            "message": "local_did must be a did:helm: DID"
        }))));
    }
    if req.global_did.trim().is_empty() || req.global_did.len() > 256 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "invalid_global_did",
            "message": "global_did must be a non-empty string ≤256 chars"
        }))));
    }

    // Timestamp anti-replay: must be within ±15s
    let now = now_ms();
    if req.timestamp_ms > now + TIMESTAMP_TOLERANCE_MS {
        return Err((StatusCode::BAD_REQUEST, Json(json!({
            "error": "timestamp_in_future",
            "message": "timestamp_ms is more than 15s in the future"
        }))));
    }
    if now.saturating_sub(req.timestamp_ms) > TIMESTAMP_TOLERANCE_MS {
        return Err((StatusCode::UNAUTHORIZED, Json(json!({
            "error": "signature_expired",
            "message": "timestamp_ms is older than 15s — replay protection"
        }))));
    }

    // Verify local_did is a registered agent
    {
        let agents = state.agents.read().await;
        if !agents.contains_key(&req.local_did) {
            return Err((StatusCode::NOT_FOUND, Json(json!({
                "error": "local_did_not_found",
                "message": "local_did is not registered. Call POST /v1/agent/boot first."
            }))));
        }
    }

    // Verify Ed25519 signature: sha256(timestamp_ms_string + ":" + global_did)
    if !verify_exchange_sig(&req.local_did, &req.signature, req.timestamp_ms, &req.global_did) {
        return Err((StatusCode::UNAUTHORIZED, Json(json!({
            "error": "invalid_signature",
            "message": "Signature verification failed. Sign sha256(timestamp_ms + ':' + global_did) with your Ed25519 private key.",
            "hint": "message = sha256(timestamp_ms.to_string() + ':' + global_did)"
        }))));
    }

    // Idempotent binding: if global_did already mapped, verify it points to same local_did
    {
        let mappings = state.did_mappings.read().await;
        if let Some(existing_local) = mappings.get(&req.global_did) {
            if existing_local != &req.local_did {
                return Err((StatusCode::CONFLICT, Json(json!({
                    "error": "global_did_already_bound",
                    "message": "This global_did is already bound to a different local DID"
                }))));
            }
        }
    }

    // Store mapping (global_did → local_did)
    state.did_mappings.write().await.insert(
        req.global_did.clone(),
        req.local_did.clone(),
    );

    // Generate session token
    let token = generate_session_token();
    let expires_at_ms = now + SESSION_TTL_MS;
    let session_token_full = format!("helm_sess_{}", token);

    state.session_tokens.write().await.insert(
        session_token_full.clone(),
        SessionRecord {
            local_did: req.local_did.clone(),
            expires_at_ms,
        },
    );

    tracing::info!(
        "DID exchange: global_did={} → local_did={}",
        req.global_did, req.local_did
    );

    Ok(Json(ExchangeResponse {
        auth_header: format!("Bearer {}", session_token_full),
        local_did: req.local_did,
        global_did: req.global_did,
        session_token: session_token_full,
        expires_at_ms,
        message: "DID bound. Use session_token as Bearer token for 30 days.",
    }))
}

/// Verify Ed25519 signature over sha256(timestamp_ms_string + ":" + global_did).
fn verify_exchange_sig(local_did: &str, sig_b64: &str, timestamp_ms: u64, global_did: &str) -> bool {
    use base64::Engine;
    use ed25519_dalek::{Signature, VerifyingKey, Verifier};
    use sha2::{Digest, Sha256};

    let pubkey_b58 = match local_did.strip_prefix("did:helm:") {
        Some(pk) => pk,
        None => return false,
    };
    let pubkey_bytes = match bs58::decode(pubkey_b58).into_vec() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let pubkey_arr: [u8; 32] = match pubkey_bytes.try_into() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let verifying_key = match VerifyingKey::from_bytes(&pubkey_arr) {
        Ok(k) => k,
        Err(_) => return false,
    };

    // Build message: sha256(timestamp_ms_string + ":" + global_did)
    let mut hasher = Sha256::new();
    hasher.update(timestamp_ms.to_string().as_bytes());
    hasher.update(b":");
    hasher.update(global_did.as_bytes());
    let message = hasher.finalize();

    let sig_bytes = match base64::engine::general_purpose::STANDARD.decode(sig_b64) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let sig_arr: [u8; 64] = match sig_bytes.try_into() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let signature = Signature::from_bytes(&sig_arr);
    verifying_key.verify(&message, &signature).is_ok()
}

/// Generate a cryptographically random 32-byte session token as lowercase hex.
fn generate_session_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
