//! GET/PUT/DEL /v1/sense/memory/:key — E-Line: Sense Memory.
//!
//! Agent-owned persistent key-value store. DID-isolated: each agent can
//! only read/write their own namespace.
//!
//! ## What the strategy doc missed
//!
//! The doc proposed Sense Memory as a new concept, but `helm-store` already
//! implements TieredCache (L1 hot/L2 warm/L3 cold), CRDTs (LWW register,
//! GCounter, ORSet), and Merkle DAG. Sense Memory is literally just an HTTP
//! wrapper around the LWW (Last-Write-Wins) register CRDT, which provides
//! exactly the "session-persistent memory" semantics the doc wants.
//!
//! For now this uses in-memory HashMap. Production version: wire to
//! `helm-store`'s sled backend with per-DID namespacing.
//!
//! ## Key format
//! Internal key: `{did}/{user_key}`
//! User sees only their `user_key` part.
//!
//! ## Quota
//! - Free tier: 100 MB total per DID
//! - Sovereign Agent: 100 GB total per DID

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::gateway::auth::CallerDid;
use crate::gateway::pricing::VIRTUAL_UNIT;
use crate::gateway::state::{AppState, MemoryEntry, now_ms};

const FREE_TIER_QUOTA_BYTES: usize = 100 * 1024 * 1024; // 100 MB
const MAX_VALUE_SIZE_BYTES: usize = 1024 * 1024; // 1 MB per entry

#[derive(Debug, Deserialize)]
pub struct PutMemoryRequest {
    pub value: serde_json::Value,
    /// TTL in milliseconds. 0 = permanent. Max: 30 days.
    #[serde(default)]
    pub ttl_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct MemoryGetResponse {
    pub key: String,
    pub value: serde_json::Value,
    pub updated_at_ms: u64,
    pub ttl_ms: u64,
    pub size_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct MemoryPutResponse {
    pub key: String,
    pub size_bytes: usize,
    pub virtual_charged: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct MemoryListResponse {
    pub keys: Vec<String>,
    pub total_keys: usize,
    pub total_bytes_used: usize,
    pub quota_bytes: usize,
    pub quota_pct: f64,
}

/// GET /v1/sense/memory/:key
pub async fn handle_memory_get(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Path(key): Path<String>,
) -> Result<Json<MemoryGetResponse>, (StatusCode, Json<serde_json::Value>)> {
    let internal_key = format!("{}/{}", did, key);

    let memory = state.memory.read().await;
    let entry = memory.get(&internal_key).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "key_not_found", "key": key})),
        )
    })?;

    // Check TTL expiry
    if entry.ttl_ms > 0 {
        let age = now_ms().saturating_sub(entry.updated_at_ms);
        if age > entry.ttl_ms {
            drop(memory);
            state.memory.write().await.remove(&internal_key);
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "key_expired", "key": key})),
            ));
        }
    }

    // Charge a tiny read fee
    let read_cost = 100u64; // 0.0001 VIRTUAL
    drop(memory);
    state.record_api_call(&did, "sense/memory/read", read_cost).await;

    let memory = state.memory.read().await;
    let entry = memory.get(&internal_key).unwrap();
    Ok(Json(MemoryGetResponse {
        key: key.clone(),
        value: entry.value.clone(),
        updated_at_ms: entry.updated_at_ms,
        ttl_ms: entry.ttl_ms,
        size_bytes: entry.size_bytes,
    }))
}

/// PUT /v1/sense/memory/:key
pub async fn handle_memory_put(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Path(key): Path<String>,
    Json(req): Json<PutMemoryRequest>,
) -> Result<Json<MemoryPutResponse>, (StatusCode, Json<serde_json::Value>)> {
    let serialized = serde_json::to_vec(&req.value).unwrap_or_default();
    let size_bytes = serialized.len();

    if size_bytes > MAX_VALUE_SIZE_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({
                "error": "value_too_large",
                "max_bytes": MAX_VALUE_SIZE_BYTES,
                "provided_bytes": size_bytes
            })),
        ));
    }

    // Check quota
    let internal_key = format!("{}/{}", did, key);
    {
        let memory = state.memory.read().await;
        let current_usage: usize = memory
            .iter()
            .filter(|(k, _)| k.starts_with(&format!("{}/", did)))
            .map(|(_, v)| v.size_bytes)
            .sum();

        if current_usage + size_bytes > FREE_TIER_QUOTA_BYTES {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "error": "quota_exceeded",
                    "used_bytes": current_usage,
                    "quota_bytes": FREE_TIER_QUOTA_BYTES,
                    "hint": "Upgrade to Sovereign Agent (500 VIRTUAL/month) for 100GB quota"
                })),
            ));
        }
    }

    let ttl_ms = req.ttl_ms.min(30 * 24 * 3600 * 1000); // Max 30 days
    let now = now_ms();

    let entry = MemoryEntry {
        value: req.value,
        size_bytes,
        updated_at_ms: now,
        ttl_ms,
    };

    state.memory.write().await.insert(internal_key, entry);

    // Write fee: 50_000 VIRTUAL micro-units = 0.05 VIRTUAL per write
    let write_cost = 50_000u64;
    state.record_api_call(&did, "sense/memory/write", write_cost).await;

    Ok(Json(MemoryPutResponse {
        key,
        size_bytes,
        virtual_charged: write_cost,
        updated_at_ms: now,
    }))
}

/// DELETE /v1/sense/memory/:key
pub async fn handle_memory_del(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
    Path(key): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let internal_key = format!("{}/{}", did, key);
    let removed = state.memory.write().await.remove(&internal_key);

    if removed.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "key_not_found", "key": key})),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// GET /v1/sense/memory — list all keys for this DID
pub async fn handle_memory_list(
    State(state): State<AppState>,
    Extension(CallerDid(did)): Extension<CallerDid>,
) -> Json<MemoryListResponse> {
    let prefix = format!("{}/", did);
    let memory = state.memory.read().await;

    let (keys, total_bytes): (Vec<String>, usize) = memory
        .iter()
        .filter(|(k, _)| k.starts_with(&prefix))
        .map(|(k, v)| (k.trim_start_matches(&prefix).to_string(), v.size_bytes))
        .fold((Vec::new(), 0), |(mut keys, bytes), (key, size)| {
            keys.push(key);
            (keys, bytes + size)
        });

    let total_keys = keys.len();
    let quota_pct = total_bytes as f64 / FREE_TIER_QUOTA_BYTES as f64 * 100.0;

    Json(MemoryListResponse {
        keys,
        total_keys,
        total_bytes_used: total_bytes,
        quota_bytes: FREE_TIER_QUOTA_BYTES,
        quota_pct,
    })
}
