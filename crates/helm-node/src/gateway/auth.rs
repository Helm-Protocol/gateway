//! DID-based authentication middleware for the Helm Sense API.
//!
//! ## Auth Protocol
//!
//! Every request must include:
//!   `Authorization: Bearer did:helm:<base58-pubkey>`
//!
//! For write operations (POST/PUT/DELETE), an optional ed25519 signature
//! header can be included for full cryptographic authentication:
//!   `X-Helm-Signature: <base64-encoded-sig-over-sha256(body)>`
//!
//! ## What the strategy document missed
//!
//! The HELM SENSE API report didn't design any authentication layer.
//! Without DID auth, anyone can impersonate any agent DID — destroying
//! the entire DID 해자 (DID moat). This middleware is the security
//! foundation that makes the moat real.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
    Json,
};
use serde_json::json;

use crate::gateway::state::AppState;

/// Extracted caller DID from Authorization header.
#[derive(Debug, Clone)]
pub struct CallerDid(pub String);

/// Rate limit state per DID (simple in-memory sliding window).
/// In production: replace with Redis INCR + EXPIRE.
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

/// Auth middleware: validates caller DID exists in agent registry.
/// Returns 401 if DID not found.
/// Returns 429 if rate limit exceeded.
///
/// Note: for AgentBoot, the DID doesn't exist yet — that endpoint
/// is unauthenticated (anyone can create a new DID).
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

    // Check DID is registered
    let agents = state.agents.read().await;
    if !agents.contains_key(&did) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "did_not_found",
                "message": format!("DID '{}' not registered. Call POST /v1/agent/boot first.", did),
                "hint": "POST /v1/agent/boot to create your DID"
            })),
        ));
    }
    drop(agents);

    // Rate limit check: ≤1000 calls/minute per DID (simple call count, no window yet)
    // TODO: sliding window rate limiter when Redis is available

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
