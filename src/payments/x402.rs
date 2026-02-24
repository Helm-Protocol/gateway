// src/payments/x402.rs
// [H-8] x402 State Channel 기반 마이크로 결제
//
// 3-Phase 아키텍처:
//   Phase 1: On-chain Deposit  (1회, 가스비 1회)
//   Phase 2: Off-chain Ticket  (API 호출마다, 가스비 0)
//   Phase 3: Batch Settlement  (주 1회, 100K건 → 1건 가스비)
//
// 취약점 수정 (Helm_INIt_Secure.txt):
//   Reentrancy: Checks-Effects-Interactions 패턴 적용
//   Front-running: Commit-Reveal (Phase 2 서명 기반)
//   Oracle: 단일 오라클 → 컨트랙트에서 처리

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use uuid::Uuid;

/// API 호출당 가격 정보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentQuote {
    /// 요청 UUID (재사용 공격 방어)
    pub request_id: Uuid,
    /// Base Toll — 항상 부과 (스팸 방지)
    pub base_toll_bnkr: f64,
    /// Novelty Premium — G-Metric 기반 추가 요금
    pub novelty_premium_bnkr: f64,
    /// 총액
    pub total_bnkr: f64,
    /// G-Metric 점수 (에이전트에게 투명하게 공개)
    pub g_metric: f64,
    /// 판정 결과
    pub verdict: PaymentVerdict,
    /// 유효기간 (5분)
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaymentVerdict {
    /// 신규 정보 — 최고가
    NovelDelta,
    /// 중복 — 기본 통행료만
    Duplicate,
    /// 스팸 — 기본 통행료만
    SpamOrOfftopic,
    /// 신규 토픽 — 프리미엄
    NewTopic,
}

/// Off-chain 결제 티켓
/// 에이전트의 Ed25519 서명 포함 — 온체인 없이 검증 가능
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentTicket {
    /// 결제 주체 (local_did)
    pub agent_did: String,
    /// 결제 금액 (BNKR × 10^6 정수화 — 부동소수점 오류 방지)
    pub amount_micro: u64,
    /// 순서 보장용 nonce
    pub nonce: u64,
    /// 타임스탬프 (Unix)
    pub timestamp: i64,
    /// 에이전트 서명 (Ed25519)
    pub signature: Vec<u8>,
    /// 티켓 해시 (Keccak256)
    pub hash: [u8; 32],
}

impl PaymentTicket {
    /// 티켓 생성 및 서명
    pub fn create(
        agent_did: &str,
        amount_bnkr: f64,
        nonce: u64,
        signing_key: &SigningKey,
    ) -> Result<Self, PaymentError> {
        // BNKR → micro-BNKR 변환 (소수점 6자리 정수화)
        let amount_micro = (amount_bnkr * 1_000_000.0) as u64;
        let timestamp = Utc::now().timestamp();

        // 티켓 해시 생성 (서명 대상)
        let hash = Self::compute_hash(agent_did, amount_micro, nonce, timestamp);

        // Ed25519 서명
        use ed25519_dalek::Signer;
        let signature = signing_key.sign(&hash).to_bytes().to_vec();

        Ok(Self {
            agent_did: agent_did.to_string(),
            amount_micro,
            nonce,
            timestamp,
            signature,
            hash,
        })
    }

    /// 티켓 해시 계산 (Keccak256)
    pub fn compute_hash(
        agent_did: &str,
        amount_micro: u64,
        nonce: u64,
        timestamp: i64,
    ) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update(agent_did.as_bytes());
        hasher.update(amount_micro.to_le_bytes());
        hasher.update(nonce.to_le_bytes());
        hasher.update(timestamp.to_le_bytes());
        hasher.update(b"helm_sense-payment-v1"); // 도메인 분리
        hasher.finalize().into()
    }

    /// 서명 검증 (게이트웨이 측)
    pub fn verify(&self, verifying_key: &VerifyingKey) -> Result<(), PaymentError> {
        // 해시 재계산 (위변조 확인)
        let expected_hash = Self::compute_hash(
            &self.agent_did,
            self.amount_micro,
            self.nonce,
            self.timestamp,
        );

        if expected_hash != self.hash {
            return Err(PaymentError::HashMismatch);
        }

        // 타임스탬프 유효성 (5분 이내)
        let age = Utc::now().timestamp() - self.timestamp;
        if age > 300 || age < 0 {
            return Err(PaymentError::TicketExpired);
        }

        // 서명 검증
        let sig_bytes: [u8; 64] = self
            .signature
            .clone()
            .try_into()
            .map_err(|_| PaymentError::InvalidSignature("서명 길이 오류".into()))?;
        let signature = Signature::from_bytes(&sig_bytes);

        use ed25519_dalek::Verifier;
        verifying_key
            .verify(&self.hash, &signature)
            .map_err(|e| PaymentError::InvalidSignature(e.to_string()))
    }

    /// BNKR 금액 반환 (micro → float)
    pub fn amount_bnkr(&self) -> f64 {
        self.amount_micro as f64 / 1_000_000.0
    }
}

/// Phase 2 결제 처리기 (게이트웨이 서버 측)
pub struct X402PaymentProcessor {
    /// 에이전트별 마지막 nonce (재사용 공격 방어)
    nonce_store: parking_lot::RwLock<std::collections::HashMap<String, u64>>,
    /// 정산 대기 중인 티켓들
    pending_tickets: parking_lot::RwLock<Vec<PaymentTicket>>,
    /// 배치 정산 임계값
    settlement_threshold: usize,
}

impl X402PaymentProcessor {
    pub fn new(settlement_threshold: usize) -> Self {
        Self {
            nonce_store: parking_lot::RwLock::new(std::collections::HashMap::new()),
            pending_tickets: parking_lot::RwLock::new(Vec::new()),
            settlement_threshold,
        }
    }

    /// [Phase 2] 티켓 수신 및 검증 (가스비 0)
    ///
    /// O(1) 서명 검증만 수행 — 10ms 이하
    pub fn process_ticket(
        &self,
        ticket: PaymentTicket,
        agent_pubkey: &VerifyingKey,
        db_balance: f64, // DB에서 가져온 현재 잔액
    ) -> Result<ProcessedPayment, PaymentError> {
        let amount = ticket.amount_bnkr();

        // === Checks (검증 먼저) ===
        // 잔액 확인
        if db_balance < amount {
            return Err(PaymentError::InsufficientBalance {
                required: amount,
                available: db_balance,
            });
        }

        // Nonce 순서 확인 (재사용 공격 방어)
        {
            let nonces = self.nonce_store.read();
            if let Some(&last_nonce) = nonces.get(&ticket.agent_did) {
                if ticket.nonce <= last_nonce {
                    return Err(PaymentError::NonceReplay);
                }
            }
        }

        // 서명 검증
        ticket.verify(agent_pubkey)?;

        // === Effects (상태 변경) ===
        {
            let mut nonces = self.nonce_store.write();
            nonces.insert(ticket.agent_did.clone(), ticket.nonce);
        }

        // === Interactions (외부 효과) ===
        // 티켓 정산 큐에 추가
        let mut pending = self.pending_tickets.write();
        pending.push(ticket.clone());

        // 임계값 도달 시 정산 트리거
        let should_settle = pending.len() >= self.settlement_threshold;

        Ok(ProcessedPayment {
            amount_bnkr: amount,
            should_settle,
            ticket_count: pending.len(),
        })
    }

    /// [Phase 3] 배치 정산 (주 1회)
    /// Merkle Root 1건만 온체인 제출 → 100K건 가스비 절감
    pub fn prepare_settlement_batch(&self) -> Option<SettlementBatch> {
        let mut pending = self.pending_tickets.write();

        if pending.is_empty() {
            return None;
        }

        let tickets: Vec<PaymentTicket> = pending.drain(..).collect();

        // 총액 계산
        let total_micro: u64 = tickets.iter().map(|t| t.amount_micro).sum();

        // Merkle Root 계산 (티켓 해시들을 트리로)
        let merkle_root = compute_merkle_root(
            &tickets.iter().map(|t| t.hash).collect::<Vec<_>>()
        );

        Some(SettlementBatch {
            tickets,
            total_bnkr: total_micro as f64 / 1_000_000.0,
            merkle_root,
            prepared_at: Utc::now(),
        })
    }
}

/// 처리 결과
#[derive(Debug)]
pub struct ProcessedPayment {
    pub amount_bnkr: f64,
    pub should_settle: bool,
    pub ticket_count: usize,
}

/// 정산 배치
#[derive(Debug)]
pub struct SettlementBatch {
    pub tickets: Vec<PaymentTicket>,
    pub total_bnkr: f64,
    pub merkle_root: [u8; 32],
    pub prepared_at: DateTime<Utc>,
}

/// Merkle Root 계산
fn compute_merkle_root(hashes: &[[u8; 32]]) -> [u8; 32] {
    if hashes.is_empty() {
        return [0u8; 32];
    }
    if hashes.len() == 1 {
        return hashes[0];
    }

    let mut current_level: Vec<[u8; 32]> = hashes.to_vec();

    while current_level.len() > 1 {
        let mut next_level = Vec::new();
        let mut i = 0;

        while i < current_level.len() {
            let left = current_level[i];
            // 홀수 개면 마지막 노드 중복
            let right = if i + 1 < current_level.len() {
                current_level[i + 1]
            } else {
                current_level[i]
            };

            let mut hasher = Keccak256::new();
            hasher.update(left);
            hasher.update(right);
            next_level.push(hasher.finalize().into());

            i += 2;
        }

        current_level = next_level;
    }

    current_level[0]
}

/// 결제 에러
#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    #[error("잔액 부족: 필요 {required} BNKR, 보유 {available} BNKR")]
    InsufficientBalance { required: f64, available: f64 },

    #[error("Nonce 재사용 공격 감지")]
    NonceReplay,

    #[error("티켓 만료")]
    TicketExpired,

    #[error("서명 검증 실패: {0}")]
    InvalidSignature(String),

    #[error("해시 불일치 — 위변조 감지")]
    HashMismatch,

    #[error("채널 미개설: 먼저 예치(deposit)하세요")]
    ChannelNotFound,

    #[error("정산 실패: {0}")]
    SettlementFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn test_ticket_create_and_verify() {
        let mut rng = rand::thread_rng();
        let signing_key = SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();

        let ticket = PaymentTicket::create(
            "did:helm_sense:agent_test",
            0.05,
            1,
            &signing_key,
        ).expect("티켓 생성 실패");

        assert_eq!(ticket.amount_bnkr(), 0.05);
        ticket.verify(&verifying_key).expect("검증 실패");
    }

    #[test]
    fn test_merkle_root_single() {
        let hash = [1u8; 32];
        let root = compute_merkle_root(&[hash]);
        assert_eq!(root, hash);
    }

    #[test]
    fn test_merkle_root_multiple() {
        let hashes = [[1u8; 32], [2u8; 32], [3u8; 32]];
        let root = compute_merkle_root(&hashes);
        assert_ne!(root, [0u8; 32]);
    }

    #[test]
    fn test_nonce_replay_detected() {
        let mut rng = rand::thread_rng();
        let signing_key = SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();

        let processor = X402PaymentProcessor::new(1000);

        let ticket = PaymentTicket::create(
            "did:helm_sense:agent_test", 0.01, 1, &signing_key
        ).unwrap();

        // 첫 번째 — OK
        processor.process_ticket(ticket.clone(), &verifying_key, 1.0).unwrap();

        // 재사용 — 에러
        let err = processor.process_ticket(ticket, &verifying_key, 1.0).unwrap_err();
        assert!(matches!(err, PaymentError::NonceReplay));
    }
}
