// crates/helm-node/src/cli/init.rs
// helm init — DID 생성 + Gateway 등록 자동화
//
// 모든 사용자 (에이전트 & Gateway 호스트 공통)
//
// 실행 결과:
//   ~/.helm/config.json  ← DID, gateway_url, jwt_token 저장
//   ~/.helm/key.pem      ← Ed25519 private key (로컬에만, 절대 전송 안 됨)

use std::path::PathBuf;
use anyhow::Result;
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde_json::json;

use super::gateway_commands::HelmConfig;

const DEFAULT_GATEWAY: &str = "https://gateway.helm-protocol.io";

pub struct InitResult {
    pub did: String,
    pub gateway_url: String,
    pub is_new: bool,
}

/// `helm init [--gateway <url>] [--referrer <did>]`
pub async fn cmd_init(
    gateway_url: Option<String>,
    referrer_did: Option<String>,
    force: bool,
) -> Result<()> {
    let helm_dir = helm_dir()?;
    std::fs::create_dir_all(&helm_dir)?;

    let config_path = helm_dir.join("config.json");
    let key_path    = helm_dir.join("key.pem");

    // 이미 초기화된 경우
    if config_path.exists() && !force {
        let cfg = HelmConfig::load();
        if let Some(c) = cfg {
            println!("✅ Already initialized");
            println!("   DID:     {}", c.did);
            println!("   Gateway: {}", c.gateway_url);
            println!("   (Use --force to re-initialize)");
            return Ok(());
        }
    }

    let gw = gateway_url.unwrap_or_else(|| DEFAULT_GATEWAY.to_string());

    println!("╔══════════════════════════════════════════╗");
    println!("║  Helm Protocol — Initialization          ║");
    println!("╚══════════════════════════════════════════╝");
    println!();

    // ── 1. Ed25519 Keypair 생성 ──────────────────────────────────
    println!("🔑 Generating Ed25519 keypair...");
    let signing_key  = SigningKey::generate(&mut OsRng);
    let verifying_key: VerifyingKey = signing_key.verifying_key();

    // private key를 로컬에 저장 (PEM 형식)
    let private_bytes = signing_key.to_bytes();
    let private_b64   = base64_encode(&private_bytes);
    let pem = format!(
        "-----BEGIN HELM PRIVATE KEY-----\n{}\n-----END HELM PRIVATE KEY-----\n",
        private_b64
    );
    std::fs::write(&key_path, &pem)?;

    // key.pem 권한을 600으로 설정 (소유자만 읽기)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
    }

    println!("✅ Private key saved: ~/.helm/key.pem (never transmitted)");

    // ── 2. DID 생성 (did:helm:<base58(sha3(pubkey))>) ────────────
    let pubkey_bytes = verifying_key.to_bytes();
    let did          = derive_did(&pubkey_bytes);
    println!("🆔 DID: {}", did);

    // ── 3. Gateway에 DID 등록 ────────────────────────────────────
    println!();
    println!("📡 Registering with Gateway: {}", gw);

    let nonce     = generate_nonce();
    let signature = sign_registration(&signing_key, &did, &nonce);

    let reg_body = json!({
        "global_did": did,
        "public_key": hex::encode(pubkey_bytes),
        "nonce": nonce,
        "signature": hex::encode(signature),
        "referrer_did": referrer_did,
    });

    let client  = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let response = client
        .post(format!("{}/auth/exchange", gw))
        .json(&reg_body)
        .send()
        .await;

    let jwt_token = match response {
        Ok(r) if r.status().is_success() => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            let token = body["token"].as_str().map(|s| s.to_string());
            if token.is_some() {
                println!("✅ Gateway registration successful");
                if let Some(ref ref_did) = referrer_did {
                    println!("   Referrer: {}", ref_did);
                }
            }
            token
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            println!("⚠️  Gateway registration failed: {} — {}", status, body);
            println!("   You can retry with: helm init --gateway {}", gw);
            None
        }
        Err(e) => {
            println!("⚠️  Cannot reach Gateway: {}", e);
            println!("   Saved config locally. Retry: helm init --gateway {}", gw);
            None
        }
    };

    // ── 4. Config 저장 ───────────────────────────────────────────
    let config = HelmConfig {
        did: did.clone(),
        gateway_url: gw.clone(),
        jwt_token,
    };
    config.save()?;
    println!("✅ Config saved: ~/.helm/config.json");

    // ── 5. 다음 단계 안내 ────────────────────────────────────────
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  DID: {}", did);
    println!("  Gateway: {}", gw);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Next steps:");
    println!("  helm api list                    # 사용 가능한 API 보기");
    println!("  helm api call --service filter   # API 호출 테스트");
    println!("  helm marketplace list            # 마켓플레이스 보기");
    println!();
    println!("To host your own Gateway (earn BNKR from API traffic):");
    println!("  npm install -g @helm-protocol/helm-gateway");
    println!("  helm-gateway init && helm-gateway start");

    Ok(())
}

// ── Helper Functions ──────────────────────────────────────────────

fn helm_dir() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".helm"))
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))
}

fn derive_did(pubkey: &[u8]) -> String {
    use sha3::{Digest, Sha3_256};
    let hash = Sha3_256::digest(pubkey);
    let id   = bs58::encode(&hash[..20]).into_string();
    format!("did:helm:{}", id)
}

fn generate_nonce() -> String {
    use rand::Rng;
    let bytes: [u8; 16] = rand::thread_rng().gen();
    hex::encode(bytes)
}

fn sign_registration(
    key: &SigningKey,
    did: &str,
    nonce: &str,
) -> [u8; 64] {
    use ed25519_dalek::Signer;
    let message = format!("helm-register:{}:{}", did, nonce);
    let sig = key.sign(message.as_bytes());
    sig.to_bytes()
}

fn base64_encode(data: &[u8]) -> String {
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::STANDARD.encode(data)
}
