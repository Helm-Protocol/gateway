//! x402 Agent-to-Agent Payment Protocol — escrow, verification, settlement.
//!
//! Enables trustless payments between autonomous agents. A buyer agent
//! locks funds in escrow, a seller agent performs work, an optional
//! verifier agent checks quality, and funds are released on success
//! or refunded on failure/timeout.
//!
//! State machine:
//! ```text
//! Created → Funded → WorkSubmitted → Verified → Settled
//!                  ↘ Expired         ↘ Disputed → Resolved
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::token::TokenAmount;
use crate::wallet::{Address, WalletStore};

/// Unique escrow identifier.
pub type EscrowId = [u8; 32];

/// Escrow lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscrowState {
    /// Contract created, awaiting funding.
    Created,
    /// Buyer has locked funds — seller can begin work.
    Funded,
    /// Seller has submitted work — awaiting verification.
    WorkSubmitted,
    /// Verifier approved — awaiting settlement.
    Verified,
    /// Funds released to seller — terminal.
    Settled,
    /// Work rejected or disputed.
    Disputed,
    /// Dispute resolved (funds distributed per resolution).
    Resolved,
    /// Deadline passed without completion — buyer can reclaim.
    Expired,
    /// Funds returned to buyer — terminal.
    Refunded,
}

impl EscrowState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            EscrowState::Settled | EscrowState::Resolved | EscrowState::Refunded
        )
    }
}

/// Quality verification result from a verifier agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityReport {
    /// Verifier agent DID.
    pub verifier: String,
    /// Quality score [0.0, 1.0].
    pub score: f64,
    /// Minimum acceptable score for release.
    pub threshold: f64,
    /// Freeform notes.
    pub notes: String,
    /// Timestamp of verification.
    pub timestamp: u64,
}

impl QualityReport {
    /// Does this report pass the quality gate?
    pub fn passes(&self) -> bool {
        self.score >= self.threshold
    }
}

/// Dispute resolution outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    /// Fraction of escrow amount going to seller [0.0, 1.0].
    pub seller_share: f64,
    /// Resolver identity.
    pub resolver: String,
    /// Reason for the resolution.
    pub reason: String,
    /// Timestamp.
    pub timestamp: u64,
}

/// An escrow contract between two agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscrowContract {
    /// Unique ID for this escrow.
    pub id: EscrowId,
    /// Current state.
    pub state: EscrowState,
    /// Buyer agent address (payer).
    pub buyer: Address,
    /// Seller agent address (payee).
    pub seller: Address,
    /// Optional verifier agent address.
    pub verifier: Option<String>,
    /// Escrowed amount.
    pub amount: TokenAmount,
    /// Protocol fee (basis points, e.g., 40 = 0.4%).
    pub fee_bp: u32,
    /// Task description.
    pub task: String,
    /// Minimum quality score for auto-release [0.0, 1.0].
    pub quality_threshold: f64,
    /// Deadline epoch — after this, buyer can reclaim.
    pub deadline: u64,
    /// Creation timestamp.
    pub created_at: u64,
    /// Funding timestamp.
    pub funded_at: Option<u64>,
    /// Work submission timestamp.
    pub submitted_at: Option<u64>,
    /// Quality report (if verified).
    pub quality_report: Option<QualityReport>,
    /// Dispute resolution (if disputed & resolved).
    pub resolution: Option<Resolution>,
    /// Settlement timestamp.
    pub settled_at: Option<u64>,
}

impl EscrowContract {
    /// Seller's payout after protocol fee.
    pub fn seller_payout(&self) -> TokenAmount {
        let fee = self.protocol_fee();
        TokenAmount(self.amount.0.saturating_sub(fee.0))
    }

    /// Protocol fee amount.
    pub fn protocol_fee(&self) -> TokenAmount {
        // fee = amount * fee_bp / 10000
        if self.fee_bp == 0 {
            return TokenAmount::ZERO;
        }
        self.amount
            .proportional(self.fee_bp as u128, 10_000)
            .unwrap_or(TokenAmount::ZERO)
    }
}

/// x402 Payment Protocol — manages all escrow contracts.
pub struct PaymentProtocol {
    escrows: HashMap<EscrowId, EscrowContract>,
    /// Protocol fee in basis points (default 40 = 0.4%).
    pub default_fee_bp: u32,
    /// Address collecting protocol fees.
    pub fee_collector: Address,
    /// Current epoch (for deadline checks).
    current_epoch: u64,
    /// Counter for deterministic ID generation.
    id_counter: u64,
    /// Total fees collected.
    pub total_fees_collected: TokenAmount,
    /// Total volume settled.
    pub total_volume: TokenAmount,
}

impl PaymentProtocol {
    pub fn new(fee_collector: Address) -> Self {
        Self {
            escrows: HashMap::new(),
            default_fee_bp: 40, // 0.4%
            fee_collector,
            current_epoch: 0,
            id_counter: 0,
            total_fees_collected: TokenAmount::ZERO,
            total_volume: TokenAmount::ZERO,
        }
    }

    /// Advance the epoch counter.
    pub fn advance_epoch(&mut self) {
        self.current_epoch += 1;
    }

    /// Current epoch.
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Create a new escrow contract (state: Created).
    #[allow(clippy::too_many_arguments)]
    pub fn create_escrow(
        &mut self,
        buyer: &Address,
        seller: &Address,
        amount: TokenAmount,
        task: &str,
        deadline: u64,
        quality_threshold: f64,
        verifier: Option<String>,
    ) -> Result<EscrowId, X402Error> {
        if amount.0 == 0 {
            return Err(X402Error::ZeroAmount);
        }
        if deadline <= self.current_epoch {
            return Err(X402Error::InvalidDeadline);
        }
        if !(0.0..=1.0).contains(&quality_threshold) {
            return Err(X402Error::InvalidThreshold);
        }

        let id = self.next_id(buyer, seller);

        let contract = EscrowContract {
            id,
            state: EscrowState::Created,
            buyer: buyer.clone(),
            seller: seller.clone(),
            verifier,
            amount,
            fee_bp: self.default_fee_bp,
            task: task.to_string(),
            quality_threshold,
            deadline,
            created_at: self.current_epoch,
            funded_at: None,
            submitted_at: None,
            quality_report: None,
            resolution: None,
            settled_at: None,
        };

        self.escrows.insert(id, contract);
        Ok(id)
    }

    /// Fund an escrow — locks buyer's tokens. Requires external wallet transfer.
    ///
    /// Caller must first transfer `amount` from buyer's wallet to the escrow
    /// holding address, then call this to mark the escrow as funded.
    pub fn fund_escrow(
        &mut self,
        id: &EscrowId,
        wallets: &mut WalletStore,
        buyer_nonce: u64,
    ) -> Result<(), X402Error> {
        let contract = self
            .escrows
            .get(id)
            .ok_or(X402Error::NotFound)?;

        if contract.state != EscrowState::Created {
            return Err(X402Error::InvalidTransition(contract.state, EscrowState::Funded));
        }

        let buyer = contract.buyer.clone();
        let amount = contract.amount;

        // Transfer from buyer to fee_collector (acting as escrow vault)
        wallets
            .transfer(
                &buyer,
                &self.fee_collector,
                amount,
                buyer_nonce,
                &format!("x402:escrow:{}", hex_id(id)),
            )
            .map_err(|e| X402Error::TransferFailed(format!("{}", e)))?;

        let contract = self.escrows.get_mut(id).unwrap();
        contract.state = EscrowState::Funded;
        contract.funded_at = Some(self.current_epoch);

        Ok(())
    }

    /// Seller submits work completion.
    pub fn submit_work(&mut self, id: &EscrowId, seller: &Address) -> Result<(), X402Error> {
        let contract = self.escrows.get_mut(id).ok_or(X402Error::NotFound)?;

        if contract.state != EscrowState::Funded {
            return Err(X402Error::InvalidTransition(
                contract.state,
                EscrowState::WorkSubmitted,
            ));
        }
        if &contract.seller != seller {
            return Err(X402Error::Unauthorized);
        }

        contract.state = EscrowState::WorkSubmitted;
        contract.submitted_at = Some(self.current_epoch);
        Ok(())
    }

    /// Verifier submits quality report. Auto-settles if passes threshold.
    pub fn verify(
        &mut self,
        id: &EscrowId,
        verifier: &str,
        score: f64,
        notes: &str,
        wallets: &mut WalletStore,
    ) -> Result<EscrowState, X402Error> {
        let contract = self.escrows.get(id).ok_or(X402Error::NotFound)?;

        if contract.state != EscrowState::WorkSubmitted {
            return Err(X402Error::InvalidTransition(
                contract.state,
                EscrowState::Verified,
            ));
        }

        // If verifier is specified, must match
        if let Some(ref expected) = contract.verifier {
            if expected != verifier {
                return Err(X402Error::Unauthorized);
            }
        }

        let threshold = contract.quality_threshold;
        let report = QualityReport {
            verifier: verifier.to_string(),
            score,
            threshold,
            notes: notes.to_string(),
            timestamp: self.current_epoch,
        };

        let contract = self.escrows.get_mut(id).unwrap();
        let passes = report.passes();
        contract.quality_report = Some(report);

        if passes {
            contract.state = EscrowState::Verified;
            // Auto-settle
            self.settle_internal(id, wallets)?;
            Ok(EscrowState::Settled)
        } else {
            contract.state = EscrowState::Disputed;
            Ok(EscrowState::Disputed)
        }
    }

    /// Buyer directly approves and settles (no verifier needed).
    pub fn approve_and_settle(
        &mut self,
        id: &EscrowId,
        buyer: &Address,
        wallets: &mut WalletStore,
    ) -> Result<(), X402Error> {
        let contract = self.escrows.get(id).ok_or(X402Error::NotFound)?;

        if contract.state != EscrowState::WorkSubmitted
            && contract.state != EscrowState::Verified
        {
            return Err(X402Error::InvalidTransition(
                contract.state,
                EscrowState::Settled,
            ));
        }
        if &contract.buyer != buyer {
            return Err(X402Error::Unauthorized);
        }

        let contract = self.escrows.get_mut(id).unwrap();
        contract.state = EscrowState::Verified;

        self.settle_internal(id, wallets)
    }

    /// Resolve a dispute — splits escrow between buyer and seller.
    pub fn resolve_dispute(
        &mut self,
        id: &EscrowId,
        resolver: &str,
        seller_share: f64,
        reason: &str,
        wallets: &mut WalletStore,
    ) -> Result<(), X402Error> {
        let contract = self.escrows.get(id).ok_or(X402Error::NotFound)?;

        if contract.state != EscrowState::Disputed {
            return Err(X402Error::InvalidTransition(
                contract.state,
                EscrowState::Resolved,
            ));
        }
        if !(0.0..=1.0).contains(&seller_share) {
            return Err(X402Error::InvalidThreshold);
        }

        let resolution = Resolution {
            seller_share,
            resolver: resolver.to_string(),
            reason: reason.to_string(),
            timestamp: self.current_epoch,
        };

        let amount = contract.amount;
        let fee = contract.protocol_fee();
        let net = TokenAmount(amount.0.saturating_sub(fee.0));
        let seller_amount = net
            .proportional((seller_share * 10_000.0) as u128, 10_000)
            .unwrap_or(TokenAmount::ZERO);
        let buyer_refund = TokenAmount(net.0.saturating_sub(seller_amount.0));
        let seller_addr = contract.seller.clone();
        let buyer_addr = contract.buyer.clone();

        // Pay seller their share
        if seller_amount.0 > 0 {
            wallets
                .deposit(&seller_addr, seller_amount, "x402:dispute:seller-share")
                .map_err(|e| X402Error::TransferFailed(format!("{}", e)))?;
        }

        // Refund buyer their share
        if buyer_refund.0 > 0 {
            wallets
                .deposit(&buyer_addr, buyer_refund, "x402:dispute:buyer-refund")
                .map_err(|e| X402Error::TransferFailed(format!("{}", e)))?;
        }

        // Protocol fee stays with fee_collector
        let contract = self.escrows.get_mut(id).unwrap();
        contract.resolution = Some(resolution);
        contract.state = EscrowState::Resolved;
        contract.settled_at = Some(self.current_epoch);

        self.total_fees_collected = TokenAmount(
            self.total_fees_collected.0.saturating_add(fee.0),
        );
        self.total_volume = TokenAmount(
            self.total_volume.0.saturating_add(amount.0),
        );

        Ok(())
    }

    /// Expire and refund escrows past their deadline.
    pub fn expire_overdue(&mut self, wallets: &mut WalletStore) -> Vec<EscrowId> {
        let expired: Vec<EscrowId> = self
            .escrows
            .iter()
            .filter(|(_, c)| {
                !c.state.is_terminal()
                    && c.state != EscrowState::Created
                    && self.current_epoch > c.deadline
            })
            .map(|(id, _)| *id)
            .collect();

        let mut refunded = Vec::new();
        for id in expired {
            if self.refund_internal(&id, wallets).is_ok() {
                refunded.push(id);
            }
        }
        refunded
    }

    /// Get an escrow by ID.
    pub fn get(&self, id: &EscrowId) -> Option<&EscrowContract> {
        self.escrows.get(id)
    }

    /// Get all escrows for a buyer.
    pub fn buyer_escrows(&self, buyer: &Address) -> Vec<&EscrowContract> {
        self.escrows
            .values()
            .filter(|c| &c.buyer == buyer)
            .collect()
    }

    /// Get all escrows for a seller.
    pub fn seller_escrows(&self, seller: &Address) -> Vec<&EscrowContract> {
        self.escrows
            .values()
            .filter(|c| &c.seller == seller)
            .collect()
    }

    /// Total active (non-terminal) escrows.
    pub fn active_count(&self) -> usize {
        self.escrows
            .values()
            .filter(|c| !c.state.is_terminal())
            .count()
    }

    /// Total escrows ever created.
    pub fn total_count(&self) -> usize {
        self.escrows.len()
    }

    // --- Internal helpers ---

    fn settle_internal(
        &mut self,
        id: &EscrowId,
        wallets: &mut WalletStore,
    ) -> Result<(), X402Error> {
        let contract = self.escrows.get(id).ok_or(X402Error::NotFound)?;
        let payout = contract.seller_payout();
        let fee = contract.protocol_fee();
        let seller = contract.seller.clone();
        let amount = contract.amount;

        // Pay seller (from fee_collector vault)
        wallets
            .deposit(&seller, payout, "x402:settlement")
            .map_err(|e| X402Error::TransferFailed(format!("{}", e)))?;

        let contract = self.escrows.get_mut(id).unwrap();
        contract.state = EscrowState::Settled;
        contract.settled_at = Some(self.current_epoch);

        self.total_fees_collected = TokenAmount(
            self.total_fees_collected.0.saturating_add(fee.0),
        );
        self.total_volume = TokenAmount(
            self.total_volume.0.saturating_add(amount.0),
        );

        Ok(())
    }

    fn refund_internal(
        &mut self,
        id: &EscrowId,
        wallets: &mut WalletStore,
    ) -> Result<(), X402Error> {
        let contract = self.escrows.get(id).ok_or(X402Error::NotFound)?;
        let buyer = contract.buyer.clone();
        let amount = contract.amount;

        // Full refund to buyer (from fee_collector vault)
        wallets
            .deposit(&buyer, amount, "x402:refund:expired")
            .map_err(|e| X402Error::TransferFailed(format!("{}", e)))?;

        let contract = self.escrows.get_mut(id).unwrap();
        contract.state = EscrowState::Refunded;
        contract.settled_at = Some(self.current_epoch);

        Ok(())
    }

    fn next_id(&mut self, buyer: &Address, seller: &Address) -> EscrowId {
        self.id_counter += 1;
        let mut input = Vec::new();
        input.extend_from_slice(buyer.0.as_bytes());
        input.extend_from_slice(seller.0.as_bytes());
        input.extend_from_slice(&self.id_counter.to_be_bytes());
        input.extend_from_slice(&self.current_epoch.to_be_bytes());
        deterministic_hash(&input)
    }
}

impl Default for PaymentProtocol {
    fn default() -> Self {
        Self::new(Address::genesis())
    }
}

/// x402 protocol errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum X402Error {
    #[error("escrow not found")]
    NotFound,

    #[error("zero amount not allowed")]
    ZeroAmount,

    #[error("invalid deadline (must be in the future)")]
    InvalidDeadline,

    #[error("invalid threshold (must be 0.0-1.0)")]
    InvalidThreshold,

    #[error("invalid state transition: {0:?} → {1:?}")]
    InvalidTransition(EscrowState, EscrowState),

    #[error("unauthorized: caller is not the expected party")]
    Unauthorized,

    #[error("transfer failed: {0}")]
    TransferFailed(String),
}

/// Hex representation of an escrow ID (first 8 bytes).
fn hex_id(id: &EscrowId) -> String {
    id[..8]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Deterministic 32-byte hash.
fn deterministic_hash(data: &[u8]) -> [u8; 32] {
    let mut hash = [0u8; 32];
    let mut h: u64 = 5381;
    for (i, &byte) in data.iter().enumerate() {
        h = h.wrapping_mul(33).wrapping_add(byte as u64);
        hash[i % 32] ^= (h & 0xFF) as u8;
    }
    for byte in &mut hash {
        h = h.wrapping_mul(33).wrapping_add(*byte as u64);
        *byte = (h & 0xFF) as u8;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::TokenAmount;
    use crate::wallet::Address;

    fn setup() -> (PaymentProtocol, WalletStore, Address, Address) {
        let fee_addr = Address::genesis();
        let mut protocol = PaymentProtocol::new(fee_addr);
        protocol.current_epoch = 1;

        let mut wallets = WalletStore::new();
        let (buyer, _) = Address::generate();
        let (seller, _) = Address::generate();

        // Fund buyer with 1000 tokens
        wallets
            .deposit(&buyer, TokenAmount::from_tokens(1000), "initial")
            .unwrap();

        (protocol, wallets, buyer, seller)
    }

    #[test]
    fn create_escrow() {
        let (mut protocol, _, buyer, seller) = setup();

        let id = protocol
            .create_escrow(
                &buyer,
                &seller,
                TokenAmount::from_tokens(100),
                "compute task",
                10,
                0.7,
                None,
            )
            .unwrap();

        let contract = protocol.get(&id).unwrap();
        assert_eq!(contract.state, EscrowState::Created);
        assert_eq!(contract.amount, TokenAmount::from_tokens(100));
        assert_eq!(contract.task, "compute task");
    }

    #[test]
    fn create_escrow_zero_amount() {
        let (mut protocol, _, buyer, seller) = setup();
        let result = protocol.create_escrow(
            &buyer,
            &seller,
            TokenAmount::ZERO,
            "task",
            10,
            0.7,
            None,
        );
        assert!(matches!(result, Err(X402Error::ZeroAmount)));
    }

    #[test]
    fn create_escrow_invalid_deadline() {
        let (mut protocol, _, buyer, seller) = setup();
        let result = protocol.create_escrow(
            &buyer,
            &seller,
            TokenAmount::from_tokens(100),
            "task",
            0, // past
            0.7,
            None,
        );
        assert!(matches!(result, Err(X402Error::InvalidDeadline)));
    }

    #[test]
    fn create_escrow_invalid_threshold() {
        let (mut protocol, _, buyer, seller) = setup();
        let result = protocol.create_escrow(
            &buyer,
            &seller,
            TokenAmount::from_tokens(100),
            "task",
            10,
            1.5, // invalid
            None,
        );
        assert!(matches!(result, Err(X402Error::InvalidThreshold)));
    }

    #[test]
    fn fund_escrow() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id = protocol
            .create_escrow(
                &buyer,
                &seller,
                TokenAmount::from_tokens(100),
                "task",
                10,
                0.7,
                None,
            )
            .unwrap();

        protocol.fund_escrow(&id, &mut wallets, 0).unwrap();

        let contract = protocol.get(&id).unwrap();
        assert_eq!(contract.state, EscrowState::Funded);
        assert_eq!(contract.funded_at, Some(1));

        // Buyer balance should be reduced
        assert_eq!(
            wallets.balance(&buyer),
            TokenAmount::from_tokens(900)
        );
    }

    #[test]
    fn fund_escrow_insufficient_balance() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id = protocol
            .create_escrow(
                &buyer,
                &seller,
                TokenAmount::from_tokens(5000), // more than buyer has
                "task",
                10,
                0.7,
                None,
            )
            .unwrap();

        let result = protocol.fund_escrow(&id, &mut wallets, 0);
        assert!(matches!(result, Err(X402Error::TransferFailed(_))));
    }

    #[test]
    fn submit_work() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id = protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(100), "task", 10, 0.7, None)
            .unwrap();
        protocol.fund_escrow(&id, &mut wallets, 0).unwrap();

        protocol.submit_work(&id, &seller).unwrap();
        assert_eq!(protocol.get(&id).unwrap().state, EscrowState::WorkSubmitted);
    }

    #[test]
    fn submit_work_unauthorized() {
        let (mut protocol, mut wallets, buyer, seller) = setup();
        let (other, _) = Address::generate();

        let id = protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(100), "task", 10, 0.7, None)
            .unwrap();
        protocol.fund_escrow(&id, &mut wallets, 0).unwrap();

        let result = protocol.submit_work(&id, &other);
        assert!(matches!(result, Err(X402Error::Unauthorized)));
    }

    #[test]
    fn verify_and_auto_settle() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id = protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(100), "task", 10, 0.7, None)
            .unwrap();
        protocol.fund_escrow(&id, &mut wallets, 0).unwrap();
        protocol.submit_work(&id, &seller).unwrap();

        let state = protocol
            .verify(&id, "verifier-1", 0.9, "good work", &mut wallets)
            .unwrap();

        assert_eq!(state, EscrowState::Settled);
        assert_eq!(protocol.get(&id).unwrap().state, EscrowState::Settled);

        // Seller should have received payout minus fee
        let seller_balance = wallets.balance(&seller);
        assert!(seller_balance.0 > 0);
    }

    #[test]
    fn verify_fails_quality_gate() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id = protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(100), "task", 10, 0.7, None)
            .unwrap();
        protocol.fund_escrow(&id, &mut wallets, 0).unwrap();
        protocol.submit_work(&id, &seller).unwrap();

        let state = protocol
            .verify(&id, "verifier-1", 0.3, "poor quality", &mut wallets)
            .unwrap();

        assert_eq!(state, EscrowState::Disputed);
        assert_eq!(protocol.get(&id).unwrap().state, EscrowState::Disputed);
    }

    #[test]
    fn verify_wrong_verifier() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id = protocol
            .create_escrow(
                &buyer,
                &seller,
                TokenAmount::from_tokens(100),
                "task",
                10,
                0.7,
                Some("expected-verifier".to_string()),
            )
            .unwrap();
        protocol.fund_escrow(&id, &mut wallets, 0).unwrap();
        protocol.submit_work(&id, &seller).unwrap();

        let result = protocol.verify(&id, "wrong-verifier", 0.9, "ok", &mut wallets);
        assert!(matches!(result, Err(X402Error::Unauthorized)));
    }

    #[test]
    fn buyer_approve_and_settle() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id = protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(100), "task", 10, 0.7, None)
            .unwrap();
        protocol.fund_escrow(&id, &mut wallets, 0).unwrap();
        protocol.submit_work(&id, &seller).unwrap();

        protocol
            .approve_and_settle(&id, &buyer, &mut wallets)
            .unwrap();

        assert_eq!(protocol.get(&id).unwrap().state, EscrowState::Settled);
        assert!(wallets.balance(&seller).0 > 0);
    }

    #[test]
    fn resolve_dispute() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id = protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(100), "task", 10, 0.7, None)
            .unwrap();
        protocol.fund_escrow(&id, &mut wallets, 0).unwrap();
        protocol.submit_work(&id, &seller).unwrap();

        // Fail quality → Disputed
        protocol
            .verify(&id, "verifier", 0.2, "bad", &mut wallets)
            .unwrap();

        // Resolve: 30% to seller, 70% back to buyer
        protocol
            .resolve_dispute(&id, "arbiter-1", 0.3, "partial delivery", &mut wallets)
            .unwrap();

        let contract = protocol.get(&id).unwrap();
        assert_eq!(contract.state, EscrowState::Resolved);
        assert!(contract.resolution.is_some());

        let seller_bal = wallets.balance(&seller);
        let buyer_bal = wallets.balance(&buyer);

        // Seller got ~30% of net, buyer got ~70% refund + kept 900 from initial
        assert!(seller_bal.0 > 0);
        assert!(buyer_bal.0 > TokenAmount::from_tokens(900).0);
    }

    #[test]
    fn expire_overdue() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id = protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(100), "task", 5, 0.7, None)
            .unwrap();
        protocol.fund_escrow(&id, &mut wallets, 0).unwrap();

        // Advance past deadline
        for _ in 0..6 {
            protocol.advance_epoch();
        }

        let refunded = protocol.expire_overdue(&mut wallets);
        assert_eq!(refunded.len(), 1);
        assert_eq!(refunded[0], id);

        let contract = protocol.get(&id).unwrap();
        assert_eq!(contract.state, EscrowState::Refunded);

        // Buyer should be made whole
        assert_eq!(wallets.balance(&buyer), TokenAmount::from_tokens(1000));
    }

    #[test]
    fn protocol_fee_calculation() {
        let contract = EscrowContract {
            id: [0; 32],
            state: EscrowState::Created,
            buyer: Address::genesis(),
            seller: Address::genesis(),
            verifier: None,
            amount: TokenAmount::from_tokens(1000),
            fee_bp: 40, // 0.4%
            task: String::new(),
            quality_threshold: 0.7,
            deadline: 10,
            created_at: 0,
            funded_at: None,
            submitted_at: None,
            quality_report: None,
            resolution: None,
            settled_at: None,
        };

        let fee = contract.protocol_fee();
        let payout = contract.seller_payout();

        // 0.4% of 1000 = 4 tokens
        assert_eq!(fee, TokenAmount::from_tokens(4));
        assert_eq!(payout, TokenAmount::from_tokens(996));
    }

    #[test]
    fn protocol_fee_zero() {
        let contract = EscrowContract {
            id: [0; 32],
            state: EscrowState::Created,
            buyer: Address::genesis(),
            seller: Address::genesis(),
            verifier: None,
            amount: TokenAmount::from_tokens(100),
            fee_bp: 0,
            task: String::new(),
            quality_threshold: 0.7,
            deadline: 10,
            created_at: 0,
            funded_at: None,
            submitted_at: None,
            quality_report: None,
            resolution: None,
            settled_at: None,
        };

        assert_eq!(contract.protocol_fee(), TokenAmount::ZERO);
        assert_eq!(contract.seller_payout(), TokenAmount::from_tokens(100));
    }

    #[test]
    fn quality_report_passes() {
        let report = QualityReport {
            verifier: "v1".to_string(),
            score: 0.8,
            threshold: 0.7,
            notes: "good".to_string(),
            timestamp: 0,
        };
        assert!(report.passes());
    }

    #[test]
    fn quality_report_fails() {
        let report = QualityReport {
            verifier: "v1".to_string(),
            score: 0.5,
            threshold: 0.7,
            notes: "poor".to_string(),
            timestamp: 0,
        };
        assert!(!report.passes());
    }

    #[test]
    fn escrow_state_terminal() {
        assert!(EscrowState::Settled.is_terminal());
        assert!(EscrowState::Resolved.is_terminal());
        assert!(EscrowState::Refunded.is_terminal());
        assert!(!EscrowState::Created.is_terminal());
        assert!(!EscrowState::Funded.is_terminal());
        assert!(!EscrowState::Disputed.is_terminal());
    }

    #[test]
    fn buyer_and_seller_escrows() {
        let (mut protocol, _, buyer, seller) = setup();

        protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(50), "task1", 10, 0.7, None)
            .unwrap();
        protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(75), "task2", 10, 0.7, None)
            .unwrap();

        assert_eq!(protocol.buyer_escrows(&buyer).len(), 2);
        assert_eq!(protocol.seller_escrows(&seller).len(), 2);
    }

    #[test]
    fn active_and_total_count() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id1 = protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(50), "t1", 10, 0.7, None)
            .unwrap();
        protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(50), "t2", 10, 0.7, None)
            .unwrap();

        assert_eq!(protocol.active_count(), 2);
        assert_eq!(protocol.total_count(), 2);

        // Fund and settle one
        protocol.fund_escrow(&id1, &mut wallets, 0).unwrap();
        protocol.submit_work(&id1, &seller).unwrap();
        protocol
            .approve_and_settle(&id1, &buyer, &mut wallets)
            .unwrap();

        assert_eq!(protocol.active_count(), 1);
        assert_eq!(protocol.total_count(), 2);
    }

    #[test]
    fn total_volume_and_fees() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id = protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(100), "task", 10, 0.7, None)
            .unwrap();
        protocol.fund_escrow(&id, &mut wallets, 0).unwrap();
        protocol.submit_work(&id, &seller).unwrap();
        protocol
            .approve_and_settle(&id, &buyer, &mut wallets)
            .unwrap();

        assert_eq!(protocol.total_volume, TokenAmount::from_tokens(100));
        assert_eq!(protocol.total_fees_collected, TokenAmount::from_tokens(0).checked_add(
            TokenAmount::from_tokens(100).proportional(40, 10_000).unwrap()
        ).unwrap());
    }

    #[test]
    fn invalid_state_transitions() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        let id = protocol
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(100), "task", 10, 0.7, None)
            .unwrap();

        // Can't submit work before funding
        assert!(protocol.submit_work(&id, &seller).is_err());

        // Can't verify before work submitted
        assert!(protocol
            .verify(&id, "v", 0.9, "", &mut wallets)
            .is_err());

        // Can't settle before work submitted
        assert!(protocol
            .approve_and_settle(&id, &buyer, &mut wallets)
            .is_err());
    }

    #[test]
    fn full_lifecycle() {
        let (mut protocol, mut wallets, buyer, seller) = setup();

        // 1. Create
        let id = protocol
            .create_escrow(
                &buyer,
                &seller,
                TokenAmount::from_tokens(200),
                "compute ml model",
                100,
                0.8,
                Some("verifier-did".to_string()),
            )
            .unwrap();
        assert_eq!(protocol.get(&id).unwrap().state, EscrowState::Created);

        // 2. Fund
        protocol.fund_escrow(&id, &mut wallets, 0).unwrap();
        assert_eq!(protocol.get(&id).unwrap().state, EscrowState::Funded);
        assert_eq!(wallets.balance(&buyer), TokenAmount::from_tokens(800));

        // 3. Submit work
        protocol.submit_work(&id, &seller).unwrap();
        assert_eq!(protocol.get(&id).unwrap().state, EscrowState::WorkSubmitted);

        // 4. Verify (passes quality gate → auto-settles)
        let state = protocol
            .verify(&id, "verifier-did", 0.95, "excellent work", &mut wallets)
            .unwrap();
        assert_eq!(state, EscrowState::Settled);

        // 5. Check balances
        let seller_bal = wallets.balance(&seller);
        // 200 tokens - 0.4% fee = 199.2 tokens
        let expected_payout = TokenAmount::from_tokens(200)
            .proportional(10_000 - 40, 10_000)
            .unwrap();
        assert_eq!(seller_bal, expected_payout);

        // Fee collector (genesis) has the fee retained from escrow
        assert!(protocol.total_fees_collected.0 > 0);
    }

    #[test]
    fn escrow_id_deterministic() {
        let (mut p1, _, buyer, seller) = setup();
        let (mut p2, _, _, _) = setup();

        // Same inputs → same id_counter increment → different because different buyer/seller keys
        let id1 = p1
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(1), "t", 10, 0.5, None)
            .unwrap();
        let id2 = p2
            .create_escrow(&buyer, &seller, TokenAmount::from_tokens(1), "t", 10, 0.5, None)
            .unwrap();

        assert_eq!(id1, id2); // Same buyer/seller/counter/epoch → same ID
    }
}
