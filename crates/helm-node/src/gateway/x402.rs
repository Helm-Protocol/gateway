//! x402 HTTP payment protocol — Base mainnet USDC → Helm VIRTUAL credits.
//!
//! ## Flow
//! 1. Agent has low/zero VIRTUAL balance.
//! 2. Agent calls any paid endpoint → 402 response with payment requirements.
//! 3. Agent sends USDC to treasury on Base mainnet (no smart contract needed).
//! 4. Agent calls POST /v1/payment/topup with the on-chain tx hash.
//! 5. Gateway verifies tx via Base RPC (eth_getTransactionReceipt).
//! 6. Gateway credits VIRTUAL to agent balance (1 USDC = 1.538 VIRTUAL).
//!
//! ## Treasury
//! All payments go to Jay's EOA wallet — no contract deployment required.
//! Verification is read-only (only calls eth_getTransactionReceipt).

use serde::{Deserialize, Serialize};

/// Jay's wallet — all API payments route here.
pub const TREASURY_ADDRESS: &str = "0x7e0118A33202c03949167853b05631baC0fA9756";

/// USDC contract on Base mainnet.
pub const USDC_CONTRACT_BASE: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";

/// Base mainnet chain ID.
pub const BASE_CHAIN_ID: u64 = 8453;

/// Default Base mainnet public RPC.
pub const BASE_RPC_DEFAULT: &str = "https://mainnet.base.org";

/// ERC-20 Transfer(address,address,uint256) event signature.
/// keccak256("Transfer(address,address,uint256)")
pub const ERC20_TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

/// Minimum topup: 0.50 USDC (500_000 in 6-decimal USDC units).
pub const MIN_TOPUP_USDC_6DEC: u64 = 500_000;

/// Exchange rate numerator: VIRTUAL micro-units per USDC_6DEC unit.
/// 1 USDC ($1.00) / $0.65 per VIRTUAL = 1.538 VIRTUAL = 1_538 μVIRTUAL per USDC_6DEC
const RATE_NUM: u64 = 1538;
const RATE_DEN: u64 = 1000;

/// Convert USDC (6-decimal units) → VIRTUAL micro-units (1 VIRTUAL = 1_000_000 μV).
///
/// 1 USDC (1_000_000 USDC_6DEC) → 1_538_000 μVIRTUAL = 1.538 VIRTUAL
pub fn usdc_to_virtual_micro(usdc_6dec: u64) -> u64 {
    usdc_6dec.saturating_mul(RATE_NUM) / RATE_DEN
}

/// Get Base RPC URL from env var `HELM_BASE_RPC_URL`, or use the public default.
pub fn base_rpc_url() -> String {
    std::env::var("HELM_BASE_RPC_URL").unwrap_or_else(|_| BASE_RPC_DEFAULT.to_string())
}

// ── Standard x402 payment-required response ───────────────────────────────

/// Standard x402 HTTP 402 response body sent to clients.
#[derive(Serialize)]
pub struct X402PaymentRequired {
    #[serde(rename = "x402Version")]
    pub x402_version: u8,
    pub accepts: Vec<X402PaymentOption>,
    pub error: &'static str,
}

/// A single payment option within the x402 402 response.
#[derive(Serialize)]
pub struct X402PaymentOption {
    /// Payment scheme: "exact" means fixed amount, no auction.
    pub scheme: &'static str,
    pub network: &'static str,
    /// Suggested minimum amount in asset's native decimals (as string).
    #[serde(rename = "maxAmountRequired")]
    pub max_amount_required: &'static str,
    /// The endpoint URL this payment unlocks.
    pub resource: String,
    pub description: &'static str,
    #[serde(rename = "mimeType")]
    pub mime_type: &'static str,
    /// Recipient address (Jay's treasury EOA on Base).
    #[serde(rename = "payTo")]
    pub pay_to: &'static str,
    #[serde(rename = "maxTimeoutSeconds")]
    pub max_timeout_seconds: u32,
    /// ERC-20 asset address (USDC on Base).
    pub asset: &'static str,
    pub extra: X402Extra,
}

/// Asset metadata for the payment option.
#[derive(Serialize)]
pub struct X402Extra {
    pub name: &'static str,
    pub decimals: u8,
}

/// Build the standard 402 payment-required response body.
pub fn payment_required_response(resource_url: &str) -> X402PaymentRequired {
    X402PaymentRequired {
        x402_version: 1,
        accepts: vec![X402PaymentOption {
            scheme: "exact",
            network: "base-mainnet",
            max_amount_required: "1000000", // suggested: 1.00 USDC (minimum 0.50)
            resource: resource_url.to_string(),
            description: "Top up Helm VIRTUAL credits. 1 USDC = 1.538 VIRTUAL. Min: 0.50 USDC.",
            mime_type: "application/json",
            pay_to: TREASURY_ADDRESS,
            max_timeout_seconds: 300,
            asset: USDC_CONTRACT_BASE,
            extra: X402Extra {
                name: "USD Coin",
                decimals: 6,
            },
        }],
        error: "Payment Required — send USDC to treasury on Base, then retry with X-Payment: <txHash> or POST /v1/payment/topup",
    }
}

// ── On-chain verification ─────────────────────────────────────────────────

/// Error variants for tx verification.
#[derive(Debug)]
pub enum VerifyError {
    /// tx_hash format invalid (must be 0x + 64 hex chars)
    InvalidHash,
    /// RPC network error
    RpcError(String),
    /// Transaction not found on chain (may be pending or wrong network)
    TxNotFound,
    /// Transaction was mined but reverted
    TxFailed,
    /// No USDC Transfer to treasury found in this transaction
    NoQualifyingTransfer,
    /// USDC amount is below minimum
    BelowMinimum(u64),
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHash => write!(f, "invalid_tx_hash"),
            Self::RpcError(e) => write!(f, "rpc_error: {e}"),
            Self::TxNotFound => write!(f, "tx_not_found"),
            Self::TxFailed => write!(f, "tx_failed_on_chain"),
            Self::NoQualifyingTransfer => write!(f, "no_usdc_transfer_to_treasury"),
            Self::BelowMinimum(m) => write!(f, "below_minimum_{m}_usdc_6dec"),
        }
    }
}

#[derive(Deserialize)]
struct RpcResponse {
    result: Option<serde_json::Value>,
}

/// Verify a USDC transfer to treasury on Base mainnet.
///
/// Calls `eth_getTransactionReceipt` on `rpc_url`, scans logs for a
/// `Transfer(from, treasury, amount)` event on the USDC contract.
///
/// Returns the USDC amount in 6-decimal units (e.g. 1_000_000 = 1.00 USDC).
pub async fn verify_usdc_topup(tx_hash: &str, rpc_url: &str) -> Result<u64, VerifyError> {
    // Format check: "0x" + 64 hex chars
    if tx_hash.len() != 66
        || !tx_hash.starts_with("0x")
        || !tx_hash[2..].chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(VerifyError::InvalidHash);
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| VerifyError::RpcError(e.to_string()))?;

    let rpc_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
        "id": 1
    });

    let rpc_resp: RpcResponse = client
        .post(rpc_url)
        .json(&rpc_body)
        .send()
        .await
        .map_err(|e| VerifyError::RpcError(e.to_string()))?
        .json()
        .await
        .map_err(|e| VerifyError::RpcError(e.to_string()))?;

    let receipt = match rpc_resp.result {
        None | Some(serde_json::Value::Null) => return Err(VerifyError::TxNotFound),
        Some(v) => v,
    };

    // status: "0x1" = success, "0x0" = reverted
    let status = receipt["status"].as_str().unwrap_or("0x0");
    if status != "0x1" {
        return Err(VerifyError::TxFailed);
    }

    let logs = receipt["logs"]
        .as_array()
        .ok_or(VerifyError::NoQualifyingTransfer)?;

    let treasury_lower = TREASURY_ADDRESS.to_lowercase();
    let usdc_lower = USDC_CONTRACT_BASE.to_lowercase();

    for log in logs {
        // Must be emitted by the USDC contract
        let contract = log["address"].as_str().unwrap_or("").to_lowercase();
        if contract != usdc_lower {
            continue;
        }

        let topics = match log["topics"].as_array() {
            Some(t) if t.len() >= 3 => t,
            _ => continue,
        };

        // topics[0] must be Transfer event signature
        if topics[0].as_str().unwrap_or("") != ERC20_TRANSFER_TOPIC {
            continue;
        }

        // topics[2] = ABI-padded `to` address: "0x000...0<40 hex chars>"
        let to_topic = topics[2].as_str().unwrap_or("");
        if to_topic.len() != 66 {
            continue;
        }
        let to_addr = format!("0x{}", &to_topic[26..]);
        if to_addr.to_lowercase() != treasury_lower {
            continue;
        }

        // data = uint256 amount in 32-byte big-endian hex (ERC-20 Transfer)
        let data = log["data"].as_str().unwrap_or("");
        let amount_hex = if data.starts_with("0x") && data.len() == 66 {
            &data[2..]
        } else {
            continue;
        };

        let amount = u64::from_str_radix(amount_hex, 16)
            .map_err(|_| VerifyError::NoQualifyingTransfer)?;

        if amount < MIN_TOPUP_USDC_6DEC {
            return Err(VerifyError::BelowMinimum(MIN_TOPUP_USDC_6DEC));
        }

        return Ok(amount);
    }

    Err(VerifyError::NoQualifyingTransfer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usdc_to_virtual_conversion() {
        // 1 USDC (1_000_000 units) → 1_538_000 μVIRTUAL = 1.538 VIRTUAL
        assert_eq!(usdc_to_virtual_micro(1_000_000), 1_538_000);

        // 0.50 USDC → 0.769 VIRTUAL
        assert_eq!(usdc_to_virtual_micro(500_000), 769_000);

        // 10 USDC → 15.38 VIRTUAL
        assert_eq!(usdc_to_virtual_micro(10_000_000), 15_380_000);

        // Zero
        assert_eq!(usdc_to_virtual_micro(0), 0);
    }

    #[test]
    fn test_payment_required_response_structure() {
        let r = payment_required_response("https://api.helm.xyz/v1/payment/topup");
        assert_eq!(r.x402_version, 1);
        assert_eq!(r.accepts.len(), 1);
        let opt = &r.accepts[0];
        assert_eq!(opt.pay_to, TREASURY_ADDRESS);
        assert_eq!(opt.asset, USDC_CONTRACT_BASE);
        assert_eq!(opt.network, "base-mainnet");
        assert_eq!(opt.extra.decimals, 6);
    }

    #[tokio::test]
    async fn test_verify_rejects_invalid_hash_format() {
        assert!(matches!(
            verify_usdc_topup("not_a_hash", "http://unused").await,
            Err(VerifyError::InvalidHash)
        ));
        assert!(matches!(
            verify_usdc_topup("0x1234", "http://unused").await,
            Err(VerifyError::InvalidHash)
        ));
        // Missing 0x prefix but correct length
        assert!(matches!(
            verify_usdc_topup(
                "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
                "http://unused"
            )
            .await,
            Err(VerifyError::InvalidHash)
        ));
    }
}
