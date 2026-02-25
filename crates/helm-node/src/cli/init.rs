// crates/helm-node/src/cli/init.rs
// helm init — DID 생성 + Gateway 등록 자동화
//
// Two authentication strategies:
//   default  → Ed25519 keypair (pure agent / machine identity)
//   --github → GitHub OAuth Device Flow + Ed25519 keypair
//              (human-readable social proof linked to DID)
//
// 실행 결과:
//   ~/.helm/config.json  ← DID, gateway_url, jwt_token, github_login? 저장
//   ~/.helm/key.pem      ← Ed25519 private key (로컬에만, 절대 전송 안 됨)

use std::path::PathBuf;
use anyhow::Result;
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::gateway_commands::HelmConfig;
use super::github_oauth;

const DEFAULT_GATEWAY: &str = "https://gateway.helm.ag";

pub struct InitResult {
    pub did: String,
    pub gateway_url: String,
    pub is_new: bool,
}

/// `helm init [--gateway <url>] [--referrer <did>] [--github] [--force]`
pub async fn cmd_init(
    gateway_url: Option<String>,
    referrer_did: Option<String>,
    use_github: bool,
    force: bool,
) -> Result<()> {
    let helm_dir = helm_dir()?;
    std::fs::create_dir_all(&helm_dir)?;

    let config_path = helm_dir.join("config.json");
    let key_path    = helm_dir.join("key.pem");

    // Already initialized?
    if config_path.exists() && !force {
        let cfg = HelmConfig::load();
        if let Some(c) = cfg {
            println!("✅ Already initialized");
            println!("   DID:     {}", c.did);
            println!("   Gateway: {}", c.gateway_url);
            if let Some(gh) = c.github_login {
                println!("   GitHub:  @{}", gh);
            }
            println!("   (Use --force to re-initialize)");
            return Ok(());
        }
    }

    let gw = gateway_url.unwrap_or_else(|| DEFAULT_GATEWAY.to_string());

    println!("╔══════════════════════════════════════════╗");
    println!("║  Helm Protocol — Initialization          ║");
    println!("╚══════════════════════════════════════════╝");
    println!();

    // ── (Optional) GitHub OAuth ───────────────────────────────────
    let github_identity = if use_github {
        println!("🐙 Authenticating via GitHub OAuth...");
        match github_oauth::authenticate().await {
            Ok(id) => Some(id),
            Err(e) => {
                println!("⚠️  GitHub login failed: {}", e);
                println!("   Falling back to Ed25519-only identity");
                None
            }
        }
    } else {
        None
    };

    // ── 1. Ed25519 Keypair 생성 ───────────────────────────────────
    println!("🔑 Generating Ed25519 keypair...");
    let signing_key   = SigningKey::generate(&mut OsRng);
    let verifying_key: VerifyingKey = signing_key.verifying_key();

    // Save private key locally (PEM)
    let private_bytes = signing_key.to_bytes();
    let private_b64   = base64_encode(&private_bytes);
    let pem = format!(
        "-----BEGIN HELM PRIVATE KEY-----\n{}\n-----END HELM PRIVATE KEY-----\n",
        private_b64
    );
    std::fs::write(&key_path, &pem)?;

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

    if let Some(ref gh) = github_identity {
        println!("🐙 GitHub: @{} (id {})", gh.login, gh.id);
    }

    // ── 3. Gateway에 DID 등록 ─────────────────────────────────────
    println!();
    println!("📡 Registering with Gateway: {}", gw);

    let nonce     = generate_nonce();
    let signature = sign_registration(&signing_key, &did, &nonce);

    let mut reg_body = json!({
        "global_did":  did,
        "public_key":  hex::encode(pubkey_bytes),
        "nonce":       nonce,
        "signature":   hex::encode(signature),
        "referrer_did": referrer_did,
    });

    // Attach GitHub identity if available (social proof — optional on Gateway)
    if let Some(ref gh) = github_identity {
        reg_body["github_login"] = json!(gh.login);
        reg_body["github_id"]    = json!(gh.id);
        if let Some(ref email) = gh.email {
            reg_body["github_email"] = json!(email);
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("helm-protocol-cli/0.1")
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
            let body   = r.text().await.unwrap_or_default();
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

    // ── 4. Config 저장 ────────────────────────────────────────────
    let config = HelmConfig {
        did: did.clone(),
        gateway_url: gw.clone(),
        jwt_token,
        github_login: github_identity.as_ref().map(|gh| gh.login.clone()),
    };
    config.save()?;
    println!("✅ Config saved: ~/.helm/config.json");

    // ── 5. 다음 단계 안내 ─────────────────────────────────────────
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  DID: {}", did);
    if let Some(ref gh) = github_identity {
        println!("  GitHub: @{}", gh.login);
    }
    println!("  Gateway: {}", gw);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Next steps:");
    println!("  helm api list                    # Browse marketplace APIs");
    println!("  helm api call --listing-id <id>  # Call an API");
    println!("  helm status                      # Credits & referral info");

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────

fn helm_dir() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".helm"))
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))
}

fn derive_did(pubkey: &[u8]) -> String {
    use sha3::{Digest, Sha3_256};
    let hash  = Sha3_256::digest(pubkey);
    let id    = bs58::encode(&hash[..20]).into_string();
    format!("did:helm:{}", id)
}

fn generate_nonce() -> String {
    use rand::Rng;
    let bytes: [u8; 16] = rand::thread_rng().gen();
    hex::encode(bytes)
}

fn sign_registration(key: &SigningKey, did: &str, nonce: &str) -> [u8; 64] {
    use ed25519_dalek::Signer;
    let message = format!("helm-register:{}:{}", did, nonce);
    key.sign(message.as_bytes()).to_bytes()
}

fn base64_encode(data: &[u8]) -> String {
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::STANDARD.encode(data)
}

/// Load the private key from ~/.helm/key.pem and reconstruct the SigningKey.
pub fn load_signing_key() -> Result<SigningKey> {
    let key_path = helm_dir()?.join("key.pem");
    let pem = std::fs::read_to_string(&key_path)
        .map_err(|_| anyhow::anyhow!("No key found at ~/.helm/key.pem — run `helm init` first"))?;

    let b64: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();

    use base64::{Engine as _, engine::general_purpose};
    let bytes = general_purpose::STANDARD.decode(b64.trim())?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid key length in ~/.helm/key.pem"))?;
    Ok(SigningKey::from_bytes(&arr))
}

// ── Additional config extension ───────────────────────────────────
// Re-export so main.rs can use the full HelmConfig with github_login.

/// Extended HelmConfig with GitHub login field.
/// This is stored in ~/.helm/config.json.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExtendedHelmConfig {
    pub did: String,
    pub gateway_url: String,
    pub jwt_token: Option<String>,
    pub github_login: Option<String>,
}
