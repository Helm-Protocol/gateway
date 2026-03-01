//! x402 HTTP payment protocol — BNKR (primary) or USDC (secondary) → Helm VIRTUAL credits.
//!
//! ## Payment Asset Priority
//!
//! 1. **BNKR** (primary — Base mainnet, EIP-3009 supported)
//!    - Coinbase x402 standard Facilitator works directly: gasless transferWithAuthorization
//!    - No Swap required. Native token for the Helm / Virtual Protocol agent ecosystem.
//!    - 18 decimals on-chain. Rate: 1 BNKR = 0.00055 USD → ~847 μVIRTUAL per BNKR
//!    - Set `HELM_BNKR_CONTRACT` env var to activate.
//!
//! 2. **USDC** (secondary — Base mainnet, EIP-3009 NOT supported for USDC on Base)
//!    - Direct EOA transfer to treasury (agent pays gas).
//!    - 6 decimals on-chain. Rate: 1 USDC = 1.538 VIRTUAL.
//!
//! 3. **VIRTUAL holders**: Swap via Aerodrome VIRTUAL→BNKR → pay with BNKR.
//!
//! ## Flow (both assets)
//! 1. Agent calls paid endpoint with zero balance → 402 with payment requirements.
//! 2. Agent sends BNKR or USDC to treasury on Base mainnet.
//!    (BNKR: agent uses EIP-3009 gasless transfer or direct ERC-20 transfer)
//! 3. Agent calls POST /v1/payment/topup { "tx_hash": "0x...", "currency": "BNKR"|"USDC" }
//! 4. Gateway verifies tx via Base RPC (eth_getTransactionReceipt).
//! 5. Gateway credits VIRTUAL to agent balance.
//!
//! ## Treasury
//! All payments go to Jay's EOA wallet — no contract deployment required.

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

/// Minimum topup: 500 whole BNKR ≈ $0.275 (BNKR at $0.00055).
/// On-chain: 500 * 10^18 wei-BNKR. We verify in whole-BNKR units after dividing by 10^18.
pub const MIN_TOPUP_BNKR_WHOLE: u64 = 500;

/// BNKR token decimals (standard ERC-20 18 decimals).
const BNKR_DECIMALS: u128 = 1_000_000_000_000_000_000; // 10^18

/// Exchange rate for USDC → VIRTUAL.
/// 1 USDC ($1.00) / $0.65 per VIRTUAL = 1.538 VIRTUAL = 1_538 μVIRTUAL per USDC_6DEC
const USDC_RATE_NUM: u64 = 1538;
const USDC_RATE_DEN: u64 = 1000;

/// BNKR → VIRTUAL rate constants.
/// 1 BNKR = $0.00055 / $0.65 VIRTUAL ≈ 0.000846 VIRTUAL = 846 μVIRTUAL per whole BNKR
const BNKR_USD: f64 = 0.00055;
const VIRTUAL_USD: f64 = 0.65;

/// Get the BNKR contract address on Base mainnet from env var.
/// Set `HELM_BNKR_CONTRACT` to the verified BNKR contract address.
/// If unset, BNKR topup is disabled (402 response will only show USDC option).
pub fn bnkr_contract_base() -> Option<String> {
    std::env::var("HELM_BNKR_CONTRACT").ok()
}

/// Get Base RPC URL from env var `HELM_BASE_RPC_URL`, or use the public default.
pub fn base_rpc_url() -> String {
    std::env::var("HELM_BASE_RPC_URL").unwrap_or_else(|_| BASE_RPC_DEFAULT.to_string())
}

/// Convert USDC (6-decimal units) → VIRTUAL micro-units (1 VIRTUAL = 1_000_000 μV).
///
/// 1 USDC (1_000_000 USDC_6DEC) → 1_538_000 μVIRTUAL = 1.538 VIRTUAL
pub fn usdc_to_virtual_micro(usdc_6dec: u64) -> u64 {
    usdc_6dec.saturating_mul(USDC_RATE_NUM) / USDC_RATE_DEN
}

/// Convert whole BNKR → VIRTUAL micro-units.
///
/// 1 BNKR = ($0.00055 / $0.65) VIRTUAL ≈ 0.000846 VIRTUAL = 846 μVIRTUAL
/// 1,000 BNKR ≈ 0.846 VIRTUAL (micro-agents use fractional VIRTUAL)
pub fn bnkr_whole_to_virtual_micro(bnkr_whole: u64) -> u64 {
    let virtual_per_bnkr = BNKR_USD / VIRTUAL_USD; // ~0.000846
    (bnkr_whole as f64 * virtual_per_bnkr * 1_000_000.0) as u64
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
    pub max_amount_required: String,
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
    /// ERC-20 asset address.
    pub asset: String,
    pub extra: X402Extra,
}

/// Asset metadata for the payment option.
#[derive(Serialize)]
pub struct X402Extra {
    pub name: &'static str,
    pub decimals: u8,
    /// EIP-3009 support: "transferWithAuthorization" available (gasless via Facilitator)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eip3009: Option<bool>,
}

/// Build the standard 402 payment-required response body.
///
/// Includes BNKR as primary option (if `HELM_BNKR_CONTRACT` is set) and USDC as fallback.
pub fn payment_required_response(resource_url: &str) -> X402PaymentRequired {
    let mut accepts = Vec::new();

    // Primary: BNKR (EIP-3009 supported → Coinbase x402 Facilitator works directly)
    if let Some(bnkr_contract) = bnkr_contract_base() {
        accepts.push(X402PaymentOption {
            scheme: "exact",
            network: "base-mainnet",
            // Suggested: 1000 BNKR ≈ $0.55 (min: 500 BNKR ≈ $0.275)
            // Expressed in wei-BNKR (18 decimals): 1000 * 10^18 = "1000000000000000000000"
            max_amount_required: "1000000000000000000000".to_string(),
            resource: resource_url.to_string(),
            description: "Top up Helm VIRTUAL credits with BNKR. ~0.846 VIRTUAL per 1,000 BNKR. Min: 500 BNKR.",
            mime_type: "application/json",
            pay_to: TREASURY_ADDRESS,
            max_timeout_seconds: 300,
            asset: bnkr_contract,
            extra: X402Extra {
                name: "BNKR",
                decimals: 18,
                eip3009: Some(true),
            },
        });
    }

    // Secondary: USDC (direct transfer, EIP-3009 not supported on Base USDC)
    accepts.push(X402PaymentOption {
        scheme: "exact",
        network: "base-mainnet",
        max_amount_required: "1000000".to_string(), // 1.00 USDC (min: 0.50 USDC)
        resource: resource_url.to_string(),
        description: "Top up Helm VIRTUAL credits with USDC. 1 USDC = 1.538 VIRTUAL. Min: 0.50 USDC.",
        mime_type: "application/json",
        pay_to: TREASURY_ADDRESS,
        max_timeout_seconds: 300,
        asset: USDC_CONTRACT_BASE.to_string(),
        extra: X402Extra {
            name: "USD Coin",
            decimals: 6,
            eip3009: None,
        },
    });

    X402PaymentRequired {
        x402_version: 1,
        accepts,
        error: "Payment Required — send BNKR (primary) or USDC to treasury on Base, then POST /v1/payment/topup { \"tx_hash\": \"0x...\", \"currency\": \"BNKR\" }",
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
    /// No qualifying Transfer to treasury found in this transaction
    NoQualifyingTransfer,
    /// Amount is below minimum
    BelowMinimum(u64),
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHash => write!(f, "invalid_tx_hash"),
            Self::RpcError(e) => write!(f, "rpc_error: {e}"),
            Self::TxNotFound => write!(f, "tx_not_found"),
            Self::TxFailed => write!(f, "tx_failed_on_chain"),
            Self::NoQualifyingTransfer => write!(f, "no_qualifying_transfer_to_treasury"),
            Self::BelowMinimum(m) => write!(f, "below_minimum_{m}"),
        }
    }
}

#[derive(serde::Deserialize)]
struct RpcResponse {
    result: Option<serde_json::Value>,
}

/// Shared receipt fetcher — returns parsed receipt JSON.
async fn fetch_receipt(tx_hash: &str, rpc_url: &str) -> Result<serde_json::Value, VerifyError> {
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

    match rpc_resp.result {
        None | Some(serde_json::Value::Null) => Err(VerifyError::TxNotFound),
        Some(v) => {
            let status = v["status"].as_str().unwrap_or("0x0");
            if status != "0x1" {
                return Err(VerifyError::TxFailed);
            }
            Ok(v)
        }
    }
}

/// Verify a USDC transfer to treasury on Base mainnet.
///
/// Returns the USDC amount in 6-decimal units (e.g. 1_000_000 = 1.00 USDC).
pub async fn verify_usdc_topup(tx_hash: &str, rpc_url: &str) -> Result<u64, VerifyError> {
    let receipt = fetch_receipt(tx_hash, rpc_url).await?;

    let logs = receipt["logs"]
        .as_array()
        .ok_or(VerifyError::NoQualifyingTransfer)?;

    let treasury_lower = TREASURY_ADDRESS.to_lowercase();
    let usdc_lower = USDC_CONTRACT_BASE.to_lowercase();

    for log in logs {
        let contract = log["address"].as_str().unwrap_or("").to_lowercase();
        if contract != usdc_lower {
            continue;
        }

        let topics = match log["topics"].as_array() {
            Some(t) if t.len() >= 3 => t,
            _ => continue,
        };

        if topics[0].as_str().unwrap_or("") != ERC20_TRANSFER_TOPIC {
            continue;
        }

        let to_topic = topics[2].as_str().unwrap_or("");
        if to_topic.len() != 66 {
            continue;
        }
        let to_addr = format!("0x{}", &to_topic[26..]);
        if to_addr.to_lowercase() != treasury_lower {
            continue;
        }

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

/// Verify a BNKR transfer to treasury on Base mainnet.
///
/// BNKR has 18 decimals. We parse the on-chain wei-amount as u128,
/// divide by 10^18 to get whole BNKR, then check against the minimum.
///
/// Returns whole BNKR units (e.g. 1000 = 1000 BNKR ≈ $0.55).
pub async fn verify_bnkr_topup(
    tx_hash: &str,
    rpc_url: &str,
    bnkr_contract: &str,
) -> Result<u64, VerifyError> {
    let receipt = fetch_receipt(tx_hash, rpc_url).await?;

    let logs = receipt["logs"]
        .as_array()
        .ok_or(VerifyError::NoQualifyingTransfer)?;

    let treasury_lower = TREASURY_ADDRESS.to_lowercase();
    let bnkr_lower = bnkr_contract.to_lowercase();

    for log in logs {
        let contract = log["address"].as_str().unwrap_or("").to_lowercase();
        if contract != bnkr_lower {
            continue;
        }

        let topics = match log["topics"].as_array() {
            Some(t) if t.len() >= 3 => t,
            _ => continue,
        };

        if topics[0].as_str().unwrap_or("") != ERC20_TRANSFER_TOPIC {
            continue;
        }

        let to_topic = topics[2].as_str().unwrap_or("");
        if to_topic.len() != 66 {
            continue;
        }
        let to_addr = format!("0x{}", &to_topic[26..]);
        if to_addr.to_lowercase() != treasury_lower {
            continue;
        }

        // BNKR data: 32-byte big-endian amount in wei-BNKR (18 decimals)
        // Parse as u128 to avoid u64 overflow (1 BNKR = 10^18 which overflows u64)
        let data = log["data"].as_str().unwrap_or("");
        let amount_hex = if data.starts_with("0x") && data.len() == 66 {
            &data[2..]
        } else {
            continue;
        };

        let amount_wei = u128::from_str_radix(amount_hex, 16)
            .map_err(|_| VerifyError::NoQualifyingTransfer)?;

        // Convert wei-BNKR → whole BNKR
        let bnkr_whole = (amount_wei / BNKR_DECIMALS) as u64;

        if bnkr_whole < MIN_TOPUP_BNKR_WHOLE {
            return Err(VerifyError::BelowMinimum(MIN_TOPUP_BNKR_WHOLE));
        }

        return Ok(bnkr_whole);
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
    fn test_bnkr_to_virtual_conversion() {
        // 1000 BNKR @ $0.00055 = $0.55 / $0.65 = 0.846 VIRTUAL = 846_153 μVIRTUAL
        let result = bnkr_whole_to_virtual_micro(1000);
        assert!(result > 800_000, "1000 BNKR must give > 0.8 VIRTUAL, got {result}");
        assert!(result < 900_000, "1000 BNKR must give < 0.9 VIRTUAL, got {result}");

        // 0 BNKR → 0 VIRTUAL
        assert_eq!(bnkr_whole_to_virtual_micro(0), 0);

        // 1_000_000 BNKR @ $550 / $0.65 ≈ 846 VIRTUAL = 846_153_846 μVIRTUAL
        let large = bnkr_whole_to_virtual_micro(1_000_000);
        assert!(large > 800_000_000, "1M BNKR must give > 800 VIRTUAL");
    }

    #[test]
    fn test_payment_required_response_usdc_always_present() {
        // Without HELM_BNKR_CONTRACT set, USDC-only response
        let r = payment_required_response("https://api.helm.xyz/v1/payment/topup");
        assert!(!r.accepts.is_empty(), "Must have at least USDC option");
        let usdc_opt = r.accepts.iter().find(|o| o.extra.name == "USD Coin");
        assert!(usdc_opt.is_some(), "USDC option must always be present");
        if let Some(opt) = usdc_opt {
            assert_eq!(opt.asset, USDC_CONTRACT_BASE);
            assert_eq!(opt.pay_to, TREASURY_ADDRESS);
            assert_eq!(opt.extra.decimals, 6);
        }
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
        assert!(matches!(
            verify_bnkr_topup("not_a_hash", "http://unused", "0x1234").await,
            Err(VerifyError::InvalidHash)
        ));
    }
}
