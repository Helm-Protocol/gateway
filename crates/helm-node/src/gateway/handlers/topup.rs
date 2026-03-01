//! POST /v1/payment/topup — x402 USDC → VIRTUAL balance topup.
//!
//! ## Flow
//! 1. POST {} or {"tx_hash": null} → 402 with payment requirements.
//! 2. Client sends USDC to treasury on Base mainnet (direct EOA transfer).
//! 3. POST {"tx_hash": "0x..."} → gateway verifies on-chain → credits VIRTUAL.
//!
//! ## Pricing
//! 1 USDC (Base mainnet) = 1.538 VIRTUAL. Minimum: 0.50 USDC.
//!
//! ## Replay Protection
//! Each tx_hash can only be used once (tracked in AppState.topup_txs).

use axum::{extract::State, http::StatusCode, Extension, Json};
use serde::Deserialize;
use serde_json::json;

use crate::gateway::auth::CallerDid;
use crate::gateway::state::AppState;
use crate::gateway::x402::{
    base_rpc_url, payment_required_response, usdc_to_virtual_micro, verify_usdc_topup,
    VerifyError, TREASURY_ADDRESS, USDC_CONTRACT_BASE,
};

#[derive(Deserialize)]
pub struct TopupRequest {
    pub tx_hash: Option<String>,
}

pub async fn handle_topup(
    State(state): State<AppState>,
    Extension(CallerDid(caller_did)): Extension<CallerDid>,
    Json(req): Json<TopupRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let tx_hash = match req.tx_hash.as_deref() {
        None | Some("") => {
            // No tx_hash: return x402 payment requirements
            let payment =
                payment_required_response("https://api.helm.xyz/v1/payment/topup");
            return Err((
                StatusCode::PAYMENT_REQUIRED,
                Json(serde_json::to_value(&payment).unwrap_or_else(|_| {
                    json!({"error": "payment_required"})
                })),
            ));
        }
        Some(h) => h,
    };

    // Replay protection: reject already-credited tx hashes
    {
        let seen = state.topup_txs.read().await;
        if seen.contains(tx_hash) {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "tx_already_used",
                    "tx_hash": tx_hash,
                    "message": "This transaction has already been credited to a VIRTUAL balance."
                })),
            ));
        }
    }

    // Verify USDC transfer to treasury on Base mainnet
    let rpc_url = base_rpc_url();
    let usdc_amount = verify_usdc_topup(tx_hash, &rpc_url)
        .await
        .map_err(|e| {
            let (status, code) = match &e {
                VerifyError::InvalidHash => (StatusCode::BAD_REQUEST, "invalid_tx_hash"),
                VerifyError::TxNotFound => (StatusCode::NOT_FOUND, "tx_not_found"),
                VerifyError::TxFailed => (StatusCode::UNPROCESSABLE_ENTITY, "tx_failed_on_chain"),
                VerifyError::NoQualifyingTransfer => {
                    (StatusCode::UNPROCESSABLE_ENTITY, "no_usdc_transfer_to_treasury")
                }
                VerifyError::BelowMinimum(_) => {
                    (StatusCode::UNPROCESSABLE_ENTITY, "amount_below_minimum")
                }
                VerifyError::RpcError(_) => (StatusCode::BAD_GATEWAY, "rpc_error"),
            };
            (
                status,
                Json(json!({
                    "error": code,
                    "detail": e.to_string(),
                    "treasury": TREASURY_ADDRESS,
                    "asset": USDC_CONTRACT_BASE,
                    "network": "base-mainnet",
                    "minimum_usdc": "0.50"
                })),
            )
        })?;

    let virtual_credited = usdc_to_virtual_micro(usdc_amount);

    // Mark tx as used before crediting (prevents double-spend under concurrent requests)
    state.topup_txs.write().await.insert(tx_hash.to_string());

    // Credit VIRTUAL to agent balance
    let new_balance = {
        let mut agents = state.agents.write().await;
        match agents.get_mut(&caller_did) {
            Some(agent) => {
                agent.virtual_balance =
                    agent.virtual_balance.saturating_add(virtual_credited);
                agent.virtual_balance
            }
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": "agent_not_found", "did": caller_did})),
                ))
            }
        }
    };

    Ok(Json(json!({
        "ok": true,
        "tx_hash": tx_hash,
        "usdc_paid_6dec": usdc_amount,
        "virtual_credited": virtual_credited,
        "new_balance": new_balance,
        "rate": "1 USDC = 1.538 VIRTUAL",
        "treasury": TREASURY_ADDRESS,
        "network": "base-mainnet"
    })))
}
