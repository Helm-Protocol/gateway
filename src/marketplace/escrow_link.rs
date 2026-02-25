// src/marketplace/escrow_link.rs
// QkvgEscrow.sol 온체인 연동
//
// lock_budget()  → createAgentEscrow() 호출 → escrow_id 반환
// settle()       → settleAgentEscrow()  호출 → tx_hash 반환
// refund()       → refundAgentEscrow()  호출 → tx_hash 반환
//
// 현재: HTTP RPC 직접 호출 (alloy/ethers 없이 jsonrpc raw)
// TODO: alloy 의존성 추가 후 typed contract 바인딩으로 교체

use reqwest::Client;
use serde_json::{json, Value};
use tracing::{info, warn};

pub struct EscrowLink {
    pub contract_address: String,   // QkvgEscrow deployed address
    pub rpc_url: String,            // Base Mainnet RPC
    pub gateway_wallet: String,     // Gateway signing wallet (hex private key)
    http: Client,
}

impl EscrowLink {
    pub fn new(contract_address: String, rpc_url: String, gateway_wallet: String) -> Self {
        Self {
            contract_address,
            rpc_url,
            gateway_wallet,
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(
            std::env::var("QKVG_ESCROW_ADDRESS")
                .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".into()),
            std::env::var("BASE_RPC_URL")
                .unwrap_or_else(|_| "https://mainnet.base.org".into()),
            std::env::var("GATEWAY_WALLET_KEY").unwrap_or_default(),
        )
    }

    /// 게시 시 예산 에스크로 잠금
    /// QkvgEscrow.createAgentEscrow(payee, amount) → bytes32 escrow_id
    pub async fn lock_budget(
        &self,
        payer_did: &str,
        amount_bnkr: u64,
    ) -> Result<String, String> {
        // 컨트랙트 미배포 시 dev 모드
        if self.contract_address.starts_with("0x000000000000000000000000000000000000") {
            let fake_id = format!("dev-escrow-{}-{}", payer_did, amount_bnkr);
            info!("[escrow_link] DEV MODE — fake escrow: {}", fake_id);
            return Ok(fake_id);
        }

        // createAgentEscrow(bytes32 did_hash, uint256 amount)
        // selector: keccak256("createAgentEscrow(bytes32,uint256)")[:4]
        let calldata = self.encode_create_escrow(payer_did, amount_bnkr);

        let result = self.eth_call(&calldata).await?;

        // 반환값: bytes32 escrow_id
        // 빈 결과 또는 "0x"는 컨트랙트 미배포/호출 실패 — Err 반환 (silent fake ID 금지)
        let escrow_id = result
            .get("result")
            .and_then(|r| r.as_str())
            .filter(|s| !s.is_empty() && *s != "0x" && *s != "0x0000000000000000000000000000000000000000000000000000000000000000")
            .map(|s| s.to_string())
            .ok_or_else(|| {
                "Escrow contract returned empty result — contract may not be deployed at this address. \
                 Set QKVG_ESCROW_ADDRESS=0x0000...0000 to use dev mode.".to_string()
            })?;

        info!("[escrow_link] locked {} BNKR for {} → escrow {}", amount_bnkr, payer_did, escrow_id);
        Ok(escrow_id)
    }

    /// 납품 확인 후 에스크로 정산
    /// QkvgEscrow.settleAgentEscrow(escrow_id, winner_did) → tx_hash
    pub async fn settle(
        &self,
        escrow_id: &str,
        winner_did: &str,
        amount_bnkr: u64,
    ) -> Result<Value, String> {
        if self.contract_address.starts_with("0x000000000000000000000000000000000000") {
            let fee = (amount_bnkr as f64 * 0.02) as u64;
            let net = amount_bnkr - fee;
            info!("[escrow_link] DEV MODE — settle {} BNKR to {}", net, winner_did);
            return Ok(json!({
                "tx_hash": format!("dev-tx-{}", uuid::Uuid::new_v4()),
                "escrow_id": escrow_id,
                "winner_did": winner_did,
                "gross_bnkr": amount_bnkr,
                "protocol_fee_bnkr": fee,   // 2%
                "net_bnkr": net,             // 98%
                "dev_mode": true,
            }));
        }

        // settleAgentEscrow(bytes32 escrow_id, address winner)
        let calldata = self.encode_settle(escrow_id, winner_did);
        let result = self.eth_send(&calldata).await?;

        let tx_hash = result
            .get("result")
            .and_then(|r| r.as_str())
            .unwrap_or("0x")
            .to_string();

        let fee = (amount_bnkr as f64 * 0.02) as u64;
        Ok(json!({
            "tx_hash": tx_hash,
            "escrow_id": escrow_id,
            "winner_did": winner_did,
            "gross_bnkr": amount_bnkr,
            "protocol_fee_bnkr": fee,
            "net_bnkr": amount_bnkr - fee,
        }))
    }

    /// 에스크로 환불 (취소/기한 초과)
    pub async fn refund(&self, escrow_id: &str, payer_did: &str) -> Result<String, String> {
        if self.contract_address.starts_with("0x000000000000000000000000000000000000") {
            info!("[escrow_link] DEV MODE — refund to {}", payer_did);
            return Ok(format!("dev-refund-tx-{}", uuid::Uuid::new_v4()));
        }

        let calldata = self.encode_refund(escrow_id);
        let result = self.eth_send(&calldata).await?;
        Ok(result["result"].as_str().unwrap_or("0x").to_string())
    }

    // ── 인코딩 헬퍼 (ABI encoding 단순화) ──────────────────────────

    fn encode_create_escrow(&self, payer_did: &str, amount: u64) -> String {
        // createAgentEscrow(bytes32,uint256)
        // selector: 0x... (실제 배포 후 채움)
        let selector = "c2a69b28"; // placeholder
        let did_hash = format!("{:0>64}", hex::encode(payer_did.as_bytes()).get(..64).unwrap_or("00"));
        let amount_hex = format!("{:0>64x}", amount);
        format!("0x{}{}{}", selector, did_hash, amount_hex)
    }

    fn encode_settle(&self, escrow_id: &str, winner_did: &str) -> String {
        let selector = "8a4068dd"; // placeholder
        let eid = format!("{:0>64}", escrow_id.trim_start_matches("0x"));
        let winner_hash = format!("{:0>64}", hex::encode(winner_did.as_bytes()).get(..64).unwrap_or("00"));
        format!("0x{}{}{}", selector, eid, winner_hash)
    }

    fn encode_refund(&self, escrow_id: &str) -> String {
        let selector = "fa89401a"; // placeholder
        let eid = format!("{:0>64}", escrow_id.trim_start_matches("0x"));
        format!("0x{}{}", selector, eid)
    }

    // ── RPC 호출 ────────────────────────────────────────────────────

    async fn eth_call(&self, data: &str) -> Result<Value, String> {
        let body = json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": self.contract_address,
                "data": data,
            }, "latest"],
            "id": 1
        });
        self.http.post(&self.rpc_url)
            .json(&body)
            .send().await
            .map_err(|e| e.to_string())?
            .json::<Value>().await
            .map_err(|e| e.to_string())
    }

    async fn eth_send(&self, _data: &str) -> Result<Value, String> {
        // ⚠️  eth_sendRawTransaction 미구현 — 프로덕션 배포 불가
        //
        // 구현 필요 사항:
        //   1. alloy / ethers-rs 의존성 추가
        //   2. gateway_wallet (hex privkey) → LocalWallet 생성
        //   3. TransactionRequest 구성 → sign → sendRawTransaction
        //
        // 현재: 프로덕션 모드에서 settle/refund 호출 자체를 Err로 차단
        // (DEV 모드: contract_address = 0x000...000 → 위 분기에서 처리)
        warn!("[escrow_link] eth_sendRawTransaction not implemented — settlement blocked in production mode");
        Err(
            "On-chain settlement is not yet implemented. \
             Use dev mode (QKVG_ESCROW_ADDRESS=0x0000000000000000000000000000000000000000) \
             or implement eth_sendRawTransaction with alloy/ethers-rs before production launch."
                .to_string()
        )
    }
}
