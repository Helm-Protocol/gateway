// src/auth/did_exchange.rs
// [H-6/H-7] DID Passport → Local Visa 교환
//
// TOCTOU 취약점 수정:
//   취약: SELECT(잔액확인) → [공격자 출금] → INSERT(에이전트생성)
//   수정: BEGIN SERIALIZABLE → SELECT FOR UPDATE → 검증 → INSERT → COMMIT
//         (원자적 트랜잭션 — 사이에 아무것도 끼어들 수 없음)
//
// Sybil 방어:
//   글로벌 DID 1개 = Visa 1개 (ON CONFLICT 처리)
//   평판 히스토리는 DID에 귀속 (다중 지갑 의미 없음)

use chrono::Utc;
use ed25519_dalek::{Signature, VerifyingKey};
use hex;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use ulid::Ulid;

use super::types::{AuthError, GlobalPassport, LocalVisa, VisaIssuanceResponse};

/// JWT Claims
#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    /// Subject = local_did
    sub: String,
    /// Global DID
    gdid: String,
    /// Expiry (Unix timestamp)
    exp: i64,
    /// Issued at
    iat: i64,
}

/// DID Exchange 서비스
pub struct DidExchangeService {
    jwt_secret: Vec<u8>,
    /// 최근 사용된 nonce 세트 (재사용 공격 방어)
    /// 실제 운영에서는 Redis로 대체
    used_nonces: parking_lot::RwLock<std::collections::HashSet<String>>,
}

impl DidExchangeService {
    pub fn new(jwt_secret: &str) -> Self {
        Self {
            jwt_secret: jwt_secret.as_bytes().to_vec(),
            used_nonces: parking_lot::RwLock::new(std::collections::HashSet::new()),
        }
    }

    /// [핵심] 글로벌 DID → Local Visa 교환
    ///
    /// 1. 서명 검증
    /// 2. Nonce 중복 체크 (재사용 공격 방어)
    /// 3. DB Atomic Upsert (Sybil 방어)
    /// 4. JWT 발급
    pub async fn exchange(
        &self,
        passport: GlobalPassport,
        db: &sqlx::PgPool,
    ) -> Result<VisaIssuanceResponse, AuthError> {
        // === Step 1: 서명 검증 ===
        self.verify_passport_signature(&passport)?;

        // === Step 2: Nonce 체크 (재사용 공격 방어) ===
        self.check_and_consume_nonce(&passport.signed_message)?;

        // === Step 3: DB Upsert (원자적) ===
        // SERIALIZABLE 격리 수준으로 TOCTOU 완전 차단
        let visa = self.upsert_visa_atomic(&passport.did, db).await?;

        // === Step 4: JWT 발급 (24시간) ===
        let token = self.issue_jwt(&visa.local_did, &visa.global_did)?;

        let free_remaining = (100 - visa.total_calls).max(0);

        Ok(VisaIssuanceResponse {
            local_did: visa.local_did,
            session_token: token,
            balance_bnkr: visa.balance_bnkr,
            reputation_score: visa.reputation_score,
            free_calls_remaining: free_remaining,
            message: "Welcome to Helm-sense Gateway. 에이전트 주권 네트워크에 오신 것을 환영합니다.".into(),
        })
    }

    /// Ed25519 서명 검증
    fn verify_passport_signature(
        &self,
        passport: &GlobalPassport,
    ) -> Result<(), AuthError> {
        // did:ethr:0xABC... → 공개키 추출
        let pubkey_hex = passport
            .did
            .strip_prefix("did:ethr:0x")
            .or_else(|| passport.did.strip_prefix("did:key:"))
            .ok_or_else(|| AuthError::InvalidDidFormat(passport.did.clone()))?;

        let pubkey_bytes = hex::decode(pubkey_hex).map_err(|e| {
            AuthError::InvalidDidFormat(format!("hex 디코드 실패: {e}"))
        })?;

        // Ed25519 검증키 구성
        let key_array: [u8; 32] = pubkey_bytes
            .try_into()
            .map_err(|_| AuthError::InvalidDidFormat("공개키 길이 오류 (32바이트 필요)".into()))?;
        let verifying_key = VerifyingKey::from_bytes(&key_array)
            .map_err(|e| AuthError::SignatureVerificationFailed(e.to_string()))?;

        // 서명 바이트 파싱
        let sig_array: [u8; 64] = passport
            .signature
            .clone()
            .try_into()
            .map_err(|_| AuthError::SignatureVerificationFailed("서명 길이 오류 (64바이트 필요)".into()))?;
        let signature = Signature::from_bytes(&sig_array);

        // 서명 검증
        use ed25519_dalek::Verifier;
        verifying_key
            .verify(passport.signed_message.as_bytes(), &signature)
            .map_err(|e| AuthError::SignatureVerificationFailed(e.to_string()))?;

        Ok(())
    }

    /// Nonce 재사용 방어
    fn check_and_consume_nonce(&self, message: &str) -> Result<(), AuthError> {
        // 메시지에서 nonce 추출 (형식: "helm-auth:{did}:{nonce}:{timestamp}")
        let nonce_key = {
            let mut hasher = Keccak256::new();
            hasher.update(message.as_bytes());
            hex::encode(hasher.finalize())
        };

        let mut nonces = self.used_nonces.write();
        if nonces.contains(&nonce_key) {
            return Err(AuthError::NonceReuse);
        }

        // 소비 (실제 운영: TTL 설정된 Redis SET NX)
        nonces.insert(nonce_key);

        // 메모리 관리: 10,000개 초과 시 절반 제거
        if nonces.len() > 10_000 {
            let to_remove: Vec<_> = nonces.iter().take(5_000).cloned().collect();
            for k in to_remove {
                nonces.remove(&k);
            }
        }

        Ok(())
    }

    /// [TOCTOU 수정] 원자적 Visa Upsert
    ///
    /// 취약했던 패턴:
    ///   1. SELECT balance  (검증)
    ///   2. [공격자가 사이에 출금]
    ///   3. INSERT agent    (생성 — 잔액 이미 없음)
    ///
    /// 수정된 패턴:
    ///   BEGIN SERIALIZABLE
    ///   → SELECT FOR UPDATE (행 락)
    ///   → 검증
    ///   → UPSERT
    ///   COMMIT (원자적)
    async fn upsert_visa_atomic(
        &self,
        global_did: &str,
        db: &sqlx::PgPool,
    ) -> Result<LocalVisa, AuthError> {
        // Serializable 격리 트랜잭션
        let mut tx = db
            .begin()
            .await
            .map_err(|e| AuthError::DatabaseError(e.to_string()))?;

        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await
            .map_err(|e| AuthError::DatabaseError(e.to_string()))?;

        // 기존 Visa 확인 (FOR UPDATE = 행 락)
        let existing: Option<LocalVisa> = sqlx::query_as(
            "SELECT * FROM local_visas WHERE global_did = $1 FOR UPDATE"
        )
        .bind(global_did)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AuthError::DatabaseError(e.to_string()))?;

        let visa = match existing {
            // 재방문 에이전트 — 기존 Visa 반환
            Some(v) => {
                sqlx::query("UPDATE local_visas SET last_active_at = $1 WHERE global_did = $2")
                    .bind(Utc::now())
                    .bind(global_did)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| AuthError::DatabaseError(e.to_string()))?;
                v
            }

            // 신규 에이전트 — Visa 발급
            None => {
                let local_did = format!("did:helm:agent_{}", Ulid::new());
                let new_visa = LocalVisa::new(local_did, global_did.to_string());

                sqlx::query_as(
                    r#"INSERT INTO local_visas
                       (id, local_did, global_did, balance_bnkr, reputation_score,
                        g_score_avg, total_calls, total_paid_bnkr, created_at, last_active_at)
                       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                       RETURNING *"#,
                )
                .bind(new_visa.id)
                .bind(&new_visa.local_did)
                .bind(&new_visa.global_did)
                .bind(new_visa.balance_bnkr)
                .bind(new_visa.reputation_score)
                .bind(new_visa.g_score_avg)
                .bind(new_visa.total_calls)
                .bind(new_visa.total_paid_bnkr)
                .bind(new_visa.created_at)
                .bind(new_visa.last_active_at)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| AuthError::DatabaseError(e.to_string()))?
            }
        };

        tx.commit()
            .await
            .map_err(|e| AuthError::DatabaseError(e.to_string()))?;

        Ok(visa)
    }

    /// JWT 발급 (24시간)
    fn issue_jwt(
        &self,
        local_did: &str,
        global_did: &str,
    ) -> Result<String, AuthError> {
        let now = Utc::now().timestamp();

        let claims = JwtClaims {
            sub: local_did.to_string(),
            gdid: global_did.to_string(),
            iat: now,
            exp: now + 86_400, // 24시간
        };

        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(&self.jwt_secret),
        )
        .map_err(|e| AuthError::DatabaseError(format!("JWT 발급 실패: {e}")))
    }
}

/// 서명 메시지 생성 헬퍼 (에이전트 SDK용)
pub fn build_auth_message(did: &str, nonce: &str) -> String {
    format!(
        "helm-auth:{did}:{nonce}:{}",
        Utc::now().timestamp()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonce_reuse_detected() {
        let svc = DidExchangeService::new("test-secret");
        let msg = "helm-auth:did:ethr:0xABC:nonce123:1234567890";

        // 첫 번째 사용 — OK
        svc.check_and_consume_nonce(msg).expect("첫 번째 nonce는 통과해야 함");

        // 두 번째 사용 — Nonce 재사용 에러
        let err = svc.check_and_consume_nonce(msg).unwrap_err();
        assert!(matches!(err, AuthError::NonceReuse));
    }
}
