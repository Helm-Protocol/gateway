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
//! Sliding window: ≤30 calls per 60 seconds per DID.
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

/// Per-DID request rate limit. 30/min: normal agents use 5-10 req/min;
/// 30 is generous for legitimate use while making automated scanning expensive.
pub const RATE_LIMIT_MAX: usize = 30;
const RATE_LIMIT_WINDOW_MS: u64 = 60_000; // 60 seconds

/// Resolve a Bearer token to a did:helm: DID string.
/// Supports three formats:
///   1. "Bearer did:helm:xxx"            — direct DID bearer (original)
///   2. "Bearer helm_sess_<hex>"         — session token from /v1/auth/exchange
///   3. "Bearer did:helm:xxx:role"       — hierarchical sub-DID (role verified separately)
///
/// Returns the resolved base did:helm: DID.
/// Session token resolution is async (requires state), so callers that need it
/// use `resolve_bearer_token` instead. This sync version handles cases 1 and 3.
pub fn extract_did_from_headers(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get("authorization")?;
    let val = auth.to_str().ok()?;

    let token = val
        .strip_prefix("Bearer ")
        .unwrap_or(val)
        .trim();

    if token.starts_with("did:helm:") {
        // Sub-DID routing: "did:helm:xxx:pool_3" → base = "did:helm:xxx"
        // The role portion after the 3rd colon is used for endpoint-level ACL checks.
        let parts: Vec<&str> = token.splitn(4, ':').collect();
        if parts.len() >= 3 {
            // Reconstruct base DID (first 3 segments)
            Some(format!("{}:{}:{}", parts[0], parts[1], parts[2]))
        } else {
            Some(token.to_string())
        }
    } else if token.starts_with("helm_sess_") {
        // Session token: resolution requires async state lookup — signal to middleware
        // by returning a sentinel. The middleware calls resolve_session_token().
        Some(format!("__sess__{}", token))
    } else {
        None
    }
}

/// Verify an ed25519 signature over the request body (legacy: no timestamp).
/// Header format: `X-Helm-Signature: <base64(ed25519_sig_over_sha256(body))>`
fn verify_signature(did: &str, sig_b64: &str, body: &[u8]) -> bool {
    use sha2::{Digest, Sha256};
    let body_hash = Sha256::digest(body);
    _verify_signature_raw(did, sig_b64, &body_hash)
}

/// Verify signature with timestamp anti-replay.
/// Signs sha256(timestamp_ms_as_string + ":" + body).
fn verify_signature_with_timestamp(
    did: &str,
    sig_b64: &str,
    timestamp_ms: u64,
    body: &[u8],
) -> bool {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(timestamp_ms.to_string().as_bytes());
    hasher.update(b":");
    hasher.update(body);
    let payload_hash = hasher.finalize();
    // Reuse verify_signature by passing the combined hash as "body"
    // (verify_signature hashes body again, so pass raw hash as input)
    _verify_signature_raw(did, sig_b64, &payload_hash)
}

/// Raw signature verification over pre-computed bytes (no additional hashing).
fn _verify_signature_raw(did: &str, sig_b64: &str, message: &[u8]) -> bool {
    use base64::Engine;
    use ed25519_dalek::{Signature, VerifyingKey};

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
    use ed25519_dalek::Verifier;
    verifying_key.verify(message, &signature).is_ok()
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
    let raw_token = extract_did_from_headers(request.headers())
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

    // Resolve session tokens (helm_sess_<hex>) to their bound DID
    let did = if let Some(sess_token) = raw_token.strip_prefix("__sess__") {
        let sessions = state.session_tokens.read().await;
        match sessions.get(sess_token) {
            Some(rec) if rec.expires_at_ms > now_ms() => rec.local_did.clone(),
            Some(_) => return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "session_expired",
                    "message": "Session token has expired. Re-authenticate via POST /v1/auth/exchange.",
                })),
            )),
            None => return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "invalid_session_token",
                    "message": "Session token not found. Obtain one via POST /v1/auth/exchange.",
                })),
            )),
        }
    } else {
        raw_token
    };

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

        // Anti-replay: validate X-Helm-Timestamp if present (must be within ±15s)
        // Protocol: sign sha256(timestamp_ms_str + ":" + body_bytes) when timestamp provided.
        // Without timestamp: old scheme (sha256(body) only) — accepted but replay-vulnerable.
        let timestamp_opt = request.headers()
            .get("x-helm-timestamp")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        if let Some(ts_ms) = timestamp_opt {
            let now = now_ms();
            // 15s tolerance: enough for network latency while closing the replay window.
            // AWS SigV4 uses 5min for compatibility; we're stricter (DeFi speed matters).
            const TIMESTAMP_TOLERANCE_MS: u64 = 15_000; // 15 seconds
            if ts_ms > now + TIMESTAMP_TOLERANCE_MS {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "timestamp_in_future",
                        "message": "X-Helm-Timestamp is more than 15s in the future. Check server clock skew.",
                    })),
                ));
            }
            if now.saturating_sub(ts_ms) > TIMESTAMP_TOLERANCE_MS {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "error": "signature_expired",
                        "message": "X-Helm-Timestamp is older than 15s — possible replay attack.",
                        "hint": "Always include a fresh X-Helm-Timestamp with each signed request.",
                    })),
                ));
            }
        } else if is_write {
            tracing::warn!(
                "Signed write without X-Helm-Timestamp: did={} — replay attacks possible",
                did
            );
        }

        // Buffer request body for signature verification
        let (parts, body) = request.into_parts();
        let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
            Ok(b) => b,
            Err(_) => return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "body_read_error", "message": "Failed to read request body for signature verification"})),
            )),
        };

        // Verify signature: new protocol includes timestamp in signed payload
        let sig_ok = if let Some(ts_ms) = timestamp_opt {
            // New: sign sha256(timestamp_ms_string + ":" + body)
            verify_signature_with_timestamp(&did, &sig_b64, ts_ms, &body_bytes)
        } else {
            // Legacy: sign sha256(body) only
            verify_signature(&did, &sig_b64, &body_bytes)
        };

        if !sig_ok {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "invalid_signature",
                    "message": "X-Helm-Signature verification failed. Sign sha256(timestamp+\":\"+body) with your ed25519 private key.",
                    "did": did,
                    "hint": "Include X-Helm-Timestamp header for anti-replay protection.",
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
    if let Some(raw_token) = extract_did_from_headers(request.headers()) {
        // Resolve session token if needed
        let did = if let Some(sess_token) = raw_token.strip_prefix("__sess__") {
            let sessions = state.session_tokens.read().await;
            sessions.get(sess_token)
                .filter(|r| r.expires_at_ms > now_ms())
                .map(|r| r.local_did.clone())
        } else {
            Some(raw_token)
        };

        if let Some(did) = did {
            let agents = state.agents.read().await;
            if agents.contains_key(&did) {
                drop(agents);
                request.extensions_mut().insert(CallerDid(did));
            }
        }
    }
    next.run(request).await
}
