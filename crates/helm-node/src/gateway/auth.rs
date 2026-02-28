//! DID-based authentication middleware for the Helm Sense API.
//!
//! ## Auth Protocol
//!
//! Every request must include:
//!   `Authorization: Bearer did:helm:<base58-pubkey>`
//!
//! For write operations (POST/PUT/DELETE), an ed25519 signature header
//! can be included for full cryptographic authentication:
//!   `X-Helm-Signature: <base64-encoded-sig-over-sha256(body)>`
//!
//! If the signature header is present, it is ALWAYS verified.
//! Verification failure → 401 Unauthorized.
//!
//! ## Rate Limiting
//!
//! Sliding window: ≤100 calls per 60 seconds per DID.
//! Tracked in AppState::rate_limits (DID → Vec<timestamp_ms>).

use axum::{
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::Response,
    Json,
};
use serde_json::json;

use crate::gateway::state::{AppState, now_ms};

/// Extracted caller DID from Authorization header.
#[derive(Debug, Clone)]
pub struct CallerDid(pub String);

const RATE_LIMIT_MAX: usize = 100;
const RATE_LIMIT_WINDOW_MS: u64 = 60_000; // 60 seconds

pub fn extract_did_from_headers(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get("authorization")?;
    let val = auth.to_str().ok()?;

    // Accept: "Bearer did:helm:..." or just "did:helm:..."
    let token = val
        .strip_prefix("Bearer ")
        .unwrap_or(val)
        .trim();

    if token.starts_with("did:helm:") {
        Some(token.to_string())
    } else {
        None
    }
}

/// Verify an ed25519 signature over the request body.
///
/// Header format: `X-Helm-Signature: <base64(ed25519_sig_over_sha256(body))>`
/// The public key is the base58-encoded part of the DID: `did:helm:<pubkey_b58>`.
fn verify_signature(
    did: &str,
    sig_b64: &str,
    body: &[u8],
) -> bool {
    use base64::Engine;
    use ed25519_dalek::{Signature, VerifyingKey};
    use sha2::{Digest, Sha256};

    // Extract public key from DID: "did:helm:<base58_pubkey>"
    let pubkey_b58 = match did.strip_prefix("did:helm:") {
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

    let sig_bytes = match base64::engine::general_purpose::STANDARD.decode(sig_b64) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let sig_arr: [u8; 64] = match sig_bytes.try_into() {
        Ok(a) => a,
        Err(_) => return false,
    };

    let signature = Signature::from_bytes(&sig_arr);

    // Verify signature over sha256(body)
    let body_hash = Sha256::digest(body);
    use ed25519_dalek::Verifier;
    verifying_key.verify(&body_hash, &signature).is_ok()
}

/// Auth middleware: validates caller DID exists in agent registry.
///
/// - Returns 401 if DID not found.
/// - Returns 401 if X-Helm-Signature present but invalid.
/// - Returns 429 if rate limit exceeded (100 req/60s per DID).
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let did = extract_did_from_headers(request.headers())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "missing_auth",
                    "message": "Include 'Authorization: Bearer did:helm:<your-did>' header",
                    "hint": "Run 'helm init' to get your DID, then use it as the Bearer token"
                })),
            )
        })?;

    // Verify DID is registered
    let pubkey_b58 = {
        let agents = state.agents.read().await;
        match agents.get(&did) {
            Some(a) => a.public_key_b58.clone(),
            None => return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "did_not_found",
                    "message": format!("DID '{}' not registered. Call POST /v1/agent/boot first.", did),
                    "hint": "POST /v1/agent/boot to create your DID"
                })),
            )),
        }
    };

    // Verify ed25519 signature if header is present
    // For write operations (POST/PUT/DELETE), the signature header is strongly recommended.
    // If the header is absent on write ops, log a warning but allow (backward compat).
    // If the header is present and INVALID, always reject regardless of method.
    let is_write = matches!(
        request.method(),
        &Method::POST | &Method::PUT | &Method::DELETE | &Method::PATCH
    );

    if let Some(sig_header) = request.headers().get("x-helm-signature") {
        let sig_b64 = sig_header.to_str().unwrap_or("").to_string();

        // Buffer request body for signature verification
        let (parts, body) = request.into_parts();
        let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
            Ok(b) => b,
            Err(_) => return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "body_read_error", "message": "Failed to read request body for signature verification"})),
            )),
        };

        if !verify_signature(&did, &sig_b64, &body_bytes) {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "invalid_signature",
                    "message": "X-Helm-Signature verification failed. Sign sha256(body) with your ed25519 private key.",
                    "did": did,
                })),
            ));
        }

        // Rebuild request with buffered body
        request = Request::from_parts(parts, axum::body::Body::from(body_bytes));
    } else if is_write {
        // Write op without signature: allow for backward compat, but log
        tracing::warn!(
            "Write operation without X-Helm-Signature: did={} method={} path={}",
            did,
            request.method(),
            request.uri().path(),
        );
        let _ = pubkey_b58; // suppress unused warning
    }

    // Rate limit: sliding window 100 req / 60s per DID
    {
        let mut rl = state.rate_limits.write().await;
        let now = now_ms();
        let window_start = now.saturating_sub(RATE_LIMIT_WINDOW_MS);
        let timestamps = rl.entry(did.clone()).or_insert_with(Vec::new);

        // Remove old entries outside the window
        timestamps.retain(|&ts| ts > window_start);

        if timestamps.len() >= RATE_LIMIT_MAX {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": "rate_limit_exceeded",
                    "message": format!("Max {} requests per 60 seconds per DID", RATE_LIMIT_MAX),
                    "retry_after_ms": RATE_LIMIT_WINDOW_MS,
                })),
            ));
        }

        timestamps.push(now);
    }

    // Inject caller DID into request extensions for handlers
    request.extensions_mut().insert(CallerDid(did));

    Ok(next.run(request).await)
}

/// Middleware that allows unauthenticated requests but extracts DID if present.
/// Used for endpoints like /v1/agent/boot and GET /v1/leaderboard.
pub async fn optional_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(did) = extract_did_from_headers(request.headers()) {
        let agents = state.agents.read().await;
        if agents.contains_key(&did) {
            drop(agents);
            request.extensions_mut().insert(CallerDid(did));
        }
    }
    next.run(request).await
}
