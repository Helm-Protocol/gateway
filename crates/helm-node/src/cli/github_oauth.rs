// crates/helm-node/src/cli/github_oauth.rs
//
// GitHub Device Authorization Flow for `helm init --github`.
//
// Flow (RFC 8628 Device Authorization Grant):
//   1. POST /login/device/code  → device_code + user_code + verification_uri
//   2. Show user: "Visit <uri> and enter <code>"
//   3. Poll POST /login/oauth/access_token until user approves (or timeout)
//   4. GET /user with Bearer token → GitHub identity (login, id, email)
//
// The returned GitHubIdentity is attached to the Helm DID registration so
// the Gateway can optionally verify social proof.
//
// Client ID is read from HELM_GITHUB_CLIENT_ID env var.
// Register a GitHub OAuth App at https://github.com/settings/developers and
// set "Device authorization flow" = enabled.

use anyhow::{bail, Result};
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// The public GitHub identity obtained after OAuth.
#[derive(Debug, Clone)]
pub struct GitHubIdentity {
    pub login: String,
    pub id: u64,
    pub email: Option<String>,
    /// Real name from GitHub profile (for display — may be None if user hides it).
    #[allow(dead_code)]
    pub name: Option<String>,
}

/// Start the device flow and wait for the user to approve.
/// Returns the GitHubIdentity on success.
pub async fn authenticate() -> Result<GitHubIdentity> {
    let client_id = std::env::var("HELM_GITHUB_CLIENT_ID")
        .unwrap_or_else(|_| "Ov23liEFKOxblC7dAaEY".to_string()); // public demo App

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("helm-protocol-cli/0.1")
        .build()?;

    // ── Step 1: Request device + user code ────────────────────────
    let device_resp = client
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .form(&[("client_id", client_id.as_str()), ("scope", "read:user,user:email")])
        .send()
        .await?
        .json::<DeviceCodeResponse>()
        .await?;

    // ── Step 2: Show user the code ─────────────────────────────────
    println!();
    println!("  ┌─────────────────────────────────────────────────┐");
    println!("  │  GitHub Login                                    │");
    println!("  │                                                  │");
    println!("  │  1. Open:  {}  │", device_resp.verification_uri);
    println!("  │  2. Enter: {}                           │", device_resp.user_code);
    println!("  │                                                  │");
    println!("  │  Waiting for approval...                         │");
    println!("  └─────────────────────────────────────────────────┘");
    println!();

    // ── Step 3: Poll for access token ─────────────────────────────
    let interval = Duration::from_secs(device_resp.interval.max(5));
    let expires  = Duration::from_secs(device_resp.expires_in);
    let deadline = Instant::now() + expires;

    let access_token = loop {
        if Instant::now() > deadline {
            bail!("GitHub login timed out — please run `helm init --github` again");
        }

        sleep(interval).await;

        let poll = client
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&[
                ("client_id",    client_id.as_str()),
                ("device_code",  device_resp.device_code.as_str()),
                ("grant_type",   "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?
            .json::<TokenResponse>()
            .await?;

        match poll.error.as_deref() {
            None => {
                // Success — access_token present
                if let Some(token) = poll.access_token {
                    break token;
                }
                bail!("GitHub returned empty token");
            }
            Some("authorization_pending") => {
                // User hasn't approved yet — keep polling
                continue;
            }
            Some("slow_down") => {
                // Back off an extra 5 s
                sleep(Duration::from_secs(5)).await;
                continue;
            }
            Some("expired_token") => {
                bail!("GitHub device code expired — please run `helm init --github` again");
            }
            Some("access_denied") => {
                bail!("GitHub login was denied by user");
            }
            Some(other) => {
                bail!("GitHub OAuth error: {}", other);
            }
        }
    };

    // ── Step 4: Fetch GitHub user info ────────────────────────────
    let user: GitHubUser = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .json()
        .await?;

    println!("✅ GitHub login successful: @{} (id {})", user.login, user.id);

    Ok(GitHubIdentity {
        login: user.login,
        id: user.id,
        email: user.email,
        name: user.name,
    })
}

// ── Internal response types ───────────────────────────────────────

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code:      String,
    user_code:        String,
    verification_uri: String,
    expires_in:       u64,
    interval:         u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error:        Option<String>,
}

#[derive(Deserialize)]
struct GitHubUser {
    login: String,
    id:    u64,
    email: Option<String>,
    name:  Option<String>,
}
